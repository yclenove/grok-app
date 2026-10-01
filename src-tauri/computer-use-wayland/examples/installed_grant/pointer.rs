//! An actual observed-pixel click and native GTK effect, never desktop guesses.
use super::ui::Command;
use base64::Engine;
use grok_computer_use_core::{
    adapter::{ActionScope, CaptureOptions, ComputerUseAdapter, DispatchRequest},
    execution::ActionCancellation,
    protocol::{ActionKind, ActionTarget},
};
use grok_computer_use_wayland::PortalRegistry;
use serde_json::json;
use std::{
    io::Write,
    sync::mpsc,
    time::{Duration, Instant},
};

async fn receipt(queue: &mpsc::Sender<Command>) -> Result<(u64, Vec<(u32, bool)>), String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    queue
        .send(Command::PointerSnapshot(send))
        .map_err(|_| "GTK queue ended")?;
    tokio::time::timeout(Duration::from_secs(2), receive)
        .await
        .map_err(|_| "GTK pointer snapshot deadline")?
        .map_err(|_| "GTK pointer snapshot dropped".into())
}

pub async fn accept(
    registry: &PortalRegistry,
    run: &str,
    target: &str,
    queue: &mpsc::Sender<Command>,
    prior_clicks: u64,
) -> Result<DispatchRequest, String> {
    if prior_clicks > 1 {
        return Err("owned acceptance permits only two original clicks".into());
    }
    let prior_edges = [(1, true), (1, false)].repeat(prior_clicks as usize);
    let before = receipt(queue).await?;
    if before != (prior_clicks, prior_edges.clone()) {
        return Err(format!("unexpected prior pointer effects: {before:?}"));
    }
    let frame =
        registry.capture_for_run_at_generation(run, target, 1, CaptureOptions::model(true))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(
            frame
                .image
                .png_base64
                .as_deref()
                .ok_or("portal PNG missing")?,
        )
        .map_err(|e| e.to_string())?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let pixels = image
        .enumerate_pixels()
        .filter(|(_, _, p)| p.0 == [17, 201, 83, 255])
        .map(|(x, y, _)| (x, y))
        .collect::<Vec<_>>();
    if pixels.len() < 400 {
        return Err("owned target colour absent from actual portal pixels".into());
    }
    let left = pixels.iter().map(|p| p.0).min().ok_or("no left edge")?;
    let right = pixels.iter().map(|p| p.0).max().ok_or("no right edge")?;
    let top = pixels.iter().map(|p| p.1).min().ok_or("no top edge")?;
    let bottom = pixels.iter().map(|p| p.1).max().ok_or("no bottom edge")?;
    if pixels.len() * 10 < ((right - left + 1) * (bottom - top + 1)) as usize * 9 {
        return Err("ambiguous/non-rectangular target colour in portal image".into());
    }
    let x = (left + right) / 2;
    let y = (top + bottom) / 2;
    println!(
        "{}",
        json!({"event":"PORTAL_POINTER_OBSERVATION","observation":frame,"targetBox":[left,top,right,bottom],"imagePoint":[x,y]})
    );
    let _ = std::io::stdout().flush();
    let request = DispatchRequest {
        managed_request: None,
        cancellation: ActionCancellation::default(),
        run_id: run.into(),
        action_id: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        target_id: target.into(),
        target_generation: frame.target_generation,
        snapshot_id: frame.snapshot_id.clone(),
        geometry_revision: frame.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Coord {
            x: f64::from(x),
            y: f64::from(y),
        },
        parameters: json!({"button":"left"}),
        scope: ActionScope::Directed,
    };
    let result = registry.act(&request)?;
    if !result.applied {
        return Err(format!("click not submitted: {result:?}"));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let after = loop {
        let after = receipt(queue).await?;
        if after.0 > prior_clicks {
            break after;
        }
        if Instant::now() >= deadline {
            return Err(format!("native pointer effect missing: {after:?}"));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let expected_edges = [prior_edges, vec![(1, true), (1, false)]].concat();
    if after != (prior_clicks + 1, expected_edges) {
        return Err(format!("unexpected GTK pointer receipt: {after:?}"));
    }
    println!(
        "{}",
        json!({"event":"POINTER_EFFECT","before":before,"after":after,"imagePoint":[x,y],"originalSnapshot":frame.snapshot_id,"nativeGtkEffect":true,"appliedAloneIsNotProof":true})
    );
    let _ = std::io::stdout().flush();
    Ok(request)
}
