//! Strict `tart list --format json` subset. A parse failure is untrusted, not "absent".

use crate::jsonutil::{self, Limits, Val};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vm {
    pub name: String,
    pub state: String,
}

const KEYS: &[&str] = &[
    "Name", "State", "Source", "Disk", "Size", "Running", "Accessed",
];

pub fn parse_tart_list(text: &str) -> Result<Vec<Vm>, &'static str> {
    let val = jsonutil::parse(text, &Limits::tart_list())?;
    let Val::Arr(items) = val else {
        return Err("list-untrusted");
    };
    let mut vms = Vec::new();
    for item in items {
        let Val::Obj(pairs) = item else {
            return Err("list-untrusted");
        };
        let mut name = None;
        let mut state = None;
        for (k, v) in &pairs {
            if !KEYS.contains(&k.as_str()) {
                return Err("list-untrusted");
            }
            match k.as_str() {
                "Name" => {
                    let s = v.str().ok_or("list-untrusted")?;
                    if s.is_empty() || s.contains('\t') || s.contains('/') {
                        return Err("list-untrusted");
                    }
                    name = Some(s.to_string());
                }
                "State" => {
                    let s = v.str().ok_or("list-untrusted")?;
                    let norm = s.to_ascii_lowercase();
                    if !matches!(norm.as_str(), "stopped" | "running" | "suspended") {
                        return Err("list-untrusted");
                    }
                    state = Some(norm);
                }
                "Running" => {
                    v.bool().ok_or("list-untrusted")?;
                }
                "Size" | "Disk" => {
                    v.int().ok_or("list-untrusted")?;
                }
                "Source" | "Accessed" => {
                    v.str().ok_or("list-untrusted")?;
                }
                _ => return Err("list-untrusted"),
            }
        }
        let name = name.ok_or("list-untrusted")?;
        let state = state.ok_or("list-untrusted")?;
        if vms.iter().any(|vm: &Vm| vm.name == name) {
            return Err("list-untrusted");
        }
        vms.push(Vm { name, state });
    }
    Ok(vms)
}

pub fn find<'a>(vms: &'a [Vm], name: &str) -> Option<&'a Vm> {
    vms.iter().find(|vm| vm.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_one_array_and_rejects_partial_or_dup() {
        let ok = r#"[{"Name":"macos-s2-eval","State":"stopped","Running":false,"Size":1}]"#;
        let vms = parse_tart_list(ok).unwrap();
        assert_eq!(vms[0].name, "macos-s2-eval");
        assert_eq!(vms[0].state, "stopped");
        assert!(parse_tart_list("[]").unwrap().is_empty());
        assert!(parse_tart_list(r#"[{"Name":"a","State":"nope"}]"#).is_err());
        assert!(parse_tart_list(
            r#"[{"Name":"a","State":"stopped"},{"Name":"a","State":"stopped"}]"#
        )
        .is_err());
        assert!(parse_tart_list(r#"[{"Name":"a","State":"stopped","Extra":1}]"#).is_err());
        assert!(parse_tart_list(r#"[{"State":"stopped"}]"#).is_err());
        assert!(parse_tart_list("not-json").is_err());
    }
}
