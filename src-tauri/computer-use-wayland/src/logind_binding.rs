//! login1's user-manager association, ONLY after its exact NoSessionForPID.
//! This is not a UID/environment fallback: the PID must resolve to the pinned
//! User, whose Display must be its only local graphical session. No rebind.
use super::{all, boolean, error, initial_session, string, Properties, MANAGER, ROOT, SESSION};
use std::collections::{HashMap, HashSet};
use zbus::{zvariant::OwnedObjectPath, Connection, Proxy};

pub(super) const USER: &str = "org.freedesktop.login1.User";
type SessionRef = (String, OwnedObjectPath);

#[derive(Clone)]
pub(super) struct UserBinding {
    pub path: OwnedObjectPath,
    pub display: SessionRef,
    pub sessions: HashMap<OwnedObjectPath, String>,
    pub pids: Vec<u32>,
}

pub(super) fn concrete(path: &str, kind: &str) -> Result<(), String> {
    let prefix = format!("{ROOT}/{kind}/");
    let suffix = path.strip_prefix(&prefix).unwrap_or("");
    if suffix.is_empty() || suffix.contains('/') || matches!(suffix, "self" | "auto") {
        return Err(error(format!(
            "{kind} must have a concrete login1 object path"
        )));
    }
    Ok(())
}

fn no_session(e: &zbus::Error) -> bool {
    matches!(e, zbus::Error::MethodError(name, _, _)
        if name.as_str() == "org.freedesktop.login1.NoSessionForPID")
}

fn property<T>(p: &Properties, key: &str) -> Result<T, String>
where
    T: TryFrom<zbus::zvariant::OwnedValue>,
    T::Error: std::fmt::Display,
{
    p.get(key)
        .ok_or_else(|| error(format!("missing {key}")))?
        .try_clone()
        .map_err(error)?
        .try_into()
        .map_err(error)
}

pub(super) fn session_refs(p: &Properties, key: &str) -> Result<Vec<SessionRef>, String> {
    let refs: Vec<SessionRef> = property(p, key)?;
    if refs.is_empty() || refs.len() > 64 {
        return Err(error("empty or oversized user session inventory"));
    }
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    for (id, path) in &refs {
        concrete(path.as_str(), "session")?;
        if id.is_empty() || !ids.insert(id) || !paths.insert(path) {
            return Err(error("invalid or duplicate user session identity"));
        }
    }
    Ok(refs)
}

fn display(p: &Properties) -> Result<SessionRef, String> {
    let display: SessionRef = property(p, "Display")?;
    concrete(display.1.as_str(), "session")?;
    if display.0.is_empty() {
        return Err(error("user has no graphical Display"));
    }
    Ok(display)
}

async fn user_snapshot(
    c: &Connection,
    owner: &str,
    uid: u32,
    user: &OwnedObjectPath,
) -> Result<(SessionRef, Vec<SessionRef>), String> {
    let p = all(c, owner, user.as_str(), USER).await?;
    if property::<u32>(&p, "UID")? != uid {
        return Err(error("user-manager UID mismatch"));
    }
    let display = display(&p)?;
    let refs = session_refs(&p, "Sessions")?;
    if !refs.contains(&display) {
        return Err(error("Display is absent from the user's session inventory"));
    }
    Ok((display, refs))
}

fn local_graphical(p: &Properties) -> Result<bool, String> {
    Ok(!boolean(p, "Remote")?
        && string(p, "Class")? == "user"
        && matches!(string(p, "Type")?, "wayland" | "x11"))
}

async fn snapshot(
    c: &Connection,
    owner: &str,
    uid: u32,
    user: OwnedObjectPath,
    pids: Vec<u32>,
) -> Result<UserBinding, String> {
    concrete(user.as_str(), "user")?;
    let manager = Proxy::new(c, owner, ROOT, MANAGER).await.map_err(error)?;
    let expected: OwnedObjectPath = manager.call("GetUser", &(uid,)).await.map_err(error)?;
    if expected != user {
        return Err(error(
            "PID user-manager object differs from its authenticated UID",
        ));
    }
    let (display, refs) = user_snapshot(c, owner, uid, &user).await?;
    let mut graphical = 0;
    for (id, path) in &refs {
        let p = all(c, owner, path.as_str(), SESSION).await?;
        let actual_user: (u32, OwnedObjectPath) = property(&p, "User")?;
        if actual_user != (uid, user.clone()) || string(&p, "Id")? != id {
            return Err(error(
                "user inventory contains a foreign or mismatched session",
            ));
        }
        if local_graphical(&p)? {
            graphical += 1;
            if path != &display.1 {
                return Err(error("ambiguous graphical user-manager association"));
            }
        }
        if path == &display.1 {
            initial_session(&p, uid)?;
        }
    }
    if graphical != 1 {
        return Err(error(
            "user-manager requires exactly one local graphical Display",
        ));
    }
    // A snapshot changing while we inspect it is not permission to guess.
    let (after_display, mut after_refs) = user_snapshot(c, owner, uid, &user).await?;
    let mut before_refs = refs.clone();
    before_refs.sort_by(|a, b| (&a.0, a.1.as_str()).cmp(&(&b.0, b.1.as_str())));
    after_refs.sort_by(|a, b| (&a.0, a.1.as_str()).cmp(&(&b.0, b.1.as_str())));
    if after_display != display || after_refs != before_refs {
        return Err(error("user-manager association changed during resolution"));
    }
    Ok(UserBinding {
        path: user,
        display,
        sessions: refs.into_iter().map(|(id, path)| (path, id)).collect(),
        pids,
    })
}

