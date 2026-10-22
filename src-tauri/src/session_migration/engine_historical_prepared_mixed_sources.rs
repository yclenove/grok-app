use super::{json, Path, PathBuf, Provider};

pub(super) struct Source {
    pub(super) path: PathBuf,
    pub(super) bytes: Vec<u8>,
    pub(super) provider: Provider,
    pub(super) messages: Vec<String>,
    pub(super) scoped: bool,
}

pub(super) fn create(root: &Path, count: usize) -> Vec<Source> {
    (0..count).map(|index| {
        let provider = [Provider::Codex, Provider::ClaudeCode, Provider::Cursor][index % 3];
        let id = format!("prepared-mixed-{index:04}");
        let messages = vec![
            format!("保留问题 {id} 🚀"),
            format!("EXCLUDED_ASSISTANT_{id}"),
            format!("保留追问 {id}"),
        ];
        let records = match provider {
            Provider::Codex => vec![
                json!({"type":"session_meta","payload":{"id":id}}),
                json!({"type":"event_msg","payload":{"type":"user_message","message":messages[0]}}),
                json!({"type":"event_msg","payload":{"type":"agent_message","message":messages[1]}}),
                json!({"type":"event_msg","payload":{"type":"user_message","message":messages[2]}}),
            ],
            Provider::ClaudeCode => vec![
                json!({"type":"user","uuid":"u1","parentUuid":null,"sessionId":id,
                    "message":{"role":"user","content":messages[0]}}),
                json!({"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":id,
                    "message":{"role":"assistant","content":messages[1]}}),
                json!({"type":"user","uuid":"u2","parentUuid":"a1","sessionId":id,
                    "message":{"role":"user","content":messages[2]}}),
            ],
            Provider::Cursor => vec![
                json!({"composerId":id,"role":"user","content":messages[0]}),
                json!({"composerId":id,"role":"assistant","content":messages[1]}),
                json!({"composerId":id,"role":"user","content":messages[2]}),
            ],
            _ => unreachable!(),
        };
        let directory = root.join(provider.as_str());
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{id}.jsonl"));
        let bytes = records.iter().map(|record| format!("{record}\n"))
            .collect::<String>().into_bytes();
        std::fs::write(&path, &bytes).unwrap();
        Source { path, bytes, provider, messages, scoped: index % 2 == 0 }
    }).collect()
}

pub(super) fn unchanged(root: &Path, offline: &Path, sources: &[Source]) {
    for source in sources {
        assert_eq!(
            std::fs::read(offline.join(source.path.strip_prefix(root).unwrap())).unwrap(),
            source.bytes
        );
    }
}
