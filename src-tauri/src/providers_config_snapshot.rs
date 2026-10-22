//! Private TOML structure for routing and isolated CLI configuration projection.
//! Never log this tree or expose its credential-bearing text through an API.
//! Unlike the legacy editor's line indexes, tables inside strings/arrays cannot
//! become provider sections here. Projection never writes the source config.
use std::collections::{BTreeMap, HashMap};

const MAX_CONFIG_DEPTH: usize = 64;

#[path = "providers_config_mcp.rs"]
mod mcp;
#[path = "providers_config_official_projection.rs"]
mod official_projection;
#[path = "providers_config_profile_auth.rs"]
mod profile_auth;
#[path = "providers_config_profile_routes.rs"]
mod profile_routes;
#[path = "providers_config_projection.rs"]
mod projection;
#[path = "providers_config_runtime.rs"]
mod runtime;
#[cfg(test)]
#[path = "providers_config_snapshot_tests.rs"]
mod tests;
#[path = "providers_config_snapshot_value.rs"]
mod value;
#[path = "providers_config_snapshot_write.rs"]
mod write;

pub(super) struct ConfigSnapshot {
    root: Table,
}

#[derive(Default)]
struct Table {
    fields: BTreeMap<String, Value>,
    defined: bool,
    inline: bool,
}

enum Value {
    String(String),
    Atom(String),
    Array(Vec<Value>),
    Table(Table),
    TableArray(Vec<Table>),
}

fn invalid() -> String {
    // Neither parse errors nor Debug output may carry source/key/header values.
    "agent configuration has invalid or ambiguous TOML structure".into()
}

fn extend_depth(depth: usize, levels: usize) -> Result<usize, String> {
    depth
        .checked_add(levels)
        .filter(|depth| *depth < MAX_CONFIG_DEPTH)
        .ok_or_else(invalid)
}

impl ConfigSnapshot {
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let mut parser = Parser { text, offset: 0 };
        let mut root = Table::default();
        let mut scope = Vec::new();
        loop {
            parser.space_and_comments()?;
            if parser.peek().is_none() {
                return Ok(Self { root });
            }
            if parser.take(b'[') {
                let array = parser.take(b'[');
                let path = parser.key_path()?;
                parser.expect(b']')?;
                if array {
                    parser.expect(b']')?;
                }
                parser.end_line()?;
                declare_table(&mut root, &path, array, 0)?;
                scope = path;
            } else {
                let key = parser.key_path()?;
                parser.expect(b'=')?;
                parser.horizontal();
                let (table, depth) = table_at(&mut root, &scope)?;
                let value = parser.value(extend_depth(depth, key.len())?)?;
                parser.end_line()?;
                assign(table, &key, value)?;
            }
        }
    }

    pub(super) fn default_model(&self) -> Result<Option<&str>, String> {
        self.model_slot("default")
    }

    pub(super) fn model_slot(&self, key: &str) -> Result<Option<&str>, String> {
        match self.root.fields.get("models") {
            None => Ok(None),
            Some(Value::Table(table)) => match table.fields.get(key) {
                None => Ok(None),
                Some(Value::String(value)) => Ok(Some(value)),
                _ => Err(invalid()),
            },
            _ => Err(invalid()),
        }
    }

    fn models(&self) -> Result<Option<&Table>, String> {
        match self.root.fields.get("model") {
            None => Ok(None),
            Some(Value::Table(table)) => Ok(Some(table)),
            _ => Err(invalid()),
        }
    }

    pub(super) fn model_ids(&self) -> Result<Vec<String>, String> {
        Ok(self.models()?.map_or_else(Vec::new, |table| {
            table
                .fields
                .iter()
                .filter(|(_, value)| matches!(value, Value::Table(_)))
                .map(|(key, _)| key.clone())
                .collect()
        }))
    }

    pub(super) fn model_fields(&self, id: &str) -> Result<Option<HashMap<String, String>>, String> {
        let Some(table) = self.models()? else {
            return Ok(None);
        };
        let Some(value) = table.fields.get(id) else {
            return Ok(None);
        };
        let Value::Table(table) = value else {
            return Err(invalid());
        };
        let mut fields = HashMap::new();
        for (key, value) in &table.fields {
            if matches!(
                key.as_str(),
                "model"
                    | "base_url"
                    | "api_key"
                    | "api_backend"
                    | "app_provider_mode"
                    | "app_models"
                    | "app_upstream_base_url"
                    | "model_provider"
                    | "auth_provider"
            ) && !matches!(value, Value::String(_))
            {
                return Err(invalid());
            }
            match value {
                Value::String(value) | Value::Atom(value) => {
                    fields.insert(key.clone(), value.clone());
                }
                // Nested headers/query tables are not direct route fields.
                // Keep them in the source, never flatten them into credentials.
                _ => {}
            }
        }
        Ok(Some(fields))
    }

    pub(super) fn key_only_catalog_override(&self, id: &str) -> Result<bool, String> {
        let Some(models) = self.models()? else {
            return Ok(false);
        };
        let Some(Value::Table(model)) = models.fields.get(id) else {
            return Err(invalid());
        };
        // Inspect the actual tree: model_fields intentionally omits nested
        // headers/query tables, so its scalar count cannot prove key-only.
        Ok(model.fields.len() == 1
            && matches!(model.fields.get("api_key"), Some(Value::String(key)) if !key.trim().is_empty()))
    }
}