pub(super) async fn resolve(
    c: &Connection,
    owner: &str,
    pid: u32,
    uid: u32,
) -> Result<(OwnedObjectPath, Option<UserBinding>), String> {
    if pid == 0 {
        return Err(error("invalid PID"));
    }
    let manager = Proxy::new(c, owner, ROOT, MANAGER).await.map_err(error)?;
    let result: zbus::Result<OwnedObjectPath> = manager.call("GetSessionByPID", &(pid,)).await;
    match result {
        Ok(path) => {
            concrete(path.as_str(), "session")?;
            // An actual session is authoritative: NEVER fall back. The caller
            // checks equality (for peers) and all selected safety properties.
            Ok((path, None))
        }
        Err(e) if no_session(&e) => {
            let user = manager.call("GetUserByPID", &(pid,)).await.map_err(error)?;
            let binding = snapshot(c, owner, uid, user, vec![pid]).await?;
            Ok((binding.display.1.clone(), Some(binding)))
        }
        Err(e) => Err(error(e)),
    }
}

/// Inspect signal-carried identities as well as current inventory. A short-lived
/// second display must not disappear behind a later positive GetAll snapshot.
pub(super) async fn refresh(
    c: &Connection,
    owner: &str,
    uid: u32,
    previous: &UserBinding,
    inspect: &[SessionRef],
) -> Result<UserBinding, String> {
    for (id, path) in inspect {
        concrete(path.as_str(), "session")?;
        let p = all(c, owner, path.as_str(), SESSION).await?;
        if !id.is_empty() && string(&p, "Id")? != id {
            return Err(error("session signal identity mismatch"));
        }
        let (session_uid, _): (u32, OwnedObjectPath) = property(&p, "User")?;
        if session_uid == uid && local_graphical(&p)? && path != &previous.display.1 {
            return Err(error("another local graphical session appeared"));
        }
    }
    let manager = Proxy::new(c, owner, ROOT, MANAGER).await.map_err(error)?;
    for pid in &previous.pids {
        let user: OwnedObjectPath = manager.call("GetUserByPID", &(pid,)).await.map_err(error)?;
        if user != previous.path {
            return Err(error("PID user-manager association changed"));
        }
        let session: zbus::Result<OwnedObjectPath> = manager.call("GetSessionByPID", &(pid,)).await;
        match session {
            Ok(path) if path == previous.display.1 => {}
            Err(e) if no_session(&e) => {}
            _ => return Err(error("PID moved to a different login session")),
        }
    }
    let next = snapshot(c, owner, uid, previous.path.clone(), previous.pids.clone()).await?;
    if next.display != previous.display {
        return Err(error(
            "user-manager Display changed; automatic rebind forbidden",
        ));
    }
    Ok(next)
}

impl UserBinding {
    pub fn check_user_change(
        &self,
        changed: &Properties,
        invalidated: &[String],
    ) -> Result<Vec<SessionRef>, String> {
        if invalidated
            .iter()
            .any(|k| ["UID", "Display", "Sessions"].contains(&k.as_str()))
            || changed.contains_key("UID")
        {
            return Err(error("user-manager safety property changed or invalidated"));
        }
        if changed.contains_key("Display") && display(changed)? != self.display {
            return Err(error("user-manager Display changed"));
        }
        if changed.contains_key("Sessions") {
            let refs = session_refs(changed, "Sessions")?;
            if !refs.contains(&self.display) {
                return Err(error("Display removed from user-manager inventory"));
            }
            return Ok(refs
                .into_iter()
                .filter(|(id, path)| self.sessions.get(path) != Some(id))
                .collect());
        }
        Ok(Vec::new())
    }
}
