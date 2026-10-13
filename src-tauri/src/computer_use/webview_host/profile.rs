//! New, probe-owned profiles; shared views never open a caller's existing profile.

use super::*;
use std::path::{Path, PathBuf};

pub(super) struct Profile {
    pub(super) path: PathBuf,
    pub(super) wait_for_exit: bool,
}

impl HostOwnedWebView {
    pub fn spawn(label: &str, tab_id: &str, url: &str) -> Result<Arc<Self>, String> {
        Self::spawn_with_profile(label, tab_id, url, None)
    }

    pub(crate) fn spawn_disposable(
        label: &str,
        tab_id: &str,
        url: &str,
        profile: &Path,
    ) -> Result<Arc<Self>, String> {
        std::fs::create_dir(profile).map_err(|_| "disposable WebView profile must be new")?;
        Self::spawn_with_profile(
            label,
            tab_id,
            url,
            Some(Profile {
                path: profile.to_owned(),
                wait_for_exit: true,
            }),
        )
    }

    fn spawn_with_profile(
        label: &str,
        tab_id: &str,
        url: &str,
        profile: Option<Profile>,
    ) -> Result<Arc<Self>, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let document = HostDocumentState::new();
        let document_t = document.clone();
        let startup = startup::Startup::new(document.alive.clone());
        let startup_t = startup.clone();
        let url = url.to_string();
        let join = thread::spawn(move || {
            sta_thread(url, cmd_rx, ready_tx, document_t, profile, startup_t)
        });
        match ready_rx.recv_timeout(Duration::from_secs(25)) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                startup.stop();
                return Err(error);
            }
            Err(_) => return Err(startup.stop()),
        }
        Ok(Arc::new(Self {
            label: label.into(),
            tab_id: tab_id.into(),
            cmds: cmd_tx,
            document,
            join: shutdown::NativeThread::new(join),
        }))
    }
}

pub(crate) struct SharedBrowser {
    pub(crate) keeper: Arc<HostOwnedWebView>,
    profile: PathBuf,
}

impl SharedBrowser {
    pub(crate) fn spawn(profile: &Path, url: &str) -> Result<Self, String> {
        let keeper =
            HostOwnedWebView::spawn_disposable("resource-browser-dl-keep", "keep", url, profile)?;
        Ok(Self {
            keeper,
            profile: profile.to_owned(),
        })
    }

    pub(crate) fn view(&self, tab: &str, url: &str) -> Result<Arc<HostOwnedWebView>, String> {
        HostOwnedWebView::spawn_with_profile(
            "resource-browser-dl-shared",
            tab,
            url,
            Some(Profile {
                path: self.profile.clone(),
                wait_for_exit: false,
            }),
        )
    }
}
