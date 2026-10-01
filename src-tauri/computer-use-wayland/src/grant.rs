use std::collections::HashMap;
use zbus::zvariant::OwnedValue;

pub(crate) type Dict = HashMap<String, OwnedValue>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Monitor,
    Window,
}

impl SourceKind {
    pub(crate) fn mask(self) -> u32 {
        match self {
            Self::Monitor => 1,
            Self::Window => 2,
        }
    }

    /// A window capture grant does NOT isolate RemoteDesktop keyboard input.
    pub fn scope_label(self) -> &'static str {
        match self {
            Self::Monitor => "user-selected monitor capture; session-wide input permission",
            Self::Window => "user-selected window capture; session-wide input permission",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamGrant {
    pub node_id: u32,
    /// Prefer this over the reusable node ID when constructing a PW stream.
    pub pipewire_serial: Option<u64>,
    /// Only an exact EI region mapping can authorize absolute input.
    pub mapping_id: Option<String>,
    pub source: SourceKind,
    /// These are NOT frame pixel dimensions or screenshot scale factors.
    pub compositor_position: Option<(i32, i32)>,
    pub compositor_size: Option<(i32, i32)>,
    pub logical_size: Option<(i32, i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortalGrant {
    pub run_id: String,
    pub session_path: String,
    pub remote_desktop_version: u32,
    pub screen_cast_version: u32,
    pub devices: u32,
    pub stream: StreamGrant,
}

pub(crate) fn parse_stream(results: &mut Dict, source: SourceKind) -> Result<StreamGrant, String> {
    // Do not silently accept a subset, extra device classes, or extra monitors.
    let devices = u32::try_from(results.remove("devices").ok_or("missing granted devices")?)
        .map_err(|_| "invalid granted devices")?;
    if devices != 3 {
        return Err("portal did not grant exactly keyboard and pointer".into());
    }
    let mut streams =
        Vec::<(u32, Dict)>::try_from(results.remove("streams").ok_or("missing portal streams")?)
            .map_err(|_| "invalid portal streams")?;
    if streams.len() != 1 {
        return Err("portal must grant exactly one selected source".into());
    }
    let (node_id, mut props) = streams.remove(0);
    if node_id == 0 || node_id == u32::MAX {
        return Err("invalid PipeWire node ID".into());
    }
    let granted_type = u32::try_from(props.remove("source_type").ok_or("missing source type")?)
        .map_err(|_| "invalid source type")?;
    if granted_type != source.mask() {
        return Err("portal source does not match the requested scope".into());
    }
    let mapping_id = props
        .remove("mapping_id")
        .map(String::try_from)
        .transpose()
        .map_err(|_| "invalid EI mapping ID")?;
    if mapping_id
        .as_ref()
        .is_some_and(|s| s.is_empty() || s.len() > 1024 || s.contains('\0'))
    {
        return Err("invalid EI mapping ID".into());
    }
    let pipewire_serial = props
        .remove("pipewire-serial")
        .map(u64::try_from)
        .transpose()
        .map_err(|_| "invalid PipeWire serial")?;
    if pipewire_serial == Some(0) {
        return Err("invalid PipeWire serial".into());
    }
    let pair = |props: &mut Dict, key: &str, positive: bool| -> Result<_, String> {
        let value = props
            .remove(key)
            .map(<(i32, i32)>::try_from)
            .transpose()
            .map_err(|_| format!("invalid stream {key}"))?;
        if positive && value.is_some_and(|(w, h)| w <= 0 || h <= 0) {
            return Err(format!("nonpositive stream {key}"));
        }
        Ok(value)
    };
    Ok(StreamGrant {
        node_id,
        pipewire_serial,
        mapping_id,
        source,
        compositor_position: pair(&mut props, "position", false)?,
        compositor_size: pair(&mut props, "size", true)?,
        logical_size: pair(&mut props, "logical_size", true)?,
    })
}
