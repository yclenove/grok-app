//! Small YAML subset parser for evaluation tasks.

use crate::Error;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Yaml {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Seq(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
}

pub(crate) fn parse_yaml(text: &str) -> Result<Yaml, String> {
    let mut items = Vec::new();
    for (number, raw) in text.lines().enumerate() {
        if raw.contains('\t') {
            return Err(format!("line {}: tabs are not allowed", number + 1));
        }
        let stripped = strip_comment(raw).trim_end();
        if stripped.trim().is_empty() {
            continue;
        }
        let indent = stripped.chars().take_while(|ch| *ch == ' ').count();
        let content = stripped[indent..].trim();
        items.push((indent, content.to_string()));
    }
    let mut cursor = 0;
    let value = parse_node(&items, &mut cursor, 0)?;
    if cursor != items.len() {
        return Err("trailing yaml".into());
    }
    Ok(value)
}

fn strip_comment(line: &str) -> &str {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if double {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                double = false;
            }
            continue;
        }
        if single {
            if ch == '\'' {
                single = false;
            }
            continue;
        }
        match ch {
            '"' => double = true,
            '\'' => single = true,
            '#' => return &line[..index],
            _ => {}
        }
    }
    line
}

fn parse_node(items: &[(usize, String)], cursor: &mut usize, min: usize) -> Result<Yaml, String> {
    if *cursor >= items.len() || items[*cursor].0 < min {
        return Ok(Yaml::Null);
    }
    let (indent, text) = &items[*cursor];
    if text.starts_with("- ") || text == "-" {
        parse_seq(items, cursor, *indent)
    } else {
        parse_map(items, cursor, *indent)
    }
}

fn parse_map(items: &[(usize, String)], cursor: &mut usize, indent: usize) -> Result<Yaml, String> {
    let mut map = Vec::new();
    while *cursor < items.len() {
        let (this, text) = &items[*cursor];
        if *this != indent || text.starts_with("- ") || text == "-" {
            break;
        }
        let (key, rest) = split_key(text)?;
        *cursor += 1;
        let value = if rest.is_empty() {
            if *cursor < items.len() && items[*cursor].0 > indent {
                parse_node(items, cursor, indent + 1)?
            } else {
                Yaml::Null
            }
        } else {
            finish_flow(&rest)?
        };
        map.push((key, value));
    }
    if map.is_empty() {
        return Err("expected a map".into());
    }
    Ok(Yaml::Map(map))
}

fn parse_seq(items: &[(usize, String)], cursor: &mut usize, indent: usize) -> Result<Yaml, String> {
    let mut seq = Vec::new();
    while *cursor < items.len() {
        let (this, text) = &items[*cursor];
        if *this != indent || !(text.starts_with("- ") || text == "-") {
            break;
        }
        let rest = if text == "-" {
            String::new()
        } else {
            text[2..].trim().to_string()
        };
        *cursor += 1;
        let value = if rest.is_empty() {
            if *cursor < items.len() && items[*cursor].0 > indent {
                parse_node(items, cursor, indent + 1)?
            } else {
                Yaml::Null
            }
        } else {
            finish_flow(&rest)?
        };
        seq.push(value);
    }
    Ok(Yaml::Seq(seq))
}

fn split_key(text: &str) -> Result<(String, String), String> {
    let mut single = false;
    let mut double = false;
    for (index, ch) in text.char_indices() {
        match ch {
            '"' if !single => double = !double,
            '\'' if !double => single = !single,
            ':' if !single && !double => {
                let key = unquote(text[..index].trim())?;
                if key.is_empty() {
                    return Err("empty key".into());
                }
                return Ok((key, text[index + ch.len_utf8()..].trim().to_string()));
            }
            _ => {}
        }
    }
    Err(format!("expected key:value in {text}"))
}

fn finish_flow(text: &str) -> Result<Yaml, String> {
    let (value, used) = parse_flow(text)?;
    if !text[used..].trim().is_empty() {
        return Err(format!("trailing flow input {}", &text[used..]));
    }
    Ok(value)
}

