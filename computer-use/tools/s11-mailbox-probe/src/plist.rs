//! Bounded plist subset for hdiutil/diskutil. Rejects entity declarations.
//! Missing keys stay unknown; they are not treated as a pass.

#[derive(Debug, Clone, PartialEq)]
pub enum Plist {
    Dict(Vec<(String, Plist)>),
    Array(Vec<Plist>),
    String(String),
    Int(u64),
    Bool(bool),
    Other,
}

impl Plist {
    pub fn get(&self, key: &str) -> Option<&Plist> {
        match self {
            Plist::Dict(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn string(&self) -> Option<&str> {
        match self {
            Plist::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn bool(&self) -> Option<bool> {
        match self {
            Plist::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<u64> {
        match self {
            Plist::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn dict_array(&self, key: &str) -> Vec<&Plist> {
        match self.get(key) {
            Some(Plist::Array(items)) => items.iter().collect(),
            _ => Vec::new(),
        }
    }
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
    nodes: usize,
}

impl<'a> P<'a> {
    fn skip_ws(&mut self) {
        while matches!(self.s.get(self.i), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.i += 1;
        }
    }

    fn starts(&self, lit: &str) -> bool {
        self.s.get(self.i..self.i + lit.len()) == Some(lit.as_bytes())
    }

    fn bump(&mut self) -> Result<(), &'static str> {
        self.nodes += 1;
        if self.nodes > 512 || self.depth > 8 {
            Err("plist-limit")
        } else {
            Ok(())
        }
    }

    fn skip_comment_or_decl(&mut self) -> Result<(), &'static str> {
        loop {
            self.skip_ws();
            if self.starts("<?") {
                let end = self.find_from("?>").ok_or("plist-syntax")?;
                self.i = end + 2;
                continue;
            }
            if self.starts("<!--") {
                let end = self.find_from("-->").ok_or("plist-syntax")?;
                self.i = end + 3;
                continue;
            }
            if self.starts("<!DOCTYPE") {
                let end = self.find_from(">").ok_or("plist-syntax")?;
                let body = std::str::from_utf8(&self.s[self.i..end]).map_err(|_| "plist-syntax")?;
                if body.contains('[') || body.to_ascii_lowercase().contains("entity") {
                    return Err("plist-entity");
                }
                self.i = end + 1;
                continue;
            }
            if self.starts("<!") {
                return Err("plist-entity");
            }
            return Ok(());
        }
    }

    fn find_from(&self, lit: &str) -> Option<usize> {
        let rest = &self.s[self.i..];
        let needle = lit.as_bytes();
        rest.windows(needle.len())
            .position(|w| w == needle)
            .map(|p| self.i + p)
    }

    fn text_until(&mut self, end: &str) -> Result<String, &'static str> {
        let at = self.find_from(end).ok_or("plist-syntax")?;
        let raw = std::str::from_utf8(&self.s[self.i..at]).map_err(|_| "plist-syntax")?;
        self.i = at + end.len();
        decode_entities(raw)
    }

    fn open_tag(&mut self) -> Result<(&'a str, bool), &'static str> {
        self.skip_comment_or_decl()?;
        self.skip_ws();
        if !self.starts("<") {
            return Err("plist-syntax");
        }
        self.i += 1;
        if self.s.get(self.i) == Some(&b'/') {
            return Err("plist-syntax");
        }
        let start = self.i;
        while let Some(b) = self.s.get(self.i) {
            if b.is_ascii_whitespace() || *b == b'>' || *b == b'/' {
                break;
            }
            self.i += 1;
        }
        let name = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| "plist-syntax")?;
        if !matches!(
            name,
            "plist"
                | "dict"
                | "array"
                | "key"
                | "string"
                | "integer"
                | "true"
                | "false"
                | "real"
                | "data"
                | "date"
        ) {
            return Err("plist-tag");
        }
        let mut close = false;
        loop {
            self.skip_ws();
            match self.s.get(self.i) {
                Some(&b'>') => {
                    self.i += 1;
                    break;
                }
                Some(&b'/') => {
                    close = true;
                    self.i += 1;
                }
                Some(_) => {
                    // Skip a single attribute. No entity expansion inside it.
                    while let Some(b) = self.s.get(self.i) {
                        if *b == b'>' || *b == b'/' {
                            break;
                        }
                        if *b == b'<' {
                            return Err("plist-syntax");
                        }
                        self.i += 1;
                    }
                }
                None => return Err("plist-syntax"),
            }
        }
        Ok((name, close))
    }

    fn value(&mut self) -> Result<Plist, &'static str> {
        self.bump()?;
        let (name, closed) = self.open_tag()?;
        if name == "plist" {
            let inner = self.value()?;
            self.expect_close("plist")?;
            return Ok(inner);
        }
        if closed {
            return match name {
                "true" => Ok(Plist::Bool(true)),
                "false" => Ok(Plist::Bool(false)),
                "dict" => Ok(Plist::Dict(Vec::new())),
                "array" => Ok(Plist::Array(Vec::new())),
                "string" => Ok(Plist::String(String::new())),
                _ => Ok(Plist::Other),
            };
        }
        match name {
            "true" => {
                self.expect_close("true")?;
                Ok(Plist::Bool(true))
            }
            "false" => {
                self.expect_close("false")?;
                Ok(Plist::Bool(false))
            }
            "string" => {
                let text = self.text_until("</string>")?;
                Ok(Plist::String(text))
            }
            "integer" => {
                let text = self.text_until("</integer>")?;
                if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("plist-int");
                }
                let n: u64 = text.parse().map_err(|_| "plist-int")?;
                Ok(Plist::Int(n))
            }
            "key" => Err("plist-syntax"),
            "dict" => {
                self.depth += 1;
                let mut pairs = Vec::new();
                loop {
                    self.skip_comment_or_decl()?;
                    self.skip_ws();
                    if self.starts("</dict>") {
                        self.i += "</dict>".len();
                        self.depth -= 1;
                        return Ok(Plist::Dict(pairs));
                    }
                    let (tag, closed) = self.open_tag()?;
                    if tag != "key" || closed {
                        return Err("plist-syntax");
                    }
                    let key = self.text_until("</key>")?;
                    if pairs.iter().any(|(k, _)| k == &key) {
                        return Err("plist-dup");
                    }
                    if pairs.len() >= 256 {
                        return Err("plist-limit");
                    }
                    pairs.push((key, self.value()?));
                }
            }
            "array" => {
                self.depth += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_comment_or_decl()?;
                    self.skip_ws();
                    if self.starts("</array>") {
                        self.i += "</array>".len();
                        self.depth -= 1;
                        return Ok(Plist::Array(items));
                    }
                    if items.len() >= 256 {
                        return Err("plist-limit");
                    }
                    items.push(self.value()?);
                }
            }
            "real" | "data" | "date" => {
                let close = format!("</{name}>");
                let at = self.find_from(&close).ok_or("plist-syntax")?;
                self.i = at + close.len();
                Ok(Plist::Other)
            }
            _ => Err("plist-tag"),
        }
    }

    fn expect_close(&mut self, name: &str) -> Result<(), &'static str> {
        self.skip_ws();
        let lit = format!("</{name}>");
        if self.starts(&lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err("plist-syntax")
        }
    }
}

fn decode_entities(raw: &str) -> Result<String, &'static str> {
    let mut out = String::new();
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'&' {
            let ch = raw[i..].chars().next().ok_or("plist-syntax")?;
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let rest = &raw[i..];
        if let Some(stripped) = rest.strip_prefix("&amp;") {
            out.push('&');
            i = raw.len() - stripped.len();
        } else if let Some(stripped) = rest.strip_prefix("&lt;") {
            out.push('<');
            i = raw.len() - stripped.len();
        } else if let Some(stripped) = rest.strip_prefix("&gt;") {
            out.push('>');
            i = raw.len() - stripped.len();
        } else if let Some(stripped) = rest.strip_prefix("&quot;") {
            out.push('"');
            i = raw.len() - stripped.len();
        } else if let Some(stripped) = rest.strip_prefix("&apos;") {
            out.push('\'');
            i = raw.len() - stripped.len();
        } else {
            return Err("plist-entity");
        }
    }
    Ok(out)
}

pub fn parse_plist(input: &str) -> Result<Plist, &'static str> {
    if input.len() > 1024 * 1024 {
        return Err("plist-limit");
    }
    if input.to_ascii_lowercase().contains("<!entity") {
        return Err("plist-entity");
    }
    let mut p = P {
        s: input.as_bytes(),
        i: 0,
        depth: 0,
        nodes: 0,
    };
    let v = p.value()?;
    p.skip_ws();
    if p.i != p.s.len() {
        return Err("plist-trailing");
    }
    Ok(v)
}

