//! CfxLua syntax that FiveM resources use, rewritten to plain Lua 5.4 before
//! loading: `` `name` `` hashes, compound assignment (`x += 1`) and safe
//! navigation (`a?.b`, `a?[k]`). Line numbers are preserved, so errors point
//! at the original source. Compound assignment evaluates its target twice,
//! so a target such as `t[f()]` calls `f` twice.

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Name,
    Number,
    Str,
    Hash,
    Op,
}
#[derive(Clone, Copy, Debug)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

const KEYWORDS: [&str; 22] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];
const COMPOUND: [&str; 9] = ["+=", "-=", "*=", "/=", "<<=", ">>=", "&=", "|=", "^="];
// Longest first.
const OPS: [&str; 30] = [
    "...", "<<=", ">>=", "..", "==", "~=", "<=", ">=", "<<", ">>", "//", "::", "+=", "-=", "*=",
    "/=", "&=", "|=", "^=", "?.", "?[", "+", "-", "*", "/", "%", "^", "#", "&", "~",
];
const BINARY: [&str; 21] = [
    "+", "-", "*", "/", "//", "%", "^", "..", "==", "~=", "<", "<=", ">", ">=", "and", "or", "&",
    "|", "~", "<<", ">>",
];

/// Jenkins one-at-a-time hash of the lower-cased name, as FiveM's GetHashKey,
/// returned as a signed 32-bit integer like CfxLua's backtick literals.
pub fn joaat(name: &str) -> i32 {
    let mut hash: u32 = 0;
    for byte in name.bytes().map(|b| b.to_ascii_lowercase()) {
        hash = hash.wrapping_add(byte as u32);
        hash = hash.wrapping_add(hash << 10);
        hash ^= hash >> 6;
    }
    hash = hash.wrapping_add(hash << 3);
    hash ^= hash >> 11;
    hash = hash.wrapping_add(hash << 15);
    hash as i32
}

fn long_bracket(src: &[u8], at: usize) -> Option<usize> {
    // `[[` or `[==[`: returns the level.
    let mut i = at + 1;
    while src.get(i) == Some(&b'=') {
        i += 1;
    }
    (src.get(i) == Some(&b'[')).then_some(i - at - 1)
}
fn long_end(src: &[u8], from: usize, level: usize) -> Result<usize, String> {
    let close = format!("]{}]", "=".repeat(level));
    let text = &src[from..];
    text.windows(close.len())
        .position(|w| w == close.as_bytes())
        .map(|p| from + p + close.len())
        .ok_or_else(|| "unfinished long string or comment".to_string())
}

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let src = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        let start = i;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'-' && src.get(i + 1) == Some(&b'-') {
            i += 2;
            if src.get(i) == Some(&b'[') {
                if let Some(level) = long_bracket(src, i) {
                    i = long_end(src, i + level + 2, level)?;
                    continue;
                }
            }
            while i < src.len() && src[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let kind = if c.is_ascii_alphabetic() || c == b'_' {
            while i < src.len() && (src[i].is_ascii_alphanumeric() || src[i] == b'_') {
                i += 1;
            }
            Kind::Name
        } else if c.is_ascii_digit()
            || (c == b'.' && src.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            while i < src.len() {
                let d = src[i];
                let exponent = matches!(d, b'e' | b'E' | b'p' | b'P');
                if exponent && matches!(src.get(i + 1), Some(b'+' | b'-')) {
                    i += 2;
                } else if d.is_ascii_alphanumeric() || d == b'.' || d == b'_' {
                    i += 1;
                } else {
                    break;
                }
            }
            Kind::Number
        } else if c == b'"' || c == b'\'' || c == b'`' {
            i += 1;
            while i < src.len() && src[i] != c {
                if src[i] == b'\\' {
                    i += 1;
                } else if src[i] == b'\n' {
                    return Err("unfinished string".into());
                }
                i += 1;
            }
            if i >= src.len() {
                return Err("unfinished string".into());
            }
            i += 1;
            if c == b'`' {
                Kind::Hash
            } else {
                Kind::Str
            }
        } else if c == b'[' && long_bracket(src, i).is_some() {
            let level = long_bracket(src, i).unwrap();
            i = long_end(src, i + level + 2, level)?;
            Kind::Str
        } else {
            let rest = &source[i..];
            let op = OPS.iter().find(|op| rest.starts_with(**op));
            i += op.map_or_else(
                || rest.chars().next().map_or(1, char::len_utf8),
                |op| op.len(),
            );
            Kind::Op
        };
        tokens.push(Token {
            kind,
            start,
            end: i,
        });
    }
    Ok(tokens)
}

