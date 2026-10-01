//! Named broker contract tests migrated from w_rounds.rs.
use super::gates::{coord_req, enabled_opts};
use super::*;
use crate::fake::FakeAdapter;
use crate::protocol::{OBSERVATION_NODE_CAP, OBSERVATION_PNG_B64_CAP};

fn ready_png() -> Result<(ComputerUseBroker, Arc<FakeAdapter>, u64, Observation), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let tid = fake.fixture_id();
    let gen = broker
        .authorize_target("run-1", &tid)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
    Ok((broker, fake, gen, obs))
}

fn w1_observation_node_cap() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    fake.set_node_count(OBSERVATION_NODE_CAP + 20);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    broker
        .authorize_target("run-1", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
    if obs.nodes.len() != OBSERVATION_NODE_CAP {
        return Err(format!(
            "node cap {}, got {}",
            OBSERVATION_NODE_CAP,
            obs.nodes.len()
        ));
    }
    if !obs.truncated {
        return Err("overflow nodes must set truncated".into());
    }
    println!("gate: w1_observation_node_cap");
    Ok(())
}

fn w2_huge_png_blocks_coordinates() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    fake.set_huge_png(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let tid = fake.fixture_id();
    let gen = broker
        .authorize_target("run-1", &tid)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
    if obs.image.png_base64.is_some() {
        return Err("oversized png must be dropped before the model sees it".into());
    }
    if !obs.truncated {
        return Err("oversized png must set truncated".into());
    }
    if obs
        .image
        .png_base64
        .as_ref()
        .is_some_and(|s| s.len() > OBSERVATION_PNG_B64_CAP)
    {
        return Err("png cap leaked".into());
    }
    let out = broker.act(coord_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "huge-coord",
    ));
    if out.executed {
        return Err("coordinate click without a visual observation must not execute".into());
    }
    if !fake.executions().is_empty() {
        return Err("huge png must not unlock desktop input".into());
    }
    println!("gate: w2_huge_png_blocks_coordinates");
    Ok(())
}

fn w3_ipc_forwarded_forbidden() -> Result<(), String> {
    let (server, token) = super::test_support::ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .bearer_auth(&token)
        .header("X-Forwarded-For", "8.8.8.8")
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() != 403 {
        return Err(format!(
            "x-forwarded-for must be 403, got {}",
            response.status()
        ));
    }
    println!("gate: w3_ipc_forwarded_forbidden");
    Ok(())
}

fn w4_model_status_traces_capped() -> Result<(), String> {
    let (broker, _, _, _) = ready_png()?;
    for _ in 0..40 {
        broker.observe("run-1").map_err(|e| e.to_string())?;
    }
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result =
        crate::tools::dispatch(&broker, &binding, "computer_status", serde_json::json!({}));
    if result.get("isError") == Some(&serde_json::json!(true)) {
        return Err("status must succeed".into());
    }
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    let payload: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("status json {e} text={text}"))?;
    let traces = payload
        .get("traces")
        .and_then(|v| v.as_array())
        .ok_or("status traces missing")?;
    if traces.len() > crate::protocol::MODEL_TRACE_CAP {
        return Err(format!(
            "model traces {}, cap {}",
            traces.len(),
            crate::protocol::MODEL_TRACE_CAP
        ));
    }
    println!("gate: w4_model_status_traces_capped");
    Ok(())
}

fn w5_observe_json_omits_png() -> Result<(), String> {
    let (broker, _, _, _) = ready_png()?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let result =
        crate::tools::dispatch(&broker, &binding, "computer_observe", serde_json::json!({}));
    let content = result
        .get("content")
        .and_then(|v| v.as_array())
        .ok_or("observe content")?;
    let has_image = content
        .iter()
        .any(|p| p.get("type").and_then(|t| t.as_str()) == Some("image"));
    if !has_image {
        return Err("observe must still attach an image part".into());
    }
    let text = content
        .iter()
        .find_map(|p| {
            if p.get("type").and_then(|t| t.as_str()) == Some("text") {
                p.get("text").and_then(|t| t.as_str())
            } else {
                None
            }
        })
        .ok_or("observe text")?;
    if text.contains("pngBase64") || text.contains("png_base64") {
        return Err("observation JSON must not repeat png bytes".into());
    }
    println!("gate: w5_observe_json_omits_png");
    Ok(())
}

fn w6_truncated_element_ref_rejected() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    fake.set_node_count(OBSERVATION_NODE_CAP + 5);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let tid = fake.fixture_id();
    let gen = broker
        .authorize_target("run-1", &tid)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
    let dropped = format!("n{}", OBSERVATION_NODE_CAP + 1);
    let mut req = super::gates::click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "dropped-ref",
    );
    req.target = crate::protocol::ActionTarget::Element {
        element_ref: dropped,
    };
    let out = broker.act(req);
    if out.executed {
        return Err("elementRef past the cap must not execute".into());
    }
    if !fake.executions().is_empty() {
        return Err("truncated ref reached the adapter".into());
    }
    println!("gate: w6_truncated_element_ref_rejected");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn observation_node_cap() {
        super::w1_observation_node_cap().expect("observation_node_cap");
    }
    #[test]
    fn huge_png_blocks_coordinates() {
        super::w2_huge_png_blocks_coordinates().expect("huge_png_blocks_coordinates");
    }
    #[test]
    fn ipc_forwarded_forbidden() {
        super::w3_ipc_forwarded_forbidden().expect("ipc_forwarded_forbidden");
    }
    #[test]
    fn model_status_traces_capped() {
        super::w4_model_status_traces_capped().expect("model_status_traces_capped");
    }
    #[test]
    fn observe_json_omits_png() {
        super::w5_observe_json_omits_png().expect("observe_json_omits_png");
    }
    #[test]
    fn truncated_element_ref_rejected() {
        super::w6_truncated_element_ref_rejected().expect("truncated_element_ref_rejected");
    }
}
