//! Exclusive unpublished-directory capability. Never construct one from a path.
use super::profile_io::{node_id, open_node, write_new};
use crate::session_execution::ExecutionSource;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) struct PendingHome {
    home: PathBuf,
    base: PathBuf,
    identity: [u64; 3],
    source: ExecutionSource,
}

fn unavailable() -> String {
    "execution preparation directory changed or is unavailable".into()
}

impl PendingHome {
    pub(super) fn create(owner: &str) -> Result<Self, String> {
        if !uuid::Uuid::parse_str(owner).is_ok_and(|id| id.to_string() == owner) {
            return Err("execution profile owner is invalid".into());
        }
        crate::paths::ensure_app_dirs().map_err(|_| unavailable())?;
        let base = crate::paths::app_data_root()
            .canonicalize()
            .map_err(|_| unavailable())?;
        let mut parent = base.clone();
        for component in ["execution-profiles", owner] {
            parent = parent.join(component);
            match fs::create_dir(&parent) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(unavailable()),
            }
            open_node(&parent, true)?;
            if parent.canonicalize().map_err(|_| unavailable())? != parent {
                return Err(unavailable());
            }
        }
        let generation = uuid::Uuid::new_v4().to_string();
        let actual_home = parent.join(&generation);
        fs::create_dir(&actual_home).map_err(|_| unavailable())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&actual_home, fs::Permissions::from_mode(0o700))
                .map_err(|_| unavailable())?;
        }
        let pending = Self {
            home: crate::paths::app_data_root()
                .join("execution-profiles")
                .join(owner)
                .join(&generation),
            base,
            identity: node_id(&open_node(&actual_home, true)?)?,
            source: ExecutionSource::AppProfile {
                owner_session_id: owner.into(),
                generation,
            },
        };
        pending.check()?;
        Ok(pending)
    }

    pub(super) fn home(&self) -> &Path {
        &self.home
    }

    pub(super) fn source(&self) -> &ExecutionSource {
        &self.source
    }

    pub(super) fn check(&self) -> Result<(), String> {
        if crate::paths::app_data_root()
            .canonicalize()
            .map_err(|_| unavailable())?
            != self.base
            || node_id(&open_node(&self.home, true)?)? != self.identity
        {
            return Err(unavailable());
        }
        let ExecutionSource::AppProfile {
            owner_session_id,
            generation,
        } = &self.source
        else {
            return Err(unavailable());
        };
        if self.home.canonicalize().map_err(|_| unavailable())?
            != self
                .base
                .join("execution-profiles")
                .join(owner_session_id)
                .join(generation)
        {
            return Err(unavailable());
        }
        match fs::symlink_metadata(self.home.join("execution-profile.json")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err("execution profile has already been published".into()),
        }
    }

    pub(super) fn publish(self, expected_config: &[u8]) -> Result<PathBuf, String> {
        self.check()?;
        let config = self.home.join("config.toml");
        open_node(&config, false)?;
        if config.canonicalize().map_err(|_| unavailable())?
            != self
                .home
                .canonicalize()
                .map_err(|_| unavailable())?
                .join("config.toml")
        {
            return Err(unavailable());
        }
        if super::profile_io::read_optional(&config)?.as_deref() != Some(expected_config) {
            return Err("execution configuration changed before publication".into());
        }
        let ExecutionSource::AppProfile {
            owner_session_id,
            generation,
        } = &self.source
        else {
            return Err(unavailable());
        };
        let marker = serde_json::to_vec(&serde_json::json!({
            "schemaVersion":1, "ownerSessionId":owner_session_id, "generation":generation,
            "configurationSha256":format!("{:x}", Sha256::digest(expected_config)),
        }))
        .map_err(|_| unavailable())?;
        let temporary = self
            .home
            .join(format!(".execution-profile-{}", uuid::Uuid::new_v4()));
        write_new(&temporary, &marker)?;
        self.check()?;
        // Linking the synced file publishes a complete marker atomically and
        // fails if a marker appeared. rename could replace it on Unix.
        fs::hard_link(&temporary, self.home.join("execution-profile.json"))
            .map_err(|_| unavailable())?;
        fs::remove_file(temporary).map_err(|_| unavailable())?;
        if self.source.home()?.as_deref() != Some(self.home.as_path()) {
            return Err(unavailable());
        }
        Ok(self.home)
    }
}
