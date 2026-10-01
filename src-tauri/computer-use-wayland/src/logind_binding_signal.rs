//! Inventory signals supplement (never replace) the selected session's fence.
use super::{binding, error, Properties, MANAGER, PROPERTIES, ROOT, SESSION};
use zbus::{zvariant::OwnedObjectPath, Message};

impl binding::UserBinding {
    pub(super) fn signal(
        &self,
        message: &Message,
        uid: u32,
    ) -> Result<Option<Vec<(String, OwnedObjectPath)>>, String> {
        let h = message.header();
        let path = h.path().map(|v| v.as_str());
        let interface = h.interface().map(|v| v.as_str());
        let member = h.member().map(|v| v.as_str());
        if interface == Some(PROPERTIES) && member == Some("PropertiesChanged") {
            let (interface, changed, invalidated): (String, Properties, Vec<String>) =
                message.body().deserialize().map_err(error)?;
            if path == Some(self.path.as_str()) && interface == binding::USER {
                let inspect = self.check_user_change(&changed, &invalidated)?;
                if changed.contains_key("Sessions") {
                    return Ok(Some(inspect));
                }
            }
            if interface == SESSION && path != Some(self.display.1.as_str()) {
                let keys = ["Type", "Class", "User", "Remote", "Id"];
                if keys.iter().any(|k| changed.contains_key(*k))
                    || invalidated.iter().any(|k| keys.contains(&k.as_str()))
                {
                    let path: OwnedObjectPath = path
                        .ok_or_else(|| error("session signal without path"))?
                        .try_into()
                        .map_err(error)?;
                    if self.sessions.contains_key(&path) {
                        // Identity-change pulses are loss, not a positive reread.
                        return Err(error("user session inventory identity changed"));
                    }
                    return Ok(Some(vec![(String::new(), path)]));
                }
            }
        }
        if path == Some(ROOT) && interface == Some(MANAGER) {
            match member {
                Some("SessionNew") => {
                    let new: (String, OwnedObjectPath) =
                        message.body().deserialize().map_err(error)?;
                    if new.1 == self.display.1 {
                        return Err(error("selected session was replaced"));
                    }
                    return Ok(Some(vec![new]));
                }
                Some("SessionRemoved") => {
                    let (_, removed): (String, OwnedObjectPath) =
                        message.body().deserialize().map_err(error)?;
                    if self.sessions.contains_key(&removed) {
                        return Ok(Some(Vec::new()));
                    }
                }
                Some("UserNew" | "UserRemoved") => {
                    let (changed_uid, user): (u32, OwnedObjectPath) =
                        message.body().deserialize().map_err(error)?;
                    if changed_uid == uid || user == self.path {
                        return Err(error("user-manager object lost or replaced"));
                    }
                }
                _ => {}
            }
        }
        Ok(None)
    }
}
