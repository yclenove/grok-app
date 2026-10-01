//! Test-only private stdio control. No control routes are added to product HTTP.
use grok_computer_use_core::{
    broker::{BrokerOptions, ComputerUseBroker},
    fake::FakeAdapter,
    ipc,
};
use serde_json::json;
use std::io::{BufRead, IsTerminal, Write};
use std::sync::Arc;

fn main() {
    if std::io::stdin().is_terminal() {
        eprintln!("pairing fixture requires a private parent pipe");
        std::process::exit(2);
    }
    let broker = Arc::new(ComputerUseBroker::new(
        Arc::new(FakeAdapter::new()),
        BrokerOptions {
            feature_enabled: true,
            ..BrokerOptions::default()
        },
    ));
    let server = ipc::spawn(broker.clone()).expect("fixture loopback");
    for line in std::io::stdin().lock().lines() {
        let line = line.expect("fixture stdin");
        let value: serde_json::Value = serde_json::from_str(&line).expect("fixture command");
        let result = match value["command"].as_str().unwrap_or_default() {
            "begin" => {
                let ch = broker.begin_pairing().expect("fixture challenge");
                json!({"endpoint": server.url, "nonce": ch.nonce, "code": ch.verification_code})
            }
            "confirm" => {
                json!({"ok": broker.confirm_pairing(value["nonce"].as_str().unwrap_or_default()).is_ok()})
            }
            "status" => json!({"paired": broker.tabs().pairing_session_key().is_some(),
                "stored": broker.tabs().has_stored_pairing_connection_for_test()}),
            "shared" => json!(broker
                .tabs()
                .list_shared_candidates()
                .into_iter()
                .map(|(id, title, url)| json!({"id": id, "title": title, "url": url}))
                .collect::<Vec<_>>()),
            "revoke" => {
                broker.tabs().revoke_pairing();
                json!({"ok": true})
            }
            "observe-shared" => {
                let tab = broker
                    .tabs()
                    .grant_picker_tab(
                        "probe-owner",
                        "probe-run",
                        value["selector"].as_str().unwrap_or_default(),
                    )
                    .expect("fixture candidate");
                let observed = broker.tabs().observe_existing_document(
                    "probe-owner",
                    "probe-run",
                    &tab.tab_id,
                    value["screenshot"].as_bool().unwrap_or(false),
                );
                let _ = broker.tabs().return_borrowed("probe-run", &tab.tab_id);
                match observed {
                    Ok(observation) => json!({"ok": true, "observation": observation}),
                    Err(_) => json!({"ok": false}),
                }
            }
            "shutdown" => break,
            _ => json!({"ok": false}),
        };
        // The parent never logs this private pipe; codes are not CLI arguments or files.
        println!("{result}");
        std::io::stdout().flush().expect("fixture stdout");
    }
}
