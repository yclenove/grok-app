//! Exercise the original SCM_RIGHTS descriptor, not just its CLOEXEC duplicate.
use super::*;
use std::os::fd::AsRawFd;

struct ExecChild(Child);

impl Drop for ExecChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portal_reply_descriptor_does_not_escape_into_owned_exec_child() {
    let mut observations = Vec::new();
    for (interface, method) in [
        ("org.freedesktop.portal.ScreenCast", "OpenPipeWireRemote"),
        ("org.freedesktop.portal.RemoteDesktop", "ConnectToEIS"),
    ] {
        let fixture = Fixture::new(Mode::Normal).await;
        let connection = zbus::connection::Builder::address(fixture.bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        // This fixture deliberately exposes only the transport; it is not an OS grant.
        let session =
            OwnedObjectPath::try_from("/org/freedesktop/portal/desktop/session/owned").unwrap();
        let reply = connection
            .call_method(
                Some(SERVICE),
                ROOT,
                Some(interface),
                method,
                &(session, Dict::new()),
            )
            .await
            .unwrap();
        assert_eq!(reply.data().fds().len(), 1);
        let raw = reply.data().fds()[0].as_raw_fd();
        let socket = std::fs::read_link(format!("/proc/self/fd/{raw}")).unwrap();
        // SAFETY: the original message retains the descriptor across this query.
        let original_flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
        assert!(original_flags >= 0);
        let duplicate: zbus::zvariant::OwnedFd = reply.body().deserialize().unwrap();
        // SAFETY: duplicate owns this live descriptor.
        let duplicate_flags = unsafe { libc::fcntl(duplicate.as_raw_fd(), libc::F_GETFD) };
        assert!(duplicate_flags >= 0);
        // spawn() completes the exec handshake. cat remains alive on our stdin pipe;
        // unlike a Python subprocess wrapper it does not close unrelated descriptors.
        let mut child = ExecChild(
            owned_child(
                Command::new("/bin/cat")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null()),
            )
            .spawn()
            .unwrap(),
        );
        let child_id = child.0.id();
        let inherited: Vec<_> = std::fs::read_dir(format!("/proc/{child_id}/fd"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| std::fs::read_link(path).is_ok_and(|target| target == socket))
            .collect();
        drop(duplicate);
        drop(reply);
        let before_join = fixture.shared.peers.lock().unwrap()[0].read(&mut [0u8]);
        let child_was_live = child.0.try_wait().unwrap().is_none();
        child.0.kill().unwrap();
        let status = child.0.wait().unwrap();
        let after_join = fixture.shared.peers.lock().unwrap()[0].read(&mut [0u8]);
        let evidence = serde_json::json!({
            "method": method, "originalFlags": original_flags,
            "duplicateFlags": duplicate_flags, "socket": socket,
            "originalChildPid": child_id, "inheritedDescriptors": inherited,
            "childWasLiveBeforeJoin": child_was_live,
            "beforeJoinRead": format!("{before_join:?}"),
            "afterJoinRead": format!("{after_join:?}"),
            "originalChildJoined": true, "childStatus": status.to_string(),
        });
        println!("SCM_RIGHTS_EXEC_EVIDENCE {evidence}");
        observations.push((
            original_flags & libc::FD_CLOEXEC != 0
                && duplicate_flags & libc::FD_CLOEXEC != 0
                && inherited.is_empty()
                && child_was_live
                && matches!(before_join, Ok(0))
                && matches!(after_join, Ok(0)),
            evidence,
        ));
    }
    assert!(
        observations.iter().all(|(passed, _)| *passed),
        "original D-Bus transport escaped exec or blocked retirement: {observations:?}"
    );
}