struct Pass<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    /// Matching bracket index for `( [ { ?[` and their closers.
    pair: Vec<Option<usize>>,
}
impl<'a> Pass<'a> {
    fn new(source: &'a str) -> Result<Self, String> {
        let tokens = tokenize(source)?;
        let mut pair = vec![None; tokens.len()];
        let mut stack = Vec::new();
        for (i, t) in tokens.iter().enumerate() {
            if t.kind != Kind::Op {
                continue;
            }
            match &source[t.start..t.end] {
                "(" | "[" | "{" | "?[" => stack.push(i),
                ")" | "]" | "}" => {
                    let open = stack.pop().ok_or("unbalanced brackets")?;
                    pair[open] = Some(i);
                    pair[i] = Some(open);
                }
                _ => {}
            }
        }
        Ok(Self {
            source,
            tokens,
            pair,
        })
    }
    fn text(&self, i: usize) -> &str {
        self.tokens
            .get(i)
            .map_or("", |t| &self.source[t.start..t.end])
    }
    fn is_name(&self, i: usize) -> bool {
        self.tokens[i].kind == Kind::Name && !KEYWORDS.contains(&self.text(i))
    }
    /// Whether token `i` can end an operand, so a following `(`/`[` is a call/index.
    fn ends_operand(&self, i: usize) -> bool {
        match self.tokens[i].kind {
            Kind::Name => {
                self.is_name(i) || matches!(self.text(i), "end" | "nil" | "true" | "false")
            }
            Kind::Op => matches!(self.text(i), ")" | "]" | "}"),
            _ => true,
        }
    }
    /// First token of the prefix expression that ends at token `end` (inclusive).
    fn prefix_start(&self, end: usize) -> Option<usize> {
        let mut j = end;
        loop {
            match self.text(j) {
                // Call arguments, or a parenthesized expression.
                ")" => {
                    let open = self.pair[j]?;
                    if open > 0 && self.ends_operand(open - 1) {
                        j = open - 1;
                    } else {
                        return Some(open);
                    }
                }
                // Indexing always follows an operand.
                "]" => j = self.pair[j]?.checked_sub(1)?,
                // `f{...}` and `f"..."` calls.
                "}" => {
                    let open = self.pair[j]?;
                    j = open.checked_sub(1).filter(|&p| self.ends_operand(p))?;
                }
                _ if self.tokens[j].kind == Kind::Str => {
                    j = j.checked_sub(1).filter(|&p| self.ends_operand(p))?;
                }
                _ if self.is_name(j) => {
                    if j >= 2 && matches!(self.text(j - 1), "." | ":" | "?.") {
                        j -= 2;
                    } else {
                        return Some(j);
                    }
                }
                _ => return None,
            }
        }
    }
    /// Token index just past the expression starting at `start`.
    fn expression_end(&self, start: usize) -> usize {
        let mut i = start;
        let mut operand = true;
        while i < self.tokens.len() {
            let text = self.text(i);
            if operand {
                match text {
                    "-" | "not" | "#" | "~" => i += 1,
                    "(" | "{" => {
                        i = self.pair[i].map_or(self.tokens.len(), |p| p + 1);
                        operand = false;
                    }
                    "function" => {
                        i = self.block_end(i);
                        operand = false;
                    }
                    _ => {
                        i += 1;
                        operand = false;
                    }
                }
            } else {
                match text {
                    _ if BINARY.contains(&text) => {
                        i += 1;
                        operand = true;
                    }
                    "." | ":" | "?." => i += 2,
                    "(" | "{" | "[" | "?[" => i = self.pair[i].map_or(self.tokens.len(), |p| p + 1),
                    _ if self.tokens[i].kind == Kind::Str => i += 1,
                    _ => return i,
                }
            }
        }
        i
    }
    /// Index just past the `end` closing the `function` at `start`.
    fn block_end(&self, start: usize) -> usize {
        let mut depth = 0;
        for i in start..self.tokens.len() {
            if self.tokens[i].kind != Kind::Name {
                continue;
            }
            match self.text(i) {
                "function" | "if" | "do" | "repeat" => depth += 1,
                "end" | "until" => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                _ => {}
            }
        }
        self.tokens.len()
    }
}

/// (position, replaced length, text); insertions at the same position are
/// kept in the order given.
type Edit = (usize, usize, String);
fn apply(source: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| e.0);
    let mut out = String::with_capacity(source.len() + edits.len() * 16);
    let mut at = 0;
    for (pos, len, text) in edits {
        out.push_str(&source[at..pos]);
        out.push_str(&text);
        at = pos + len;
    }
    out.push_str(&source[at..]);
    out
}

fn hashes(source: &str) -> Result<String, String> {
    let pass = Pass::new(source)?;
    let edits = pass
        .tokens
        .iter()
        .filter(|t| t.kind == Kind::Hash)
        .map(|t| {
            let name = &source[t.start + 1..t.end - 1];
            (t.start, t.end - t.start, joaat(name).to_string())
        })
        .collect();
    Ok(apply(source, edits))
}

