//! The server database: one embedded SQLite file shared by all resources.
//! Lua reaches it through the `oxmysql` resource (`@oxmysql/lib/MySQL.lua`),
//! which calls `SareDbExecute` and `SareDbTransaction` (natives.rs).
//!
//! Queries run on the server thread. SQLite in WAL mode answers small RP
//! queries in microseconds, so this stays cheap on weak PCs.
use anyhow::{bail, Context, Result};
use rusqlite::{types::Value as Sql, Connection, Statement};
use serde_json::{json, Map, Value as Json};
use std::path::{Path, PathBuf};

/// What a query call returns, after oxmysql's methods of the same name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Rows for a SELECT, otherwise `{ affectedRows, insertId, changedRows }`.
    Query,
    /// The first row, or nothing.
    Single,
    /// The first column of the first row, or nothing.
    Scalar,
    /// The new row's id.
    Insert,
    /// The number of changed rows.
    Update,
    /// One value, one row or rows, by what the query returned. A list of
    /// parameter lists runs the query once per list.
    Prepare,
}
impl Mode {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "query" | "execute" | "rawExecute" => Self::Query,
            "single" => Self::Single,
            "scalar" => Self::Scalar,
            "insert" => Self::Insert,
            "update" => Self::Update,
            "prepare" => Self::Prepare,
            _ => return None,
        })
    }
}

/// One statement's result: rows (columns in order) or a change count.
enum Outcome {
    Rows(Vec<Vec<(String, Json)>>),
    Changes { affected: u64, insert_id: i64 },
}

pub struct Db {
    /// The server folder (next to server.cfg); relative paths start here.
    root: PathBuf,
    conn: Option<Connection>,
    /// Warnings for the console, such as an ignored MySQL connection string.
    warnings: Vec<String>,
}

impl Db {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            conn: None,
            warnings: Vec::new(),
        }
    }

    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Opens the database on first use. `sare_db_file` (a convar) picks the
    /// file; the default is `db/sare.db` next to server.cfg.
    pub fn open(&mut self, file: Option<&str>, mysql: Option<&str>) -> Result<&Connection> {
        if self.conn.is_none() {
            if mysql.is_some_and(|s| !s.trim().is_empty()) {
                self.warnings.push(
                    "mysql_connection_string is set, but SARE does not support MySQL yet; using the built-in SQLite database.".into(),
                );
            }
            let path = self.path(file);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Cannot create {}", parent.display()))?;
            }
            let conn = Connection::open(&path)
                .with_context(|| format!("Cannot open the database {}", path.display()))?;
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 2000;
                 CREATE TABLE IF NOT EXISTS sare_migrations (
                     resource TEXT NOT NULL,
                     name TEXT NOT NULL,
                     applied_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     PRIMARY KEY (resource, name)
                 );",
            )
            .context("Cannot set up the database")?;
            self.conn = Some(conn);
        }
        Ok(self.conn.as_ref().unwrap())
    }

    pub fn path(&self, file: Option<&str>) -> PathBuf {
        let file = file
            .filter(|f| !f.trim().is_empty())
            .unwrap_or("db/sare.db");
        let file = Path::new(file);
        if file.is_absolute() {
            file.to_path_buf()
        } else {
            self.root.join(file)
        }
    }

    /// Runs one query; `params` is a list (`?`) or a table (`@name`, `:name`).
    pub fn execute(conn: &Connection, mode: Mode, sql: &str, params: &Json) -> Result<Json> {
        if mode == Mode::Prepare {
            if let Json::Array(lists) = params {
                if lists.first().is_some_and(|p| p.is_array() || p.is_object()) {
                    let results = lists
                        .iter()
                        .map(|p| run(conn, sql, p).map(prepared))
                        .collect::<Result<Vec<_>>>()?;
                    return Ok(Json::Array(results));
                }
            }
        }
        let outcome = run(conn, sql, params)?;
        Ok(match (mode, outcome) {
            (Mode::Query, Outcome::Rows(rows)) => {
                Json::Array(rows.into_iter().map(object).collect())
            }
            (Mode::Single, Outcome::Rows(rows)) => {
                rows.into_iter().next().map(object).unwrap_or(Json::Null)
            }
            (Mode::Scalar, Outcome::Rows(rows)) => rows
                .into_iter()
                .next()
                .and_then(|row| row.into_iter().next())
                .map(|(_, value)| value)
                .unwrap_or(Json::Null),
            (Mode::Insert, _) => json!(conn.last_insert_rowid()),
            (Mode::Update, Outcome::Changes { affected, .. }) => json!(affected),
            (Mode::Update, Outcome::Rows(_)) => json!(conn.changes()),
            (Mode::Prepare, outcome) => prepared(outcome),
            (
                _,
                Outcome::Changes {
                    affected,
                    insert_id,
                },
            ) => changes(affected, insert_id),
        })
    }

    /// Runs all queries or none.
    pub fn transaction(conn: &Connection, queries: &[(String, Json)]) -> Result<()> {
        conn.execute_batch("BEGIN")?;
        for (sql, params) in queries {
            if let Err(error) = run(conn, sql, params) {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        conn.execute_batch("COMMIT")?;
        Ok(())
    }

    /// Applies the `.sql` files in a resource's `migrations/` folder that this
    /// database has not seen, in file name order, each in its own transaction.
    /// Returns the names it applied.
    pub fn migrate(conn: &Connection, resource: &str, folder: &Path) -> Result<Vec<String>> {
        let dir = folder.join("migrations");
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .with_context(|| format!("Cannot read {}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sql")))
            .collect();
        files.sort();
        let mut applied = Vec::new();
        for file in files {
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            let seen: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sare_migrations WHERE resource = ?1 AND name = ?2)",
                (resource, &name),
                |row| row.get(0),
            )?;
            if seen {
                continue;
            }
            let sql = std::fs::read_to_string(&file)
                .with_context(|| format!("Cannot read {}", file.display()))?;
            conn.execute_batch("BEGIN")?;
            let result = conn.execute_batch(&sql).and_then(|_| {
                conn.execute(
                    "INSERT INTO sare_migrations (resource, name) VALUES (?1, ?2)",
                    (resource, &name),
                )
            });
            if let Err(error) = result {
                let _ = conn.execute_batch("ROLLBACK");
                bail!("migration {name} failed: {error}");
            }
            conn.execute_batch("COMMIT")?;
            applied.push(name);
        }
        Ok(applied)
    }
}

