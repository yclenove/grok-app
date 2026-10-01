# zbus 5.18.0: Linux atomic SCM_RIGHTS close-on-exec

Upstream package: `zbus` 5.18.0, unmodified crates.io archive SHA-256
`fe18fb60dc696039e738717b76eaea21e7a4489bbb1885020b43c94236d7e98a`.
The complete published package and its license are retained. The package was
copied from the pinned, checksum-verified Cargo cache, not edited in that cache.

The only upstream source change is in `src/connection/socket/unix.rs`:
on Linux, `fd_recvmsg` supplies `RecvFlags::CMSG_CLOEXEC` instead of empty flags.
Both the async-io and Tokio Unix readers call this function. Other platforms
retain their upstream behavior; this patch makes no non-Linux protection claim.

Why this belongs at reception: the original ancillary descriptor remains owned
by the D-Bus message. Deserializing an `OwnedFd` creates a separate close-on-exec
duplicate. Marking that duplicate in the transport validator neither closes nor
protects the original, which an unrelated concurrent child can inherit. Using
`fcntl` after reception would also leave an exec race. The Linux receive flag
sets close-on-exec atomically on each received descriptor, before publication.

Regression: `computer-use-wayland/src/tests/fd_inheritance.rs` receives actual
PipeWire/EIS-shaped socket descriptors through a private D-Bus, retains the raw
reply across an owned `/bin/cat` exec, inspects the child's descriptors, then
requires peer EOF while that child is still alive. It also verifies the
original descriptor flags and joins the exact child on all paths. Before this
patch, both original descriptors had flags 0, both children inherited the
socket, and both EOF checks blocked until the children exited; the decoded
duplicates already had CLOEXEC. This is a transport regression, not a real
desktop authorization test.

Original failed parallel-suite evidence and deterministic pre-fix evidence are
preserved in `.run/gnome-owner-lifetime-20261001/` under the computer-use probe.
Do not remove this patch merely because a serial suite passes. Removal requires
an upstream receive implementation with equivalent atomic semantics and the
same original-descriptor/exec/retirement regression passing without this patch.