/// `a?.b` -> `__cfx_safe(a, "b")`, `a?[k]` -> `__cfx_safe(a, k)`. One `?` per
/// pass, so chains nest correctly.
fn safe_navigation(source: &str) -> Result<String, String> {
    let mut source = source.to_string();
    loop {
        let pass = Pass::new(&source)?;
        let Some(i) = (0..pass.tokens.len()).find(|&i| matches!(pass.text(i), "?." | "?[")) else {
            return Ok(source);
        };
        let start = i
            .checked_sub(1)
            .and_then(|end| pass.prefix_start(end))
            .ok_or("safe navigation without a left operand")?;
        let mut edits = vec![(pass.tokens[start].start, 0, "__cfx_safe(".to_string())];
        let op = pass.tokens[i];
        if pass.text(i) == "?." {
            if i + 1 >= pass.tokens.len() || !pass.is_name(i + 1) {
                return Err("expected a name after ?.".into());
            }
            let name = pass.tokens[i + 1];
            edits.push((
                op.start,
                name.end - op.start,
                format!(", {:?})", pass.text(i + 1)),
            ));
        } else {
            let close = pass.pair[i].ok_or("unbalanced ?[")?;
            edits.push((op.start, 2, ", ".into()));
            edits.push((pass.tokens[close].start, 1, ")".into()));
        }
        source = apply(&source, edits);
    }
}

/// `target op= value` -> `target = target op (value)`.
fn compound(source: &str) -> Result<String, String> {
    let pass = Pass::new(source)?;
    let mut edits = Vec::new();
    for i in 0..pass.tokens.len() {
        let op = pass.text(i);
        if pass.tokens[i].kind != Kind::Op || !COMPOUND.contains(&op) {
            continue;
        }
        let start = i
            .checked_sub(1)
            .and_then(|end| pass.prefix_start(end))
            .ok_or("compound assignment without a target")?;
        let end = pass.expression_end(i + 1);
        if end <= i + 1 {
            return Err("compound assignment without a value".into());
        }
        let target: String = source[pass.tokens[start].start..pass.tokens[i - 1].end]
            .chars()
            .map(|c| if c == '\n' { ' ' } else { c })
            .collect();
        let binary = &op[..op.len() - 1];
        edits.push((
            pass.tokens[i].start,
            op.len(),
            format!("= {target} {binary} ("),
        ));
        edits.push((pass.tokens[end - 1].end, 0, ")".into()));
    }
    Ok(apply(source, edits))
}

/// Translate CfxLua to Lua 5.4. Plain Lua comes back unchanged.
pub fn translate(source: &str) -> Result<String, String> {
    if !source.contains('`')
        && !source.contains('?')
        && !COMPOUND.iter().any(|op| source.contains(op))
    {
        return Ok(source.to_string());
    }
    compound(&safe_navigation(&hashes(source)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> String {
        let lua = mlua::Lua::new();
        lua.load("function __cfx_safe(v, k) if v == nil then return nil end return v[k] end")
            .exec()
            .unwrap();
        let code = translate(source).unwrap();
        lua.load(&code)
            .eval()
            .unwrap_or_else(|e| panic!("{e}\n{code}"))
    }

    #[test]
    fn hashes_match_get_hash_key() {
        assert_eq!(joaat("adder"), -1216765807);
        assert_eq!(joaat("ADDER"), joaat("adder"));
        assert_eq!(translate("return `adder`").unwrap(), "return -1216765807");
    }

    #[test]
    fn compound_assignment_and_safe_navigation() {
        let value = run(r#"
local t = { n = 1, list = { 2 }, deep = { a = { b = 5 } } }
local count = 0
t.n += 2 * 3 count += 1
t.list[1] -= 1
for i = 1, 3 do count += i end
local f = function(x) x *= 2 return x end
count += f(2)
local missing = nil
local a = missing?.field
local b = t?.deep?.a?.b
local c = t.deep?['a']?.b
local d = getmetatable('')?.__index ~= nil
local s = "x += 1 and a?.b"
return ('%d %d %d %s %d %d %s %s'):format(t.n, t.list[1], count, tostring(a), b, c, tostring(d), s)
"#);
        assert_eq!(value, "7 1 11 nil 5 5 true x += 1 and a?.b");
    }

    #[test]
    fn line_numbers_are_preserved() {
        let source = "local x = 1\nx +=\n  2\nlocal y = {}\nlocal z = y?.a\nerror('here')";
        let code = translate(source).unwrap();
        assert_eq!(code.lines().count(), source.lines().count());
        let lua = mlua::Lua::new();
        lua.load("function __cfx_safe(v, k) if v == nil then return nil end return v[k] end")
            .exec()
            .unwrap();
        let error = lua
            .load(&code)
            .set_name("=t")
            .exec()
            .unwrap_err()
            .to_string();
        assert!(error.contains("t:6:"), "{error}");
    }

    #[test]
    fn plain_lua_is_unchanged() {
        let source = "local a = b ~= c and d <= e\nprint('?.')\n-- x += 1\n--[[ a?.b ]]";
        assert_eq!(translate(source).unwrap(), source);
    }
}