fn run(conn: &Connection, sql: &str, params: &Json) -> Result<Outcome> {
    let mut stmt = conn.prepare_cached(sql)?;
    bind(&mut stmt, params)?;
    let columns = stmt.column_count();
    if columns == 0 {
        stmt.raw_execute()?;
        return Ok(Outcome::Changes {
            affected: conn.changes(),
            insert_id: conn.last_insert_rowid(),
        });
    }
    let names: Vec<String> = stmt.column_names().map(str::to_string).collect();
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut values = Vec::with_capacity(columns);
        for (i, name) in names.iter().enumerate() {
            values.push((name.clone(), from_sql(row.get::<_, Sql>(i)?)));
        }
        out.push(values);
    }
    Ok(Outcome::Rows(out))
}

/// Binds a list to `?`/`?NNN` and a table to `@name`/`:name`/`$name` (the
/// key may be written with or without its prefix). Missing values are NULL,
/// as Lua lists stop at the first nil.
fn bind(stmt: &mut Statement, params: &Json) -> Result<()> {
    let mut next = 0;
    for index in 1..=stmt.parameter_count() {
        let name = stmt.parameter_name(index).map(str::to_string);
        let value = match (params, name.as_deref()) {
            (Json::Array(list), None) => {
                next += 1;
                list.get(next - 1)
            }
            (Json::Array(list), Some(n)) if n.starts_with('?') => {
                let at = n[1..].parse::<usize>().unwrap_or(next + 1);
                next = at;
                list.get(at.wrapping_sub(1))
            }
            (Json::Object(map), Some(n)) => {
                let bare = &n[1..];
                [
                    n.to_string(),
                    bare.to_string(),
                    format!("@{bare}"),
                    format!(":{bare}"),
                ]
                .iter()
                .find_map(|key| map.get(key))
            }
            (Json::Object(map), None) => {
                next += 1;
                map.get(&next.to_string())
            }
            _ => None,
        };
        stmt.raw_bind_parameter(index, to_sql(value.unwrap_or(&Json::Null)))?;
    }
    Ok(())
}

fn to_sql(value: &Json) -> Sql {
    match value {
        Json::Null => Sql::Null,
        Json::Bool(b) => Sql::Integer(*b as i64),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Sql::Integer(i),
            None => Sql::Real(n.as_f64().unwrap_or(0.0)),
        },
        Json::String(s) => Sql::Text(s.clone()),
        // Tables are stored as JSON text, for columns such as `money` or `metadata`.
        other => Sql::Text(other.to_string()),
    }
}

fn from_sql(value: Sql) -> Json {
    match value {
        Sql::Null => Json::Null,
        Sql::Integer(i) => json!(i),
        Sql::Real(f) => serde_json::Number::from_f64(f).map_or(Json::Null, Json::Number),
        Sql::Text(s) => Json::String(s),
        Sql::Blob(b) => Json::String(String::from_utf8_lossy(&b).into_owned()),
    }
}

/// A row as a table; NULL columns are left out, so they read as nil in Lua.
fn object(row: Vec<(String, Json)>) -> Json {
    Json::Object(
        row.into_iter()
            .filter(|(_, v)| !v.is_null())
            .collect::<Map<_, _>>(),
    )
}

fn changes(affected: u64, insert_id: i64) -> Json {
    json!({ "affectedRows": affected, "insertId": insert_id, "changedRows": affected })
}

