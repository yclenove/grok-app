//! Host protocol security regression tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_host_protocol::auth::*;
use tokio::io::duplex;

#[tokio::test]
async fn handshake_mutual_proofs_authenticate_expected_instances() {
    let (mut a, mut b) = duplex(8192);
    let config = AuthConfig::current(Role::Host, Channel::Rpc).unwrap();
    let mut engine = config.clone();
    engine.role = Role::Engine;
    let peer = config.instance.clone();
    let token = Secret::random();
    let other = token.clone();
    let host = tokio::spawn(async move { authenticate(&mut a, &config, &peer, &token).await });
    let expected = engine.instance.clone();
    assert!(authenticate(&mut b, &engine, &expected, &other)
        .await
        .is_ok());
    assert!(host.await.unwrap().is_ok());
}
#[tokio::test]
async fn handshake_wrong_token_or_major_or_process_or_channel_is_rejected() {
    for case in 0..4 {
        let (mut a, mut b) = duplex(8192);
        let host = AuthConfig::current(Role::Host, Channel::Rpc).unwrap();
        let mut engine = host.clone();
        engine.role = Role::Engine;
        let mut expected = host.instance.clone();
        if case == 1 {
            engine.version.major += 1;
        }
        if case == 2 {
            expected.started += 1;
        }
        if case == 3 {
            engine.channel = Channel::Stop;
        }
        let secret = Secret::random();
        let other = if case == 0 {
            Secret::random()
        } else {
            secret.clone()
        };
        let hp = host.instance.clone();
        let task = tokio::spawn(async move { authenticate(&mut a, &host, &hp, &secret).await });
        assert!(authenticate(&mut b, &engine, &expected, &other)
            .await
            .is_err());
        assert!(task.await.unwrap().is_err());
    }
}
#[tokio::test]
async fn handshake_launch_secret_is_consumed_once_per_role_and_channel() {
    let config = AuthConfig::current(Role::Host, Channel::Rpc).unwrap();
    let expected = config.instance.clone();
    let secret = Secret::random();
    let other = secret.clone();
    let mut engine = config.clone();
    engine.role = Role::Engine;
    let (mut a, mut b) = duplex(8192);
    let hp = expected.clone();
    let hc = config.clone();
    let task = tokio::spawn(async move { authenticate(&mut a, &hc, &hp, &other).await });
    assert!(authenticate(&mut b, &engine, &expected, &secret)
        .await
        .is_ok());
    assert!(task.await.unwrap().is_ok());
    let (mut a, _) = duplex(8192);
    assert!(matches!(
        authenticate(&mut a, &config, &expected, &secret).await,
        Err(gcu_host_protocol::Error::Authentication)
    ));
}