#[derive(Clone, Debug)]
pub struct ImageAttach {
    pub image_path: String,
    pub whole: String,
    pub slice: Option<String>,
    pub mount: Option<String>,
}

pub fn whole_disk(dev: &str) -> bool {
    let rest = dev.strip_prefix("/dev/disk").unwrap_or("");
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

pub fn slice_disk(dev: &str) -> bool {
    let Some(rest) = dev.strip_prefix("/dev/disk") else {
        return false;
    };
    let Some((num, part)) = rest.split_once('s') else {
        return false;
    };
    !num.is_empty()
        && num.bytes().all(|b| b.is_ascii_digit())
        && !part.is_empty()
        && part.bytes().all(|b| b.is_ascii_digit())
}

/// Images from `hdiutil info -plist` or `hdiutil attach -plist`.
pub fn attached_images(doc: &Plist) -> Result<Vec<ImageAttach>, &'static str> {
    let entities = if doc.get("images").is_some() {
        doc.dict_array("images")
    } else if doc.get("system-entities").is_some() {
        vec![doc]
    } else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for image in entities {
        let path = image
            .get("image-path")
            .and_then(Plist::string)
            .unwrap_or("")
            .to_string();
        let mut whole = None;
        let mut slice = None;
        let mut mount = None;
        let sys = image.dict_array("system-entities");
        for ent in sys {
            let dev = ent.get("dev-entry").and_then(Plist::string).unwrap_or("");
            if dev.is_empty() {
                return Err("plist-incomplete");
            }
            if whole_disk(dev) {
                if whole.is_some() {
                    return Err("plist-ambiguous");
                }
                whole = Some(dev.to_string());
            } else if slice_disk(dev) {
                slice = Some(dev.to_string());
            } else {
                return Err("plist-device");
            }
            if let Some(m) = ent.get("mount-point").and_then(Plist::string) {
                if !m.is_empty() {
                    mount = Some(m.to_string());
                }
            }
        }
        if path.is_empty() && whole.is_none() {
            continue;
        }
        out.push(ImageAttach {
            image_path: path,
            whole: whole.unwrap_or_default(),
            slice,
            mount,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_entities_and_reads_mapping() {
        assert_eq!(
            parse_plist(r#"<!DOCTYPE plist [<!ENTITY x "y">]><plist><string>a</string></plist>"#)
                .unwrap_err(),
            "plist-entity"
        );
        let doc = parse_plist(
            r#"<?xml version="1.0"?>
            <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
            <plist version="1.0"><dict>
              <key>images</key><array><dict>
                <key>image-path</key><string>/vol/mailbox.sparseimage</string>
                <key>system-entities</key><array>
                  <dict><key>dev-entry</key><string>/dev/disk4</string></dict>
                  <dict><key>dev-entry</key><string>/dev/disk4s1</string>
                        <key>mount-point</key><string>/vol/mnt</string></dict>
                </array>
              </dict></array>
            </dict></plist>"#,
        )
        .unwrap();
        let images = attached_images(&doc).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].whole, "/dev/disk4");
        assert_eq!(images[0].slice.as_deref(), Some("/dev/disk4s1"));
        assert_eq!(images[0].mount.as_deref(), Some("/vol/mnt"));
    }
}