fn parse_flow(text: &str) -> Result<(Yaml, usize), String> {
    let trimmed = text.trim_start();
    let offset = text.len() - trimmed.len();
    if trimmed.is_empty() {
        return Ok((Yaml::Null, text.len()));
    }
    let (value, used) = match trimmed.as_bytes()[0] {
        b'{' => parse_flow_map(trimmed)?,
        b'[' => parse_flow_seq(trimmed)?,
        b'"' | b'\'' => {
            let (string, used) = parse_quoted(trimmed)?;
            (Yaml::String(string), used)
        }
        _ => parse_bare(trimmed)?,
    };
    Ok((value, offset + used))
}

fn parse_flow_map(text: &str) -> Result<(Yaml, usize), String> {
    let mut cursor = Cur { text, index: 1 };
    let mut map = Vec::new();
    loop {
        cursor.skip_ws();
        if cursor.eat('}') {
            return Ok((Yaml::Map(map), cursor.index));
        }
        let (key, key_end) = parse_flow_key(&text[cursor.index..])?;
        cursor.index += key_end;
        cursor.skip_ws();
        if !cursor.eat(':') {
            return Err("expected ':' in flow map".into());
        }
        cursor.skip_ws();
        let (value, value_end) = parse_flow(&text[cursor.index..])?;
        cursor.index += value_end;
        map.push((key, value));
        cursor.skip_ws();
        if cursor.eat(',') {
            continue;
        }
        if cursor.eat('}') {
            return Ok((Yaml::Map(map), cursor.index));
        }
        return Err("expected ',' or '}'".into());
    }
}

fn parse_flow_seq(text: &str) -> Result<(Yaml, usize), String> {
    let mut cursor = Cur { text, index: 1 };
    let mut seq = Vec::new();
    loop {
        cursor.skip_ws();
        if cursor.eat(']') {
            return Ok((Yaml::Seq(seq), cursor.index));
        }
        let (value, value_end) = parse_flow(&text[cursor.index..])?;
        cursor.index += value_end;
        seq.push(value);
        cursor.skip_ws();
        if cursor.eat(',') {
            continue;
        }
        if cursor.eat(']') {
            return Ok((Yaml::Seq(seq), cursor.index));
        }
        return Err("expected ',' or ']'".into());
    }
}

struct Cur<'a> {
    text: &'a str,
    index: usize,
}

impl Cur<'_> {
    fn rest(&self) -> &str {
        &self.text[self.index..]
    }
    fn skip_ws(&mut self) {
        while let Some(ch) = self.rest().chars().next() {
            if !ch.is_whitespace() {
                break;
            }
            self.index += ch.len_utf8();
        }
    }
    fn eat(&mut self, expected: char) -> bool {
        if self.rest().starts_with(expected) {
            self.index += expected.len_utf8();
            true
        } else {
            false
        }
    }
}

fn parse_flow_key(text: &str) -> Result<(String, usize), String> {
    let trimmed = text.trim_start();
    let offset = text.len() - trimmed.len();
    if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        let (key, used) = parse_quoted(trimmed)?;
        return Ok((key, offset + used));
    }
    let mut used = 0;
    for ch in trimmed.chars() {
        if matches!(ch, ':' | ',' | '}' | ']' | '[') {
            break;
        }
        used += ch.len_utf8();
    }
    let key = trimmed[..used].trim();
    if key.is_empty() {
        return Err("empty flow key".into());
    }
    Ok((key.to_string(), offset + used))
}

fn parse_quoted(text: &str) -> Result<(String, usize), String> {
    let quote = text.chars().next().ok_or("empty quote")?;
    let mut out = String::new();
    let mut escaped = false;
    let mut used = quote.len_utf8();
    for ch in text[quote.len_utf8()..].chars() {
        used += ch.len_utf8();
        if quote == '"' && escaped {
            escaped = false;
            match ch {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' | '"' => out.push(ch),
                other => out.push(other),
            }
            continue;
        }
        if quote == '"' && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return Ok((out, used));
        }
        out.push(ch);
    }
    Err("unterminated string".into())
}

