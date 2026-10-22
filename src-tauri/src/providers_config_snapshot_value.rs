//! TOML values consumed structurally; no config or parse-value logging.
use super::{assign, extend_depth, invalid, Parser, Table, Value, MAX_CONFIG_DEPTH};

impl Parser<'_> {
    pub(super) fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth >= MAX_CONFIG_DEPTH {
            return Err(invalid());
        }
        match self.peek() {
            Some(b'\'' | b'"') => self.string(true).map(Value::String),
            Some(b'[') => {
                self.offset += 1;
                self.space_and_comments()?;
                let mut values = Vec::new();
                if self.take(b']') {
                    return Ok(Value::Array(values));
                }
                loop {
                    values.push(self.value(extend_depth(depth, 1)?)?);
                    self.space_and_comments()?;
                    if self.take(b']') {
                        return Ok(Value::Array(values));
                    }
                    self.expect(b',')?;
                    self.space_and_comments()?;
                    if self.take(b']') {
                        return Ok(Value::Array(values));
                    }
                }
            }
            Some(b'{') => {
                self.offset += 1;
                let mut table = Table::default();
                self.horizontal();
                if !self.take(b'}') {
                    loop {
                        let key = self.key_path()?;
                        self.expect(b'=')?;
                        self.horizontal();
                        // Dotted keys create table nodes inside the inline
                        // value; count them before parsing/allocating its child.
                        let value = self.value(extend_depth(depth, key.len())?)?;
                        assign(&mut table, &key, value)?;
                        self.horizontal();
                        if self.take(b'}') {
                            break;
                        }
                        self.expect(b',')?;
                        self.horizontal();
                    }
                }
                table.inline = true;
                Ok(Value::Table(table))
            }
            Some(_) => {
                let start = self.offset;
                while self
                    .peek()
                    .is_some_and(|byte| !matches!(byte, b',' | b']' | b'}' | b'\r' | b'\n' | b'#'))
                {
                    self.offset += 1;
                }
                let raw = self.text[start..self.offset].trim_end_matches([' ', '\t']);
                if valid_atom(raw) {
                    Ok(Value::Atom(raw.into()))
                } else {
                    Err(invalid())
                }
            }
            None => Err(invalid()),
        }
    }

    pub(super) fn string(&mut self, allow_multiline: bool) -> Result<String, String> {
        let quote = self.peek().ok_or_else(invalid)?;
        self.offset += 1;
        let multiline =
            self.text.as_bytes().get(self.offset..self.offset + 2) == Some(&[quote, quote]);
        if multiline {
            if !allow_multiline {
                return Err(invalid());
            }
            self.offset += 2;
            self.newline()?;
        }
        let mut out = String::new();
        loop {
            let byte = self.peek().ok_or_else(invalid)?;
            if byte == quote {
                if !multiline {
                    self.offset += 1;
                    return Ok(out);
                }
                let count = self.text.as_bytes()[self.offset..]
                    .iter()
                    .take_while(|&&byte| byte == quote)
                    .count();
                if count > 5 {
                    return Err(invalid());
                }
                self.offset += count;
                for _ in 0..count.min(2) {
                    if count < 3 {
                        out.push(quote as char);
                    }
                }
                if count >= 3 {
                    for _ in 0..count - 3 {
                        out.push(quote as char);
                    }
                    return Ok(out);
                }
                continue;
            }
            if matches!(byte, b'\r' | b'\n') {
                if !multiline {
                    return Err(invalid());
                }
                self.newline()?;
                out.push('\n');
                continue;
            }
            if byte == b'\\' && quote == b'"' {
                self.offset += 1;
                if multiline && matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                    self.horizontal();
                    if !self.newline()? {
                        return Err(invalid());
                    }
                    loop {
                        self.horizontal();
                        if !self.newline()? {
                            break;
                        }
                    }
                    continue;
                }
                let escaped = self.peek().ok_or_else(invalid)?;
                self.offset += 1;
                let character = match escaped {
                    b'b' => '\u{0008}',
                    b't' => '\t',
                    b'n' => '\n',
                    b'f' => '\u{000c}',
                    b'r' => '\r',
                    b'"' => '"',
                    b'\\' => '\\',
                    b'u' | b'U' => {
                        let length = if escaped == b'u' { 4 } else { 8 };
                        let end = self.offset.checked_add(length).ok_or_else(invalid)?;
                        let digits = self.text.get(self.offset..end).ok_or_else(invalid)?;
                        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                            return Err(invalid());
                        }
                        self.offset = end;
                        char::from_u32(u32::from_str_radix(digits, 16).map_err(|_| invalid())?)
                            .ok_or_else(invalid)?
                    }
                    _ => return Err(invalid()),
                };
                out.push(character);
                continue;
            }
            let character = self.text[self.offset..]
                .chars()
                .next()
                .ok_or_else(invalid)?;
            if character < '\u{0020}' && character != '\t' || character == '\u{007f}' {
                return Err(invalid());
            }
            self.offset += character.len_utf8();
            out.push(character);
        }
    }
}

fn digits(raw: &str, radix: u32) -> bool {
    let mut previous_digit = false;
    for character in raw.chars() {
        if character == '_' {
            if !previous_digit {
                return false;
            }
            previous_digit = false;
        } else if character.is_ascii() && character.is_digit(radix) {
            previous_digit = true;
        } else {
            return false;
        }
    }
    previous_digit
}

fn valid_atom(raw: &str) -> bool {
    if matches!(
        raw,
        "true" | "false" | "inf" | "+inf" | "-inf" | "nan" | "+nan" | "-nan"
    ) {
        return true;
    }
    if chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d").is_ok()
        || chrono::NaiveTime::parse_from_str(raw, "%H:%M:%S%.f").is_ok()
        || chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f").is_ok()
        || chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f").is_ok()
    {
        return true;
    }
    let timestamp = raw.replace('t', "T").replace('z', "Z").replace(' ', "T");
    if chrono::DateTime::parse_from_rfc3339(&timestamp).is_ok() {
        return true;
    }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(value) = raw.strip_prefix(prefix) {
            return digits(value, radix)
                && i64::from_str_radix(&value.replace('_', ""), radix).is_ok();
        }
    }
    let unsigned = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    let mut exponent = unsigned.split(['e', 'E']);
    let mantissa = exponent.next().unwrap_or_default();
    let power = exponent.next();
    if exponent.next().is_some()
        || power.is_some_and(|value| !digits(value.strip_prefix(['+', '-']).unwrap_or(value), 10))
    {
        return false;
    }
    let mut fraction = mantissa.split('.');
    let whole = fraction.next().unwrap_or_default();
    let part = fraction.next();
    if fraction.next().is_some()
        || !digits(whole, 10)
        || part.is_some_and(|value| !digits(value, 10))
    {
        return false;
    }
    let whole = whole.replace('_', "");
    if whole.len() > 1 && whole.starts_with('0') {
        return false;
    }
    let clean = raw.replace('_', "");
    if power.is_some() || part.is_some() {
        clean.parse::<f64>().is_ok()
    } else {
        clean.parse::<i64>().is_ok()
    }
}
