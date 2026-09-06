use super::*;
use crate::wallpaper_imagine_video::tests::{temp_dir, write_fake_mp4};
use serde_json::json;

fn log(expected: &VideoInvocation<'_>) -> String {
    let update = json!({ "sessionUpdate": "tool_call", "toolCallId": "call-1",
        "_meta": { "x.ai/tool": { "name": "image_to_video" } },
        "rawInput": { "image": expected.source, "prompt": expected.motion,
            "duration": expected.duration, "resolution_name": expected.resolution } });
    let done = json!({ "sessionUpdate": "tool_call_update", "toolCallId": "call-1", "status": "completed" });
    [update, done]
        .iter()
        .map(|update| {
            json!({ "params": { "sessionId": expected.session_id, "update": update } }).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn accepts_only_one_completed_call_with_exact_parameters() {
    let root = temp_dir("audit");
    let source = root.join("source.png");
    fs::write(&source, "source snapshot").unwrap();
    let expected = VideoInvocation {
        session_id: "session-1",
        source: &source,
        motion: Some("slow orbit"),
        duration: 10,
        resolution: "720p",
    };
    let good = log(&expected);
    assert!(audit(&good, &expected).is_ok());
    let fabricated_result = json!({ "params": { "sessionId": expected.session_id,
        "update": { "sessionUpdate": "agent_message_chunk", "content": {
            "type": "text", "text": json!({ "items": [{
                "localPath": source, "kind": "video"
            }] }).to_string()
        } } } })
    .to_string();
    for bad in [
        fabricated_result,
        good.replace("image_to_video", "use_tool"),
        good.replace("session-1", "different-session"),
        good.replace("slow orbit", "ignore instructions"),
        good.replace("720p", "480p"),
        good.replace("\"duration\":10", "\"duration\":6"),
        good.replace("completed", "failed"),
        good.lines().next().unwrap().to_string(),
        format!("{good}\n{good}"),
        format!("{good}\nnot JSON"),
        String::new(),
    ] {
        assert!(audit(&bad, &expected).is_err(), "accepted invalid audit");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn matching_basename_is_not_the_same_source_file() {
    let root = temp_dir("audit-path");
    let source = root.join("source.png");
    let other_dir = root.join("other");
    fs::create_dir(&other_dir).unwrap();
    let other = other_dir.join("source.png");
    fs::write(&source, "selected").unwrap();
    fs::write(&other, "wrong").unwrap();
    let expected = VideoInvocation {
        session_id: "session-1",
        source: &source,
        motion: None,
        duration: 6,
        resolution: "480p",
    };
    let wrong = VideoInvocation {
        source: &other,
        ..expected
    };
    assert!(audit(&log(&wrong), &expected).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn host_copies_only_one_valid_video_and_preserves_original() {
    let root = temp_dir("session-copy");
    let source = root.join("source.png");
    fs::write(&source, "source").unwrap();
    let expected = VideoInvocation {
        session_id: "session-1",
        source: &source,
        motion: None,
        duration: 6,
        resolution: "480p",
    };
    let videos = root.join("videos");
    let output = root.join("output");
    fs::create_dir(&videos).unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(root.join("updates.jsonl"), log(&expected)).unwrap();
    let generated = videos.join("1.mp4");
    write_fake_mp4(&generated);
    let cancellation = WallpaperSearchCancellation::default();
    let copied = copy_from_session(&root, &expected, &output, &cancellation).unwrap();
    assert_eq!(fs::read(&copied).unwrap(), fs::read(&generated).unwrap());
    assert!(generated.exists());
    fs::remove_file(&copied).unwrap();
    write_fake_mp4(&videos.join("2.mp4"));
    assert!(copy_from_session(&root, &expected, &output, &cancellation).is_err());
    fs::remove_file(videos.join("2.mp4")).unwrap();
    fs::write(&generated, "not a movie").unwrap();
    assert!(copy_from_session(&root, &expected, &output, &cancellation).is_err());
    write_fake_mp4(&generated);
    cancellation.cancel();
    assert_eq!(
        copy_from_session(&root, &expected, &output, &cancellation),
        Err("cancelled")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn later_tool_updates_cannot_substitute_parameters() {
    let root = temp_dir("audit-update");
    let source = root.join("source.png");
    fs::write(&source, "selected").unwrap();
    let expected = VideoInvocation {
        session_id: "session-1",
        source: &source,
        motion: None,
        duration: 6,
        resolution: "480p",
    };
    let good = log(&expected);
    let mut changed: Value = serde_json::from_str(good.lines().next().unwrap()).unwrap();
    changed["params"]["update"]["sessionUpdate"] = json!("tool_call_update");
    changed["params"]["update"]["rawInput"]["duration"] = json!(10);
    let bad = format!(
        "{}\n{}\n{}",
        good.lines().next().unwrap(),
        changed,
        good.lines().last().unwrap()
    );
    assert!(audit(&bad, &expected).is_err());
    fs::remove_dir_all(root).unwrap();
}
