//! Bounded JSON writer and a parser that only accepts one JSON value.
//! Callers impose a schema on top. Guest bytes never become a path or a shell.

use crate::budget::is_code;

#[derive(Debug)]
pub struct Buf {
    raw: Vec<u8>,
    limit: usize,
    failed: bool,
}

impl Buf {
    pub fn new(limit: usize) -> Self {
        Self {
            raw: Vec::new(),
            limit,
            failed: false,
        }
    }

    pub fn push_raw(&mut self, bytes: &[u8]) {
        if self.failed {
            return;
        }
        if self.raw.len().saturating_add(bytes.len()) > self.limit {
            self.failed = true;
            return;
        }
        self.raw.extend_from_slice(bytes);
    }

    pub fn comma(&mut self) {
        self.push_raw(b",");
    }

    pub fn key(&mut self, k: &str) {
        self.string(k);
        self.push_raw(b":");
    }

    pub fn string(&mut self, s: &str) {
        if self.failed {
            return;
        }
        self.push_raw(b"\"");
        for ch in s.chars() {
            match ch {
                '"' => self.push_raw(b"\\\""),
                '\\' => self.push_raw(b"\\\\"),
                '\n' => self.push_raw(b"\\n"),
                '\r' => self.push_raw(b"\\r"),
                '\t' => self.push_raw(b"\\t"),
                c if (c as u32) < 0x20 => {
                    let esc = format!("\\u{:04x}", c as u32);
                    self.push_raw(esc.as_bytes());
                }
                c => {
                    let mut tmp = [0u8; 4];
                    self.push_raw(c.encode_utf8(&mut tmp).as_bytes());
                }
            }
        }
        self.push_raw(b"\"");
    }

    pub fn code(&mut self, s: &str) {
        if !is_code(s) {
            self.failed = true;
            return;
        }
        self.string(s);
    }

    pub fn u64(&mut self, n: u64) {
        self.push_raw(n.to_string().as_bytes());
    }

    pub fn bool(&mut self, v: bool) {
        self.push_raw(if v { b"true" } else { b"false" });
    }

    pub fn codes(&mut self, items: &[&str]) {
        self.push_raw(b"[");
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.comma();
            }
            self.code(item);
        }
        self.push_raw(b"]");
    }

    pub fn finish(self) -> Result<Vec<u8>, &'static str> {
        if self.failed {
            Err("json-limit")
        } else {
            Ok(self.raw)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Int(u64),
    Str(String),
    Arr(Vec<Val>),
    Obj(Vec<(String, Val)>),
}

impl Val {
    pub fn get<'a>(&'a self, key: &str) -> Option<&'a Val> {
        match self {
            Val::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<u64> {
        match self {
            Val::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn bool(&self) -> Option<bool> {
        match self {
            Val::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

struct Parse<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
    max_depth: usize,
    max_array: usize,
    max_string: usize,
    nodes: usize,
    max_nodes: usize,
}

impl<'a> Parse<'a> {
    fn bump_node(&mut self) -> Result<(), &'static str> {
        self.nodes += 1;
        if self.nodes > self.max_nodes {
            Err("json-nodes")
        } else {
            Ok(())
        }
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.s.get(self.i) {
            if b.is_ascii_whitespace() {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, b: u8) -> Result<(), &'static str> {
        self.skip_ws();
        if self.s.get(self.i) == Some(&b) {
            self.i += 1;
            Ok(())
        } else {
            Err("json-syntax")
        }
    }

    fn parse_value(&mut self) -> Result<Val, &'static str> {
        self.bump_node()?;
        self.skip_ws();
        let b = *self.s.get(self.i).ok_or("json-syntax")?;
        match b {
            b'n' => self.lit(b"null", Val::Null),
            b't' => self.lit(b"true", Val::Bool(true)),
            b'f' => self.lit(b"false", Val::Bool(false)),
            b'"' => Ok(Val::Str(self.string()?)),
            b'[' => self.array(),
            b'{' => self.object(),
            b'0'..=b'9' => Ok(Val::Int(self.number()?)),
            _ => Err("json-syntax"),
        }
    }

    fn lit(&mut self, word: &[u8], v: Val) -> Result<Val, &'static str> {
        if self.s.get(self.i..self.i + word.len()) == Some(word) {
            self.i += word.len();
            Ok(v)
        } else {
            Err("json-syntax")
        }
    }

    fn number(&mut self) -> Result<u64, &'static str> {
        let start = self.i;
        while matches!(self.s.get(self.i), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        if self.i == start {
            return Err("json-syntax");
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| "json-syntax")?;
        if text.len() > 1 && text.starts_with('0') {
            return Err("json-syntax");
        }
        if matches!(self.s.get(self.i), Some(b'.' | b'e' | b'E' | b'+' | b'-')) {
            return Err("json-syntax");
        }
        text.parse::<u64>().map_err(|_| "json-overflow")
    }

    fn string(&mut self) -> Result<String, &'static str> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            if out.len() > self.max_string {
                return Err("json-string");
            }
            let b = *self.s.get(self.i).ok_or("json-syntax")?;
            self.i += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = *self.s.get(self.i).ok_or("json-syntax")?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            if self.i + 4 > self.s.len() {
                                return Err("json-syntax");
                            }
                            let hex = std::str::from_utf8(&self.s[self.i..self.i + 4])
                                .map_err(|_| "json-syntax")?;
                            self.i += 4;
                            let cp = u32::from_str_radix(hex, 16).map_err(|_| "json-syntax")?;
                            let ch = char::from_u32(cp).ok_or("json-syntax")?;
                            if (0xD800..=0xDFFF).contains(&cp) {
                                return Err("json-syntax");
                            }
                            out.push(ch);
                        }
                        _ => return Err("json-syntax"),
                    }
                }
                0x00..=0x1F => return Err("json-syntax"),
                c => {
                    // Continue a UTF-8 sequence from this lead byte.
                    self.i -= 1;
                    let rest = &self.s[self.i..];
                    let width = utf8_width(c).ok_or("json-syntax")?;
                    if rest.len() < width {
                        return Err("json-syntax");
                    }
                    let text = std::str::from_utf8(&rest[..width]).map_err(|_| "json-syntax")?;
                    out.push_str(text);
                    self.i += width;
                }
            }
        }
    }

    fn array(&mut self) -> Result<Val, &'static str> {
        self.eat(b'[')?;
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err("json-depth");
        }
        let mut items = Vec::new();
        self.skip_ws();
        if self.s.get(self.i) == Some(&b']') {
            self.i += 1;
            self.depth -= 1;
            return Ok(Val::Arr(items));
        }
        loop {
            if items.len() >= self.max_array {
                return Err("json-array");
            }
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.s.get(self.i) {
                Some(&b',') => {
                    self.i += 1;
                }
                Some(&b']') => {
                    self.i += 1;
                    self.depth -= 1;
                    return Ok(Val::Arr(items));
                }
                _ => return Err("json-syntax"),
            }
        }
    }

    fn object(&mut self) -> Result<Val, &'static str> {
        self.eat(b'{')?;
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err("json-depth");
        }
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.s.get(self.i) == Some(&b'}') {
            self.i += 1;
            self.depth -= 1;
            return Ok(Val::Obj(pairs));
        }
        loop {
            if pairs.len() >= self.max_array {
                return Err("json-array");
            }
            self.skip_ws();
            let key = self.string()?;
            if pairs.iter().any(|(k, _)| k == &key) {
                return Err("json-dup");
            }
            self.eat(b':')?;
            let val = self.parse_value()?;
            pairs.push((key, val));
            self.skip_ws();
            match self.s.get(self.i) {
                Some(&b',') => self.i += 1,
                Some(&b'}') => {
                    self.i += 1;
                    self.depth -= 1;
                    return Ok(Val::Obj(pairs));
                }
                _ => return Err("json-syntax"),
            }
        }
    }
}

