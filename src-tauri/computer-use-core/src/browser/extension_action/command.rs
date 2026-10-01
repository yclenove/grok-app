use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionName {
    Click,
    SetValue,
    TypeText,
    Scroll,
    Wait,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActKind {
    Act,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeftButton {
    #[default]
    Left,
}

fn one() -> u8 {
    1
}
fn wait_timeout() -> u64 {
    2000
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClickParameters {
    #[serde(default)]
    pub button: LeftButton,
    #[serde(default = "one")]
    pub count: u8,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextParameters {
    pub text: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrollParameters {
    pub delta: i32,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WaitParameters {
    pub name_equals: String,
    #[serde(default = "wait_timeout")]
    pub timeout_ms: u64,
}

// Explicit fields per variant avoid flatten + deny_unknown_fields ambiguities.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", deny_unknown_fields, rename_all = "snake_case")]
pub enum ExtensionActionCommand {
    Click {
        kind: ActKind,
        #[serde(rename = "snapshotId")]
        snapshot_id: String,
        #[serde(rename = "elementRef")]
        element_ref: String,
        parameters: ClickParameters,
    },
    SetValue {
        kind: ActKind,
        #[serde(rename = "snapshotId")]
        snapshot_id: String,
        #[serde(rename = "elementRef")]
        element_ref: String,
        parameters: TextParameters,
    },
    TypeText {
        kind: ActKind,
        #[serde(rename = "snapshotId")]
        snapshot_id: String,
        #[serde(rename = "elementRef")]
        element_ref: String,
        parameters: TextParameters,
    },
    Scroll {
        kind: ActKind,
        #[serde(rename = "snapshotId")]
        snapshot_id: String,
        #[serde(rename = "elementRef")]
        element_ref: String,
        parameters: ScrollParameters,
    },
    Wait {
        kind: ActKind,
        #[serde(rename = "snapshotId")]
        snapshot_id: String,
        #[serde(rename = "elementRef")]
        element_ref: String,
        parameters: WaitParameters,
    },
}

impl ExtensionActionCommand {
    pub fn name(&self) -> ActionName {
        match self {
            Self::Click { .. } => ActionName::Click,
            Self::SetValue { .. } => ActionName::SetValue,
            Self::TypeText { .. } => ActionName::TypeText,
            Self::Scroll { .. } => ActionName::Scroll,
            Self::Wait { .. } => ActionName::Wait,
        }
    }

    fn identity(&self) -> (&str, &str) {
        match self {
            Self::Click {
                snapshot_id,
                element_ref,
                ..
            }
            | Self::SetValue {
                snapshot_id,
                element_ref,
                ..
            }
            | Self::TypeText {
                snapshot_id,
                element_ref,
                ..
            }
            | Self::Scroll {
                snapshot_id,
                element_ref,
                ..
            }
            | Self::Wait {
                snapshot_id,
                element_ref,
                ..
            } => (snapshot_id, element_ref),
        }
    }

    pub fn snapshot_id(&self) -> &str {
        self.identity().0
    }
    pub fn element_ref(&self) -> &str {
        self.identity().1
    }

    pub fn valid(&self) -> bool {
        let (snapshot, element) = self.identity();
        snapshot.len() == 36
            && uuid::Uuid::parse_str(snapshot).is_ok()
            && element.len() <= 128
            && element
                .strip_prefix(snapshot)
                .is_some_and(|suffix| suffix.starts_with('-') && suffix.len() > 1)
            && element
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            && match self {
                Self::Click { parameters, .. } => parameters.count == 1,
                Self::SetValue { parameters, .. } | Self::TypeText { parameters, .. } => {
                    parameters.text.chars().count() <= 4000 && !parameters.text.contains('\0')
                }
                Self::Scroll { parameters, .. } => (-2400..=2400).contains(&parameters.delta),
                Self::Wait { parameters, .. } => {
                    !parameters.name_equals.trim().is_empty()
                        && parameters.name_equals.encode_utf16().count() <= 256
                        && (1..=10000).contains(&parameters.timeout_ms)
                }
            }
    }
}

impl std::fmt::Debug for ExtensionActionCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionActionCommand")
            .field("action", &self.name())
            .finish_non_exhaustive()
    }
}
