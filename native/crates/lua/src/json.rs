//! Lua values to JSON the way FiveM's `json.encode` and msgpack treat them:
//! sequences become arrays, other tables objects with integer keys as
//! strings (`{ [0] = 'a' }` becomes `{"0":"a"}`), functions are left out.
use mlua::{Table, Value};
use serde_json::{Map, Value as Json};

const MAX_DEPTH: usize = 64;

pub fn to_json(value: &Value) -> mlua::Result<Json> {
    convert(value, 0).map(|v| v.unwrap_or(Json::Null))
}

/// `None` for values JSON cannot hold (functions, userdata, threads).
fn convert(value: &Value, depth: usize) -> mlua::Result<Option<Json>> {
    Ok(Some(match value {
        Value::Nil => Json::Null,
        Value::Boolean(b) => Json::Bool(*b),
        Value::Integer(i) => Json::from(*i),
        Value::Number(n) => serde_json::Number::from_f64(*n).map_or(Json::Null, Json::Number),
        Value::String(s) => Json::String(s.to_string_lossy()),
        Value::Table(table) => table_to_json(table, depth + 1)?,
        _ => return Ok(None),
    }))
}

fn table_to_json(table: &Table, depth: usize) -> mlua::Result<Json> {
    if depth > MAX_DEPTH {
        return Err(mlua::Error::runtime(
            "table is nested too deeply (or refers to itself)",
        ));
    }
    let length = table.raw_len();
    let mut count = 0;
    let mut sequence = true;
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, _) = pair?;
        count += 1;
        sequence &= matches!(key, Value::Integer(i) if i >= 1 && i as usize <= length);
    }
    if sequence && count > 0 && count == length {
        let mut list = Vec::with_capacity(length);
        for i in 1..=length {
            list.push(convert(&table.raw_get::<Value>(i)?, depth)?.unwrap_or(Json::Null));
        }
        return Ok(Json::Array(list));
    }
    let mut object = Map::new();
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, value) = pair?;
        let key = match key {
            Value::String(s) => s.to_string_lossy(),
            Value::Integer(i) => i.to_string(),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => continue,
        };
        if let Some(value) = convert(&value, depth)? {
            object.insert(key, value);
        }
    }
    Ok(Json::Object(object))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;
    use serde_json::json;

    #[test]
    fn sequences_sparse_tables_and_functions() {
        let lua = Lua::new();
        let value: Value = lua
            .load(
                "local t = { list = { 1, 2.5, 'x' }, grades = { [0] = 'a', [1] = 'b' }, \
                 empty = {}, fn = print, nested = { { n = 1 } } } return t",
            )
            .eval()
            .unwrap();
        assert_eq!(
            to_json(&value).unwrap(),
            json!({ "list": [1, 2.5, "x"], "grades": { "0": "a", "1": "b" }, "empty": {}, "nested": [{ "n": 1 }] })
        );
        let cyclic: Value = lua.load("local t = {} t.self = t return t").eval().unwrap();
        assert!(to_json(&cyclic).is_err());
    }
}
