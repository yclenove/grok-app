//! Host protocol security regression tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_host_protocol::{
    codec::{read_frame, write_frame, MAX_FRAME},
    messages::*,
};
use tokio::io::{duplex, AsyncWriteExt};

#[tokio::test]
async fn codec_valid_frame_roundtrips_with_backpressure() {
    let (mut a, mut b) = duplex(8);
    let expected = serde_json::json!({"payload":"x".repeat(2000)});
    let sent = expected.clone();
    let writer = tokio::spawn(async move { write_frame(&mut a, &sent).await.unwrap() });
    assert_eq!(
        read_frame::<_, serde_json::Value>(&mut b).await.unwrap(),
        Some(expected)
    );
    writer.await.unwrap();
}
#[tokio::test]
async fn codec_invalid_truncated_and_oversized_frames_return_errors() {
    for bytes in [
        vec![0, 0],
        vec![0, 0, 0, 3, b'{'],
        vec![0, 0, 0, 1, 0xff],
        ((MAX_FRAME + 1) as u32).to_be_bytes().to_vec(),
        vec![0, 0, 0, 0],
    ] {
        let (mut a, mut b) = duplex(32);
        a.write_all(&bytes).await.unwrap();
        drop(a);
        assert!(read_frame::<_, serde_json::Value>(&mut b).await.is_err());
    }
    let (a, mut b) = duplex(1);
    drop(a);
    assert!(read_frame::<_, serde_json::Value>(&mut b)
        .await
        .unwrap()
        .is_none());
}
#[test]
fn methods_strict_table_has_no_persistent_grant_write() {
    assert!(!METHODS.iter().any(|m| m.name == "host.policy.grantAlways"));
    for m in METHODS {
        assert_eq!(m.name.split('.').count(), 3);
    }
    assert!(serde_json::from_value::<SessionOpen>(
        serde_json::json!({"session_id":"s","kind":"local","extra":true})
    )
    .is_err());
}
#[test]
fn codec_envelope_rejects_batches_unknown_fields_and_non_rpc_versions() {
    use gcu_host_protocol::codec::validate;
    for value in [
        serde_json::json!([]),
        serde_json::json!({"jsonrpc":"1.0","id":1,"result":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":0,"result":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":1,"result":{},"error":{"code":0,"message":"x"}}),
    ] {
        let body = serde_json::to_vec(&value).unwrap();
        let mut frame = (body.len() as u32).to_be_bytes().to_vec();
        frame.extend(body);
        assert!(validate(&frame).is_err());
    }
    let body = br#"{"jsonrpc":"2.0","id":1,"result":null}"#;
    let mut frame = (body.len() as u32).to_be_bytes().to_vec();
    frame.extend(body);
    assert!(validate(&frame).is_ok());
    frame.push(0);
    assert!(validate(&frame).is_err());
}
#[test]
fn schema_golden_is_generated_from_the_exact_strict_wire_types() {
    let actual = schemas();
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../golden/messages.json")).unwrap();
    assert_eq!(actual, golden);
    for method in METHODS {
        let schema = &actual["methods"][method.name]["params"];
        assert_eq!(schema["additionalProperties"], false);
        let value = serde_json::json!({"jsonrpc":"2.0","id":1,"method":method.name,"params":{}});
        let request: Request = serde_json::from_value(value).unwrap();
        let _ = validate_params(&request);
    }
}