fn descend(value: &mut Value) -> Result<&mut Table, String> {
    let table = match value {
        Value::Table(table) => table,
        Value::TableArray(tables) => tables.last_mut().ok_or_else(invalid)?,
        _ => return Err(invalid()),
    };
    if table.inline {
        return Err(invalid());
    }
    Ok(table)
}

fn table_at<'a>(root: &'a mut Table, path: &[String]) -> Result<(&'a mut Table, usize), String> {
    let mut current = root;
    let mut depth = 0;
    for key in path {
        let value = current.fields.get_mut(key).ok_or_else(invalid)?;
        depth = extend_depth(
            depth,
            if matches!(value, Value::TableArray(_)) {
                2
            } else {
                1
            },
        )?;
        current = descend(value)?;
    }
    Ok((current, depth))
}

fn declare_table(
    root: &mut Table,
    path: &[String],
    array: bool,
    depth: usize,
) -> Result<(), String> {
    let (key, tail) = path.split_first().ok_or_else(invalid)?;
    if !tail.is_empty() {
        let depth = extend_depth(
            depth,
            if matches!(root.fields.get(key), Some(Value::TableArray(_))) {
                2
            } else {
                1
            },
        )?;
        let next = root
            .fields
            .entry(key.clone())
            .or_insert_with(|| Value::Table(Table::default()));
        return declare_table(descend(next)?, tail, array, depth);
    }
    // An array-of-tables adds both an array and its latest table instance.
    // Check before inserting either node, including for an empty declaration.
    extend_depth(depth, if array { 2 } else { 1 })?;
    if array {
        let value = root
            .fields
            .entry(key.clone())
            .or_insert_with(|| Value::TableArray(Vec::new()));
        let Value::TableArray(tables) = value else {
            return Err(invalid());
        };
        tables.push(Table {
            defined: true,
            ..Table::default()
        });
    } else {
        let value = root
            .fields
            .entry(key.clone())
            .or_insert_with(|| Value::Table(Table::default()));
        let Value::Table(table) = value else {
            return Err(invalid());
        };
        if table.defined || table.inline {
            return Err(invalid());
        }
        table.defined = true;
    }
    Ok(())
}

fn assign(root: &mut Table, path: &[String], value: Value) -> Result<(), String> {
    let (key, tail) = path.split_first().ok_or_else(invalid)?;
    if root.inline {
        return Err(invalid());
    }
    if !tail.is_empty() {
        let next = root.fields.entry(key.clone()).or_insert_with(|| {
            Value::Table(Table {
                defined: true,
                ..Table::default()
            })
        });
        // Dotted keys cannot extend an array of tables as an assignment.
        let Value::Table(table) = next else {
            return Err(invalid());
        };
        return assign(table, tail, value);
    }
    if root.fields.contains_key(key) {
        return Err(invalid());
    }
    root.fields.insert(key.clone(), value);
    Ok(())
}

struct Parser<'a> {
    text: &'a str,
    offset: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.offset).copied()
    }

    fn take(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        self.take(byte).then_some(()).ok_or_else(invalid)
    }

    fn horizontal(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.offset += 1;
        }
    }

    fn newline(&mut self) -> Result<bool, String> {
        if self.take(b'\r') {
            self.expect(b'\n')?;
            return Ok(true);
        }
        Ok(self.take(b'\n'))
    }

    fn comment(&mut self) -> Result<(), String> {
        if self.take(b'#') {
            while let Some(byte) = self.peek() {
                if matches!(byte, b'\r' | b'\n') {
                    break;
                }
                if byte < 0x20 && byte != b'\t' || byte == 0x7f {
                    return Err(invalid());
                }
                self.offset += 1;
            }
        }
        Ok(())
    }

    fn space_and_comments(&mut self) -> Result<(), String> {
        loop {
            self.horizontal();
            self.comment()?;
            if !self.newline()? {
                return Ok(());
            }
        }
    }

    fn end_line(&mut self) -> Result<(), String> {
        self.horizontal();
        self.comment()?;
        if self.peek().is_none() || self.newline()? {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    fn key_path(&mut self) -> Result<Vec<String>, String> {
        let mut path = Vec::new();
        loop {
            if path.len() >= MAX_CONFIG_DEPTH {
                return Err(invalid());
            }
            self.horizontal();
            let key = if matches!(self.peek(), Some(b'\'' | b'"')) {
                self.string(false)?
            } else {
                let start = self.offset;
                while self
                    .peek()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    self.offset += 1;
                }
                if start == self.offset {
                    return Err(invalid());
                }
                self.text[start..self.offset].to_owned()
            };
            path.push(key);
            self.horizontal();
            if !self.take(b'.') {
                return Ok(path);
            }
        }
    }
}