fn utf8_width(lead: u8) -> Option<usize> {
    if lead < 0x80 {
        Some(1)
    } else if lead & 0xE0 == 0xC0 {
        Some(2)
    } else if lead & 0xF0 == 0xE0 {
        Some(3)
    } else if lead & 0xF8 == 0xF0 {
        Some(4)
    } else {
        None
    }
}

pub struct Limits {
    pub max_depth: usize,
    pub max_array: usize,
    pub max_string: usize,
    pub max_nodes: usize,
    pub max_bytes: usize,
}

impl Limits {
    pub fn result_slot() -> Self {
        Self {
            max_depth: 4,
            max_array: 16,
            max_string: 128,
            max_nodes: 64,
            max_bytes: 4096,
        }
    }

    pub fn tart_list() -> Self {
        Self {
            max_depth: 4,
            max_array: 256,
            max_string: 1024,
            max_nodes: 4096,
            max_bytes: 1024 * 1024,
        }
    }

    pub fn ledger() -> Self {
        Self {
            max_depth: 3,
            max_array: 8,
            max_string: 128,
            max_nodes: 32,
            max_bytes: 4096,
        }
    }
}

pub fn parse(input: &str, lim: &Limits) -> Result<Val, &'static str> {
    if input.len() > lim.max_bytes {
        return Err("json-limit");
    }
    let mut p = Parse {
        s: input.as_bytes(),
        i: 0,
        depth: 0,
        max_depth: lim.max_depth,
        max_array: lim.max_array,
        max_string: lim.max_string,
        nodes: 0,
        max_nodes: lim.max_nodes,
    };
    let v = p.parse_value()?;
    p.skip_ws();
    if p.i != p.s.len() {
        return Err("json-trailing");
    }
    Ok(v)
}

pub fn require_keys(obj: &Val, allowed: &[&str]) -> Result<(), &'static str> {
    let Val::Obj(pairs) = obj else {
        return Err("json-type");
    };
    for (k, _) in pairs {
        if !allowed.contains(&k.as_str()) {
            return Err("json-unknown-field");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_rejects_over_limit() {
        let mut b = Buf::new(64);
        b.string("a\"b\\c\n\u{0001}");
        let out = b.finish().expect("fit");
        let text = std::str::from_utf8(&out).unwrap();
        assert_eq!(text, "\"a\\\"b\\\\c\\n\\u0001\"");
        let parsed = parse(text, &Limits::result_slot()).unwrap();
        assert_eq!(parsed.str(), Some("a\"b\\c\n\u{0001}"));

        let mut big = Buf::new(16);
        big.string("0123456789abcdef");
        assert_eq!(big.finish().unwrap_err(), "json-limit");
    }

    #[test]
    fn rejects_dup_depth_trailing_and_bad_codes() {
        let lim = Limits::result_slot();
        assert_eq!(parse(r#"{"a":1,"a":2}"#, &lim).unwrap_err(), "json-dup");
        assert_eq!(
            parse(r#"{"a":{"b":{"c":{"d":{"e":1}}}}}"#, &lim).unwrap_err(),
            "json-depth"
        );
        assert_eq!(
            parse(r#"{"a":1} trailing"#, &lim).unwrap_err(),
            "json-trailing"
        );
        assert_eq!(parse(r#"{"a":-1}"#, &lim).unwrap_err(), "json-syntax");
        assert_eq!(parse(r#"{"a":01}"#, &lim).unwrap_err(), "json-syntax");
        let mut b = Buf::new(64);
        b.code("../x");
        assert_eq!(b.finish().unwrap_err(), "json-limit");
    }
}
