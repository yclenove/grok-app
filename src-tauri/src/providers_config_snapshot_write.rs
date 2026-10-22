//! Canonical CLI TOML, retaining all parsed values and table-array instances.
//! Private configuration only: not a log/UI/receipt serializer.
use super::{invalid, Table, Value, MAX_CONFIG_DEPTH};

pub(super) fn to_text(root: &Table) -> Result<String, String> {
    let mut text = String::new();
    table(&mut text, root, &mut Vec::new(), 0)?;
    Ok(text)
}

fn string(out: &mut String, value: &str) -> Result<(), String> {
    // JSON and TOML basic-string escapes agree except for DEL: JSON permits
    // the literal character, while TOML requires an escape even after decoding
    // a valid source such as "\\u007f".
    out.push_str(
        &serde_json::to_string(value)
            .map_err(|_| invalid())?
            .replace('\u{007f}', "\\u007f"),
    );
    Ok(())
}

fn key(out: &mut String, value: &str) -> Result<(), String> {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        out.push_str(value);
        Ok(())
    } else {
        string(out, value)
    }
}

fn header(out: &mut String, path: &[&str], array: bool) -> Result<(), String> {
    out.push_str(if array { "\n[[" } else { "\n[" });
    for (index, part) in path.iter().enumerate() {
        if index > 0 {
            out.push('.');
        }
        key(out, part)?;
    }
    out.push_str(if array { "]]\n" } else { "]\n" });
    Ok(())
}

fn inline_table(out: &mut String, table: &Table, depth: usize) -> Result<(), String> {
    if depth >= MAX_CONFIG_DEPTH {
        return Err(invalid());
    }
    out.push('{');
    for (index, (name, item)) in table.fields.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        key(out, name)?;
        out.push_str(" = ");
        value(out, item, depth + 1)?;
    }
    out.push('}');
    Ok(())
}

fn value(out: &mut String, item: &Value, depth: usize) -> Result<(), String> {
    if depth >= MAX_CONFIG_DEPTH {
        return Err(invalid());
    }
    match item {
        Value::String(text) => string(out, text)?,
        Value::Atom(atom) => out.push_str(atom),
        Value::Table(table) => inline_table(out, table, depth)?,
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                value(out, item, depth + 1)?;
            }
            out.push(']');
        }
        Value::TableArray(tables) => {
            out.push('[');
            for (index, table) in tables.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                inline_table(out, table, depth + 1)?;
            }
            out.push(']');
        }
    }
    Ok(())
}

fn table<'a>(
    out: &mut String,
    item: &'a Table,
    path: &mut Vec<&'a str>,
    depth: usize,
) -> Result<(), String> {
    // Dotted keys can add many table levels inside a single value. Bound the
    // actual tree walk, rather than relying only on lexical parser depth.
    if depth >= MAX_CONFIG_DEPTH {
        return Err(invalid());
    }
    for (name, item) in &item.fields {
        if matches!(item, Value::Table(table) if !table.inline)
            || matches!(item, Value::TableArray(_))
        {
            continue;
        }
        key(out, name)?;
        out.push_str(" = ");
        value(out, item, depth + 1)?;
        out.push('\n');
    }
    for (name, item) in &item.fields {
        match item {
            Value::Table(child) if !child.inline => {
                path.push(name);
                header(out, path, false)?;
                table(out, child, path, depth + 1)?;
                path.pop();
            }
            Value::TableArray(tables) => {
                path.push(name);
                for child in tables {
                    header(out, path, true)?;
                    table(out, child, path, depth + 2)?;
                }
                path.pop();
            }
            _ => {}
        }
    }
    Ok(())
}
