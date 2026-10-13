//! Execute the App-embedded MCP catalog with shipped Node, not PATH Node.
//! No model, permission decision, file picker or native assignment is claimed.
use super::*;
use serde_json::{json, Value};
use std::process::Command;

const CATALOG_DRIVER: &str = r#"
import { spawnSync } from 'node:child_process';
const input = [
  { jsonrpc:'2.0', id:1, method:'initialize', params:{protocolVersion:'2024-11-05'} },
  { jsonrpc:'2.0', id:2, method:'tools/list' }
].map(value => JSON.stringify(value)).join('\n') + '\n';
const child = spawnSync(process.execPath, [process.argv[1]], {
  input, encoding:'utf8', timeout:5000, maxBuffer:1024*1024, windowsHide:true,
  env:{ GROK_APP_CU_IPC:'http://127.0.0.1:9',
        GROK_APP_CU_TOKEN:'synthetic-catalog-only', GROK_APP_CU_SESSION:'catalog-only' }
});
if (child.error || child.status !== 0) {
  process.stderr.write(String(child.error?.message || child.stderr || 'MCP child failed'));
  process.exit(1);
}
process.stdout.write(child.stdout);
"#;

#[test]
fn app_embedded_upload_catalog_upgrades_old_runtime_and_executes_with_shipped_node() {
    let root = std::env::temp_dir().join(format!("cu-embedded-catalog-{}", uuid::Uuid::new_v4()));
    let store = RuntimeStore::new(root.clone());
    store
        .ensure_embedded_mcp(
            "export const oldServer = true;\n",
            "export const oldProtocol = true;\n",
        )
        .unwrap();
    store.ensure_embedded_mcp(SCRIPT, PROTOCOL).unwrap();
    let script = store.resolve(MCP_SERVER).unwrap();
    let protocol = store
        .resolve(grok_computer_use_core::runtime::MCP_PROTOCOL)
        .unwrap();
    assert_eq!(std::fs::read(&script).unwrap(), SCRIPT.as_bytes());
    assert_eq!(std::fs::read(&protocol).unwrap(), PROTOCOL.as_bytes());

    let seed = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/computer-use/seed");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(seed.join("manifest.json")).unwrap()).unwrap();
    let node = seed.join(
        manifest["components"][JS_RUNTIME]["relpath"]
            .as_str()
            .unwrap(),
    );
    assert!(node.starts_with(&seed) && node.is_file());
    let mut command = Command::new(&node);
    command
        .args(["--input-type=module", "-e", CATALOG_DRIVER])
        .arg(&script)
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 2);
    let initialize = replies.iter().find(|reply| reply["id"] == 1).unwrap();
    let instructions = initialize["result"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("computer_upload waits for the user"));
    assert!(instructions.contains("Never supply local paths or bytes"));
    assert!(instructions.contains("not website submission"));
    let catalog = replies.iter().find(|reply| reply["id"] == 2).unwrap();
    let tools = catalog["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    let mut expected = grok_computer_use_core::tools::MODEL_TOOLS.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    let upload = tools
        .iter()
        .find(|tool| tool["name"] == "computer_upload")
        .unwrap();
    let schema = &upload["inputSchema"];
    assert_eq!(schema["additionalProperties"], false);
    let mut fields: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| field.as_str().unwrap())
        .collect();
    fields.sort_unstable();
    let mut expected_fields = [
        "version",
        "actionId",
        "runId",
        "targetId",
        "targetGeneration",
        "snapshotId",
        "geometryRevision",
        "elementRef",
    ];
    expected_fields.sort_unstable();
    assert_eq!(fields, expected_fields);
    assert_eq!(schema["properties"]["version"]["const"], json!(1));
    assert_eq!(
        schema["properties"].as_object().unwrap().len(),
        expected_fields.len()
    );
    // Both bounded child processes have exited before the exact test root is removed.
    assert!(!std::fs::symlink_metadata(&root)
        .unwrap()
        .file_type()
        .is_symlink());
    std::fs::remove_dir_all(root).unwrap();
}