fn parse_bare(text: &str) -> Result<(Yaml, usize), String> {
    let mut used = 0;
    for ch in text.chars() {
        if matches!(ch, ',' | '{' | '}' | '[' | ']') {
            break;
        }
        used += ch.len_utf8();
    }
    let token = text[..used].trim();
    if token.is_empty() {
        return Err("empty scalar".into());
    }
    let value = match token {
        "true" => Yaml::Bool(true),
        "false" => Yaml::Bool(false),
        "null" => Yaml::Null,
        _ => {
            if let Ok(number) = token.parse::<i64>() {
                if token.chars().all(|ch| ch.is_ascii_digit() || ch == '-') {
                    Yaml::Int(number)
                } else {
                    Yaml::String(token.to_string())
                }
            } else if token.contains('.') && token.parse::<f64>().is_ok() && !token.contains('/') {
                Yaml::Float(token.parse::<f64>().map_err(|err| err.to_string())?)
            } else {
                Yaml::String(token.to_string())
            }
        }
    };
    Ok((value, used))
}

fn unquote(text: &str) -> Result<String, String> {
    if text.starts_with('"') || text.starts_with('\'') {
        Ok(parse_quoted(text)?.0)
    } else {
        Ok(text.to_string())
    }
}

pub(crate) fn expect_map(value: &Yaml) -> Result<&[(String, Yaml)], Error> {
    match value {
        Yaml::Map(map) => Ok(map),
        _ => Err(Error::msg("expected a map")),
    }
}

pub(crate) fn expect_seq(value: &Yaml) -> Result<&[Yaml], Error> {
    match value {
        Yaml::Seq(seq) => Ok(seq),
        _ => Err(Error::msg("expected a sequence")),
    }
}

pub(crate) fn map_get<'a>(map: &'a [(String, Yaml)], key: &str) -> Option<&'a Yaml> {
    map.iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

pub(crate) fn require<'a>(map: &'a [(String, Yaml)], key: &str) -> Result<&'a Yaml, Error> {
    map_get(map, key).ok_or_else(|| Error::msg(format!("missing {key}")))
}

pub(crate) fn require_str(map: &[(String, Yaml)], key: &str) -> Result<String, Error> {
    match require(map, key)? {
        Yaml::String(text) => Ok(text.clone()),
        _ => Err(Error::msg(format!("{key} must be a string"))),
    }
}

pub(crate) fn require_u64(map: &[(String, Yaml)], key: &str) -> Result<u64, Error> {
    match require(map, key)? {
        Yaml::Int(number) => u64::try_from(*number)
            .map_err(|_| Error::msg(format!("{key} must be an unsigned integer"))),
        _ => Err(Error::msg(format!("{key} must be an unsigned integer"))),
    }
}

pub(crate) fn require_str_list(map: &[(String, Yaml)], key: &str) -> Result<Vec<String>, Error> {
    require_str_list_value(require(map, key)?)
}

pub(crate) fn require_str_list_value(value: &Yaml) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    for item in expect_seq(value)? {
        match item {
            Yaml::String(text) => out.push(text.clone()),
            _ => return Err(Error::msg("list entries must be strings")),
        }
    }
    Ok(out)
}

pub(crate) fn yaml_to_json(value: &Yaml) -> Result<Value, Error> {
    Ok(match value {
        Yaml::Null => Value::Null,
        Yaml::Bool(bit) => Value::Bool(*bit),
        Yaml::Int(number) => Value::Number((*number).into()),
        Yaml::Float(number) => serde_json::Number::from_f64(*number)
            .map(Value::Number)
            .ok_or_else(|| Error::msg("non-finite number"))?,
        Yaml::String(text) => Value::String(text.clone()),
        Yaml::Seq(_) | Yaml::Map(_) => return Err(Error::msg("equals must be a scalar")),
    })
}

pub(crate) fn yaml_to_f64(value: &Yaml) -> Result<f64, Error> {
    match value {
        Yaml::Int(number) => Ok(*number as f64),
        Yaml::Float(number) => Ok(*number),
        _ => Err(Error::msg("numeric comparison needs a number")),
    }
}
