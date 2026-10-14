//! Fallback endpoint pinning and anti-preemption contracts.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(unix)]
mod unix {
    use gcu_host_protocol::{
        auth::{ProcessInstance, Secret},
        transport::*,
    };
    use std::{os::unix::fs::PermissionsExt, path::PathBuf};
    struct Private(PathBuf);
    impl Private {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("gcu-ht-{}", &Secret::random().hex()[..8]));
            std::fs::create_dir(&p).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self(p)
        }
    }
    impl Drop for Private {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[tokio::test]
    async fn fallback_known_endpoint_accepts_only_pinned_live_instance() {
        let dir = Private::new();
        let path = dir.0.join("host.sock");
        let listener = PrivateListener::bind(&path).unwrap();
        let pin = listener.stamp();
        let instance = ProcessInstance::current().unwrap();
        let (accepted, connected) =
            tokio::join!(listener.accept(&instance), connect(&path, &pin, &instance));
        assert!(accepted.is_ok());
        assert!(connected.is_ok());
        drop(listener);
        assert!(!path.exists());
    }
    #[tokio::test]
    async fn fallback_unknown_precreated_replaced_endpoints_and_insecure_directory_are_rejected() {
        let dir = Private::new();
        let path = dir.0.join("host.sock");
        std::fs::write(&path, "unknown").unwrap();
        assert!(PrivateListener::bind(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"unknown");
        std::fs::remove_file(&path).unwrap();
        let listener = PrivateListener::bind(&path).unwrap();
        assert!(PrivateListener::bind(&path).is_err());
        let pin = listener.stamp();
        std::fs::remove_file(&path).unwrap();
        let replacement = PrivateListener::bind(&path).unwrap();
        assert!(pin.check(&path).is_err());
        let instance = ProcessInstance::current().unwrap();
        assert!(connect(&path, &pin, &instance).await.is_err());
        drop(listener);
        assert!(path.exists());
        drop(replacement);
        std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(PrivateListener::bind(&path).is_err());
    }
    #[tokio::test]
    async fn fallback_same_uid_impersonator_with_wrong_process_instance_is_rejected() {
        let dir = Private::new();
        let path = dir.0.join("host.sock");
        let listener = PrivateListener::bind(&path).unwrap();
        let pin = listener.stamp();
        let mut expected = ProcessInstance::current().unwrap();
        expected.started += 1;
        let (_, connected) =
            tokio::join!(listener.accept(&expected), connect(&path, &pin, &expected));
        assert!(connected.is_err());
    }
    #[tokio::test]
    async fn inherited_invalid_descriptor_returns_error_without_panicking() {
        assert!(inherited(usize::MAX).is_err());
        assert!(inherited(i32::MAX as usize).is_err());
        assert!(token_fd(i32::MAX as usize).await.is_err());
        assert!(bootstrap_fd(i32::MAX as usize).await.is_err());
    }
}
#[cfg(unix)]
#[tokio::test]
async fn secret_fd_bootstrap_and_raw_token_reads_cancel_and_reject_truncation() {
    use gcu_host_protocol::{auth::*, codec::write_frame, transport::*};
    use std::os::fd::AsRawFd;
    use tokio::io::AsyncWriteExt;
    let (writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    writer.set_nonblocking(true).unwrap();
    let mut writer = tokio::net::UnixStream::from_std(writer).unwrap();
    let secret = Secret::random();
    let original =
        Bootstrap::new(ProcessInstance::current().unwrap(), &secret).with_endpoint(EndpointStamp {
            directory_device: 1,
            directory_inode: 2,
            device: 3,
            inode: 4,
        });
    write_frame(&mut writer, &original).await.unwrap();
    let bootstrap = bootstrap_fd(reader.as_raw_fd() as usize).await.unwrap();
    assert_eq!(bootstrap.secret().hex(), secret.hex());
    assert_eq!(bootstrap.endpoint.unwrap().inode, 4);
    writer.write_all(&[3; 32]).await.unwrap();
    assert_eq!(
        token_fd(reader.as_raw_fd() as usize).await.unwrap(),
        [3; 32]
    );
    writer.write_all(&[4; 3]).await.unwrap();
    drop(writer);
    assert!(token_fd(reader.as_raw_fd() as usize).await.is_err());
    let (writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(10),
        token_fd(reader.as_raw_fd() as usize)
    )
    .await
    .is_err());
    drop(writer);
    let (writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(writer);
    assert!(bootstrap_fd(reader.as_raw_fd() as usize).await.is_err());
}
#[cfg(windows)]
#[tokio::test]
async fn named_pipe_local_current_user_endpoint_rejects_preemption_and_wrong_process() {
    use gcu_host_protocol::{
        auth::{ProcessInstance, Secret},
        transport::*,
    };
    let path = std::path::PathBuf::from(format!("gcu-ht-{}", &Secret::random().hex()[..8]));
    let listener = PrivateListener::bind(&path).unwrap();
    assert!(PrivateListener::bind(&path).is_err());
    let expected = ProcessInstance::current().unwrap();
    let stamp = listener.stamp();
    let (a, b) = tokio::join!(
        listener.accept(&expected),
        connect(&path, &stamp, &expected)
    );
    assert!(a.is_ok());
    assert!(b.is_ok());
    let mut wrong = expected;
    wrong.started += 1;
    assert!(connect(&path, &listener.stamp(), &wrong).await.is_err());
}
