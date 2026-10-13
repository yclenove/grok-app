//! Exact instance witnesses supplied by trusted adapters, never model JSON.
//! New backends must prove their own membership; liveness alone is insufficient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeScopeIdentity {
    WindowsRoot {
        process_id: u32,
        process_birth: u64,
        root_hwnd: usize,
        window_stamp: u64,
    },
    ManagedPage {
        identity: crate::browser::ManagedScopeIdentity,
    },
    ExistingPage {
        identity: Box<crate::browser::ExistingScopeIdentity>,
    },
    WebViewSelected {
        identity: super::WebViewSelectedIdentity,
    },
    /// An execution binding retained by the App Host, never a page label or
    /// pre-approval picker candidate. The executor pins the original object.
    WebViewPage {
        target_id: String,
        native_tab_id: String,
        document_generation: u64,
        session_id: String,
        run_id: String,
    },
    #[cfg(any(test, feature = "test-support"))]
    Synthetic { target_id: String, instance: u64 },
}

impl NativeScopeIdentity {
    pub(crate) fn target_id(&self) -> Option<String> {
        match self {
            Self::ManagedPage { identity } => identity.target_id(),
            Self::ExistingPage { identity } => Some(identity.target_id()),
            Self::WebViewSelected { identity }
                if identity.target_id.starts_with("wv:selection:")
                    && identity.target_id.len() <= 128
                    && !identity.native_tab_id.is_empty()
                    && identity.document_generation != 0 =>
            {
                Some(identity.target_id.clone())
            }
            Self::WebViewSelected { .. } => None,
            Self::WebViewPage {
                target_id,
                native_tab_id,
                document_generation,
                session_id,
                run_id,
            } if target_id.starts_with("wv|")
                && target_id.len() <= 256
                && !native_tab_id.is_empty()
                && *document_generation != 0
                && !session_id.is_empty()
                && !run_id.is_empty() =>
            {
                Some(target_id.clone())
            }
            Self::WebViewPage { .. } => None,
            Self::WindowsRoot {
                process_id,
                process_birth,
                root_hwnd,
                window_stamp,
            } if *process_id != 0
                && *process_birth != 0
                && *root_hwnd != 0
                && *window_stamp != 0 =>
            {
                Some(format!(
                    "win:{process_id}:{root_hwnd:x}:{window_stamp}:{process_birth}"
                ))
            }
            Self::WindowsRoot { .. } => None,
            #[cfg(any(test, feature = "test-support"))]
            Self::Synthetic {
                target_id,
                instance,
            } if !target_id.is_empty() && *instance != 0 => Some(target_id.clone()),
            #[cfg(any(test, feature = "test-support"))]
            Self::Synthetic { .. } => None,
        }
    }

    pub(crate) fn decision_class(&self) -> &'static str {
        match self {
            Self::ManagedPage { .. } => "host-native-scope",
            Self::ExistingPage { .. } => "host-native-scope",
            Self::WebViewPage { .. } => "host-native-scope",
            Self::WebViewSelected { .. } => "host-native-scope",
            Self::WindowsRoot { .. } => "host-native-scope",
            #[cfg(any(test, feature = "test-support"))]
            Self::Synthetic { .. } => crate::task_consent::SYNTHETIC_DECISION_CLASS,
        }
    }
}