fn prepared(outcome: Outcome) -> Json {
    match outcome {
        Outcome::Rows(mut rows) if rows.len() == 1 => {
            let row = rows.remove(0);
            if row.len() == 1 {
                row.into_iter().next().unwrap().1
            } else {
                object(row)
            }
        }
        Outcome::Rows(rows) if rows.is_empty() => Json::Null,
        Outcome::Rows(rows) => Json::Array(rows.into_iter().map(object).collect()),
        Outcome::Changes {
            affected,
            insert_id,
        } => changes(affected, insert_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE players (id INTEGER PRIMARY KEY AUTOINCREMENT, license TEXT UNIQUE, cash INTEGER, job TEXT);
             CREATE TABLE sare_migrations (resource TEXT, name TEXT, applied_at INTEGER DEFAULT 0, PRIMARY KEY (resource, name));",
        )
        .unwrap();
        conn
    }

    #[test]
    fn queries_return_oxmysql_shapes() {
        let conn = db();
        let insert = "INSERT INTO players (license, cash) VALUES (?, ?)";
        assert_eq!(
            Db::execute(&conn, Mode::Insert, insert, &json!(["a", 10])).unwrap(),
            json!(1)
        );
        let named = "INSERT INTO players (license, cash) VALUES (@license, :cash)";
        let result = Db::execute(
            &conn,
            Mode::Query,
            named,
            &json!({"license": "b", "@cash": 20}),
        )
        .unwrap();
        assert_eq!(
            result,
            json!({"affectedRows": 1, "insertId": 2, "changedRows": 1})
        );

        let rows = Db::execute(
            &conn,
            Mode::Query,
            "SELECT * FROM players ORDER BY id",
            &Json::Null,
        )
        .unwrap();
        // NULL `job` is left out of each row.
        assert_eq!(
            rows,
            json!([{"id": 1, "license": "a", "cash": 10}, {"id": 2, "license": "b", "cash": 20}])
        );
        let single = "SELECT cash FROM players WHERE license = ?";
        assert_eq!(
            Db::execute(&conn, Mode::Single, single, &json!(["b"])).unwrap(),
            json!({"cash": 20})
        );
        assert_eq!(
            Db::execute(&conn, Mode::Scalar, single, &json!(["a"])).unwrap(),
            json!(10)
        );
        assert_eq!(
            Db::execute(&conn, Mode::Scalar, single, &json!(["zz"])).unwrap(),
            Json::Null
        );
        let update = "UPDATE players SET cash = cash + ?";
        assert_eq!(
            Db::execute(&conn, Mode::Update, update, &json!([5])).unwrap(),
            json!(2)
        );

        assert_eq!(
            Db::execute(&conn, Mode::Prepare, single, &json!(["a"])).unwrap(),
            json!(15)
        );
        assert_eq!(
            Db::execute(&conn, Mode::Prepare, single, &json!([["a"], ["b"]])).unwrap(),
            json!([15, 25])
        );
        // Tables are stored as JSON text.
        let job = "UPDATE players SET job = ? WHERE id = 1";
        Db::execute(&conn, Mode::Update, job, &json!([{"name": "police"}])).unwrap();
        let stored = Db::execute(
            &conn,
            Mode::Scalar,
            "SELECT job FROM players WHERE id = 1",
            &Json::Null,
        );
        assert_eq!(stored.unwrap(), json!(r#"{"name":"police"}"#));

        assert!(Db::execute(&conn, Mode::Query, "SELEC nope", &Json::Null).is_err());
    }

    #[test]
    fn transactions_are_all_or_nothing() {
        let conn = db();
        let ok = [
            (
                "INSERT INTO players (license) VALUES (?)".to_string(),
                json!(["a"]),
            ),
            ("UPDATE players SET cash = 1".to_string(), Json::Null),
        ];
        Db::transaction(&conn, &ok).unwrap();
        let bad = [
            ("UPDATE players SET cash = 2".to_string(), Json::Null),
            (
                "INSERT INTO players (license) VALUES (?)".to_string(),
                json!(["a"]),
            ),
        ];
        assert!(Db::transaction(&conn, &bad).is_err());
        let cash = Db::execute(&conn, Mode::Scalar, "SELECT cash FROM players", &Json::Null);
        assert_eq!(cash.unwrap(), json!(1));
    }

    #[test]
    fn migrations_run_once_in_name_order() {
        let conn = db();
        let folder = std::env::temp_dir().join(format!("sare-db-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("migrations")).unwrap();
        std::fs::write(
            folder.join("migrations/002_cars.sql"),
            "ALTER TABLE cars ADD plate TEXT;",
        )
        .unwrap();
        std::fs::write(
            folder.join("migrations/001_cars.sql"),
            "CREATE TABLE cars (id INTEGER);",
        )
        .unwrap();
        std::fs::write(folder.join("migrations/notes.txt"), "not sql").unwrap();
        assert_eq!(
            Db::migrate(&conn, "res", &folder).unwrap(),
            ["001_cars.sql", "002_cars.sql"]
        );
        assert!(Db::migrate(&conn, "res", &folder).unwrap().is_empty());
        // A failing migration is rolled back and stays pending.
        std::fs::write(
            folder.join("migrations/003_bad.sql"),
            "CREATE TABLE x (a); SELEC;",
        )
        .unwrap();
        assert!(Db::migrate(&conn, "res", &folder).is_err());
        let x = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'x'",
            [],
            |r| r.get::<_, i64>(0),
        );
        assert_eq!(x.unwrap(), 0);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
