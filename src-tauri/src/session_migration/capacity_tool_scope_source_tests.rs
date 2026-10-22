use super::{json, Path, PathBuf, Provider, SelectionFields};

#[derive(Clone, Copy, Debug)]
pub(super) enum Mode {
    Off,
    Name,
    Input,
    Result,
}

impl Mode {
    pub(super) fn fields(self) -> SelectionFields {
        SelectionFields {
            tool_call_name: matches!(self, Self::Name),
            tool_call_input: matches!(self, Self::Input),
            tool_result_content: matches!(self, Self::Result),
            ..SelectionFields::default()
        }
    }

    pub(super) fn allows(self, index: usize) -> bool {
        matches!(
            (self, index),
            (Self::Name, 0) | (Self::Input, 1) | (Self::Result, 2)
        )
    }

    pub(super) fn readable(self, ids: &[String]) -> Vec<String> {
        ids.iter()
            .enumerate()
            .filter(|(index, _)| match index {
                2 => matches!(self, Self::Name | Self::Input),
                3 => matches!(self, Self::Result),
                _ => true,
            })
            .map(|(_, id)| id.clone())
            .collect()
    }
}

#[derive(Clone)]
pub(super) struct Source {
    pub(super) path: PathBuf,
    pub(super) bytes: Vec<u8>,
    pub(super) provider: Provider,
    pub(super) mode: Mode,
    pub(super) messages: [String; 3],
    pub(super) sentinels: [String; 3],
}

pub(super) fn create(root: &Path) -> Vec<Source> {
    let mut sources = Vec::new();
    for provider in [Provider::Codex, Provider::ClaudeCode, Provider::Cursor] {
        for (index, mode) in [Mode::Off, Mode::Name, Mode::Input, Mode::Result]
            .into_iter()
            .enumerate()
        {
            let id = format!("tools-{}-{index}", provider.as_str());
            let messages = [
                format!("请核对工具字段 {id} 🚀"),
                format!("正在检查 {id}"),
                format!("检查完成 {id}"),
            ];
            let sentinels = ["NAME", "INPUT", "RESULT"]
                .map(|field| format!("ONLY_{field}_{}_{index}", provider.as_str()));
            let input = json!({"payload":sentinels[1]});
            let records = match provider {
                Provider::Codex => vec![
                    json!({"type":"session_meta","payload":{"id":id}}),
                    json!({"type":"response_item","payload":{"type":"message","role":"user",
                        "content":[{"type":"input_text","text":messages[0]}]}}),
                    json!({"type":"response_item","payload":{"type":"message","role":"assistant",
                        "content":[{"type":"output_text","text":messages[1]}]}}),
                    json!({"type":"response_item","payload":{"type":"function_call",
                        "call_id":"call","name":sentinels[0],"arguments":input.to_string()}}),
                    json!({"type":"response_item","payload":{"type":"function_call_output",
                        "call_id":"call","output":sentinels[2]}}),
                    json!({"type":"response_item","payload":{"type":"message","role":"assistant",
                        "content":[{"type":"output_text","text":messages[2]}]}}),
                ],
                Provider::ClaudeCode => vec![
                    json!({"type":"user","uuid":"u1","parentUuid":null,"sessionId":id,
                        "message":{"role":"user","content":messages[0]}}),
                    json!({"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":id,
                        "message":{"role":"assistant","content":[{"type":"text","text":messages[1]},
                            {"type":"tool_use","id":"call","name":sentinels[0],"input":input}]}}),
                    json!({"type":"user","uuid":"r1","parentUuid":"a1","sessionId":id,
                        "message":{"role":"user","content":[{"type":"tool_result",
                            "tool_use_id":"call","content":sentinels[2]}]}}),
                    json!({"type":"assistant","uuid":"a2","parentUuid":"r1","sessionId":id,
                        "message":{"role":"assistant","content":messages[2]}}),
                ],
                Provider::Cursor => vec![
                    json!({"composerId":id,"role":"user","content":messages[0]}),
                    json!({"composerId":id,"role":"assistant","content":messages[1]}),
                    json!({"composerId":id,"role":"assistant","content":[{"type":"tool_use",
                        "id":"call","name":sentinels[0],"input":input}]}),
                    json!({"composerId":id,"role":"user","content":[{"type":"tool_result",
                        "tool_use_id":"call","content":sentinels[2]}]}),
                    json!({"composerId":id,"role":"assistant","content":messages[2]}),
                ],
                _ => unreachable!(),
            };
            let directory = root.join(provider.as_str());
            std::fs::create_dir_all(&directory).unwrap();
            let path = directory.join(format!("{id}.jsonl"));
            let bytes = records
                .iter()
                .map(|record| format!("{record}\n"))
                .collect::<String>()
                .into_bytes();
            std::fs::write(&path, &bytes).unwrap();
            sources.push(Source {
                path,
                bytes,
                provider,
                mode,
                messages,
                sentinels,
            });
        }
    }
    sources
}
