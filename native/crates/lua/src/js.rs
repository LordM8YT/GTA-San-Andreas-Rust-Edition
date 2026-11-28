//! World scripts in JavaScript (QuickJS). A JS script gets the same natives
//! as a Lua one, kept in a Lua state that runs no Lua of its own: JS calls
//! `__native(name, ...args)`, which calls that native and hands its results
//! back as an array. So both languages share one world state, one set of
//! checks and the same requests. `world.js` builds the API on top.
use mlua::{Lua, Table};
use rquickjs::function::Rest;
use rquickjs::{Array, Context, Ctx, Function, Object, Runtime, Type, Value};

const PRELUDE: &str = include_str!("world.js");
/// Deepest nesting of values passed to a native.
const MAX_DEPTH: usize = 16;

pub struct JsScript {
    runtime: Runtime,
    context: Context,
}

/// A thrown value as a line for Output.
fn describe(ctx: &Ctx<'_>, error: rquickjs::Error) -> String {
    if !matches!(error, rquickjs::Error::Exception) {
        return error.to_string();
    }
    let caught = ctx.catch();
    if let Some(exception) = caught.as_exception() {
        let message = exception.message().unwrap_or_default();
        let line = exception.stack().and_then(|stack| {
            stack
                .lines()
                .find_map(|l| l.rsplit_once(':').map(|(head, _)| head.to_string()))
                .and_then(|head| head.rsplit(':').next().map(str::to_string))
                .filter(|n| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty())
        });
        return match line {
            Some(line) => format!("line {line}: {message}"),
            None => message,
        };
    }
    caught
        .as_string()
        .and_then(|s| s.to_string().ok())
        .unwrap_or_else(|| "the script threw something that is not an error".into())
}

fn to_lua(lua: &Lua, value: &Value<'_>, depth: usize) -> mlua::Result<mlua::Value> {
    if depth > MAX_DEPTH {
        return Err(mlua::Error::runtime("values nested too deeply"));
    }
    Ok(match value.type_of() {
        Type::Undefined | Type::Null | Type::Uninitialized => mlua::Value::Nil,
        Type::Bool => mlua::Value::Boolean(value.as_bool().unwrap_or(false)),
        Type::Int => mlua::Value::Integer(value.as_int().unwrap_or(0).into()),
        Type::Float => {
            let n = value.as_float().unwrap_or(0.0);
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                mlua::Value::Integer(n as i64)
            } else {
                mlua::Value::Number(n)
            }
        }
        Type::String => {
            let text = value
                .as_string()
                .and_then(|s| s.to_string().ok())
                .unwrap_or_default();
            mlua::Value::String(lua.create_string(&text)?)
        }
        Type::Array => {
            let table = lua.create_table()?;
            if let Some(array) = value.as_array() {
                for (i, item) in array.iter::<Value>().enumerate() {
                    let item = item.map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    table.raw_set(i + 1, to_lua(lua, &item, depth + 1)?)?;
                }
            }
            mlua::Value::Table(table)
        }
        Type::Object => {
            let table = lua.create_table()?;
            if let Some(object) = value.as_object() {
                for prop in object.props::<String, Value>() {
                    let (key, item) = prop.map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    table.raw_set(key, to_lua(lua, &item, depth + 1)?)?;
                }
            }
            mlua::Value::Table(table)
        }
        _ => {
            return Err(mlua::Error::runtime(
                "functions and other values cannot be passed",
            ))
        }
    })
}

fn to_js<'js>(ctx: &Ctx<'js>, value: mlua::Value, depth: usize) -> rquickjs::Result<Value<'js>> {
    Ok(match value {
        mlua::Value::Nil => Value::new_undefined(ctx.clone()),
        mlua::Value::Boolean(b) => Value::new_bool(ctx.clone(), b),
        mlua::Value::Integer(i) => Value::new_number(ctx.clone(), i as f64),
        mlua::Value::Number(n) => Value::new_number(ctx.clone(), n),
        mlua::Value::String(s) => {
            rquickjs::String::from_str(ctx.clone(), &s.to_string_lossy())?.into_value()
        }
        mlua::Value::Table(table) if depth < MAX_DEPTH => {
            let len = table.raw_len();
            let pairs = table.clone().pairs::<mlua::Value, mlua::Value>().count();
            if len == pairs {
                let array = Array::new(ctx.clone())?;
                for i in 1..=len {
                    let item: mlua::Value = table.raw_get(i).unwrap_or(mlua::Value::Nil);
                    array.set(i - 1, to_js(ctx, item, depth + 1)?)?;
                }
                array.into_value()
            } else {
                let object = Object::new(ctx.clone())?;
                for (key, item) in table.pairs::<String, mlua::Value>().flatten() {
                    object.set(key, to_js(ctx, item, depth + 1)?)?;
                }
                object.into_value()
            }
        }
        _ => Value::new_undefined(ctx.clone()),
    })
}

/// A native's result, made a JS value when handed back.
struct Out(mlua::Value);
impl<'js> rquickjs::IntoJs<'js> for Out {
    fn into_js(self, ctx: &Ctx<'js>) -> rquickjs::Result<Value<'js>> {
        to_js(ctx, self.0, 0)
    }
}

impl JsScript {
    /// A JS engine for one script, calling the natives in `natives` (a Lua
    /// state made for this script without the Lua prelude), with the same
    /// time and memory limits. Runs the prelude, not the body yet.
    pub(crate) fn new(
        natives: &Lua,
        memory: usize,
        over: impl FnMut() -> bool + 'static,
    ) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        runtime.set_memory_limit(memory);
        runtime.set_max_stack_size(512 * 1024);
        runtime.set_interrupt_handler(Some(Box::new(over)));
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;
        let table: Table = natives
            .globals()
            .get("__world")
            .map_err(|e| e.to_string())?;
        let lua = natives.clone();
        context.with(|ctx| -> Result<(), String> {
            let bridge = Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>,
                      name: String,
                      args: Rest<Value<'_>>|
                      -> rquickjs::Result<Vec<Out>> {
                    let throw = |text: String| rquickjs::Exception::throw_message(&ctx, &text);
                    let f: mlua::Function = match table.get(name.as_str()) {
                        Ok(f) => f,
                        Err(_) => return Err(throw(format!("no native {name}"))),
                    };
                    let mut values = Vec::with_capacity(args.0.len());
                    for arg in args.0.iter() {
                        values.push(to_lua(&lua, arg, 0).map_err(|e| throw(e.to_string()))?);
                    }
                    let results: mlua::MultiValue = f
                        .call(mlua::MultiValue::from_iter(values))
                        .map_err(|e| throw(crate::world::clean(&e)))?;
                    Ok(results.into_iter().map(Out).collect())
                },
            )
            .map_err(|e| e.to_string())?;
            ctx.globals()
                .set("__native", bridge)
                .map_err(|e| e.to_string())?;
            ctx.eval::<(), _>(PRELUDE).map_err(|e| describe(&ctx, e))
        })?;
        Ok(Self { runtime, context })
    }

    /// Run the script's body: its top level may use `await`.
    pub(crate) fn start(&self, source: &str) -> Result<(), String> {
        // On the body's first line, so errors give its own line numbers.
        let wrapped = format!("__world_start(async () => {{{source}\n}});");
        let result = self
            .context
            .with(|ctx| ctx.eval::<(), _>(wrapped).map_err(|e| describe(&ctx, e)));
        self.run_jobs();
        result
    }

    /// Call a global function of the prelude with JSON arguments.
    pub(crate) fn call(&self, name: &str, args: &serde_json::Value) -> Result<(), String> {
        let text = args.to_string();
        let result = self.context.with(|ctx| -> Result<(), String> {
            let f: Function = ctx.globals().get(name).map_err(|e| describe(&ctx, e))?;
            let parsed = ctx.json_parse(text).map_err(|e| describe(&ctx, e))?;
            let array = parsed
                .as_array()
                .ok_or_else(|| "arguments must be a list".to_string())?;
            let values: Vec<Value> = array.iter::<Value>().flatten().collect();
            f.call::<_, ()>((Rest(values),))
                .map_err(|e| describe(&ctx, e))
        });
        self.run_jobs();
        result
    }

    /// Let awaited work go on (promises that resolved).
    fn run_jobs(&self) {
        for _ in 0..100_000 {
            match self.runtime.execute_pending_job() {
                Ok(true) => continue,
                Ok(false) | Err(_) => break,
            }
        }
    }
}

/// Parse `source` without running it: None when it is valid JavaScript (a
/// body that may use await), or the error with its line.
pub fn check(source: &str) -> Option<String> {
    let runtime = Runtime::new().ok()?;
    let context = Context::full(&runtime).ok()?;
    context.with(|ctx| {
        let parse = format!(
            "(Object.getPrototypeOf(async function () {{}}).constructor)({})",
            serde_json::to_string(source).ok()?
        );
        ctx.eval::<(), _>(parse).err().map(|e| describe(&ctx, e))
    })
}
