//! Strict bodies for fake inventory, status, and guest evidence.
//!
//! A parse failure is not "the resource is gone". Callers confirm absence
//! only from a successful, complete list.

use super::super::Check;

/// The body could not be used as evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParseError {
    /// The bytes are not the expected grammar.
    Malformed,
    /// A required marker or row is missing. Not an empty inventory.
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VmState {
    Running,
    Stopped,
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VmStatus {
    pub state: VmState,
    pub run: Option<String>,
    pub boot: Option<String>,
    pub incarnation: Option<String>,
    pub config: Option<String>,
    pub tart_up: Option<bool>,
    pub softnet_up: Option<bool>,
}

/// Identity measured from a successful clone body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CloneMark {
    pub incarnation: String,
    pub config: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GuestView {
    pub boot: String,
    pub uid: String,
    pub desktop: String,
    pub config: String,
}

/// Complete inventory. Names are raw tokens from the body, not delete targets.
pub(super) fn parse_list(body: &[u8]) -> Result<Vec<String>, ParseError> {
    let text = std::str::from_utf8(body).map_err(|_| ParseError::Malformed)?;
    if !text.ends_with('\n') || text.ends_with("\n\n") {
        return Err(ParseError::Incomplete);
    }
    let lines: Vec<&str> = text[..text.len() - 1].split('\n').collect();
    if lines.first().copied() != Some("list v1") {
        return Err(ParseError::Malformed);
    }
    let count_line = lines.get(1).ok_or(ParseError::Incomplete)?;
    let count_text = count_line
        .strip_prefix("count ")
        .ok_or(ParseError::Malformed)?;
    if count_text.is_empty() || !count_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseError::Malformed);
    }
    let count: usize = count_text.parse().map_err(|_| ParseError::Malformed)?;
    if lines.len() != count.saturating_add(3) {
        return Err(ParseError::Incomplete);
    }
    if lines.last().copied() != Some("end") {
        return Err(ParseError::Incomplete);
    }
    let mut names = Vec::with_capacity(count);
    for name in &lines[2..lines.len() - 1] {
        if name.is_empty() || name.len() > 64 || names.iter().any(|seen| seen == name) {
            return Err(ParseError::Malformed);
        }
        names.push((*name).to_owned());
    }
    if names.len() != count {
        return Err(ParseError::Incomplete);
    }
    Ok(names)
}

pub(super) fn parse_status(body: &[u8]) -> Result<VmStatus, ParseError> {
    let text = std::str::from_utf8(body).map_err(|_| ParseError::Malformed)?;
    if text.contains('\n') || !text.starts_with("status v1 ") {
        return Err(ParseError::Malformed);
    }
    let mut state = None;
    let mut run = None;
    let mut boot = None;
    let mut incarnation = None;
    let mut config = None;
    let mut tart_up = None;
    let mut softnet_up = None;
    for token in text["status v1 ".len()..].split(' ') {
        if token.is_empty() {
            return Err(ParseError::Malformed);
        }
        let (key, value) = token.split_once('=').ok_or(ParseError::Malformed)?;
        match key {
            "state" if state.is_none() => {
                state = Some(match value {
                    "running" => VmState::Running,
                    "stopped" => VmState::Stopped,
                    "absent" => VmState::Absent,
                    _ => return Err(ParseError::Malformed),
                });
            }
            "run" if run.is_none() => run = Some(token_value(value)?),
            "boot" if boot.is_none() => boot = Some(token_value(value)?),
            "inc" if incarnation.is_none() => incarnation = Some(token_value(value)?),
            "cfg" if config.is_none() => config = Some(token_value(value)?),
            "tart" if tart_up.is_none() => tart_up = Some(up_down(value)?),
            "softnet" if softnet_up.is_none() => softnet_up = Some(up_down(value)?),
            _ => return Err(ParseError::Malformed),
        }
    }
    let state = state.ok_or(ParseError::Incomplete)?;
    // A live process contradicts `absent`. Absence of the VM is a list fact,
    // not a status fact, and it does not certify that Tart or Softnet exited.
    if state == VmState::Absent && (tart_up == Some(true) || softnet_up == Some(true)) {
        return Err(ParseError::Malformed);
    }
    if state != VmState::Absent
        && (run.is_none()
            || boot.is_none()
            || incarnation.is_none()
            || config.is_none()
            || tart_up.is_none()
            || softnet_up.is_none())
    {
        return Err(ParseError::Incomplete);
    }
    Ok(VmStatus {
        state,
        run,
        boot,
        incarnation,
        config,
        tart_up,
        softnet_up,
    })
}

/// Stop is real only when the VM is stopped and both processes have exited.
///
/// `Absent` is not that proof. A missing tart/softnet field is not an exit.
pub(super) fn stopped_clean(status: &VmStatus) -> bool {
    status.state == VmState::Stopped
        && status.tart_up == Some(false)
        && status.softnet_up == Some(false)
}

pub(super) fn parse_clone(body: &[u8]) -> Result<CloneMark, ParseError> {
    let text = std::str::from_utf8(body).map_err(|_| ParseError::Malformed)?;
    if text.contains('\n') || !text.starts_with("clone v1 ") {
        return Err(ParseError::Malformed);
    }
    let mut incarnation = None;
    let mut config = None;
    for token in text["clone v1 ".len()..].split(' ') {
        if token.is_empty() {
            return Err(ParseError::Malformed);
        }
        let (key, value) = token.split_once('=').ok_or(ParseError::Malformed)?;
        match key {
            "inc" if incarnation.is_none() => incarnation = Some(token_value(value)?),
            "cfg" if config.is_none() => config = Some(token_value(value)?),
            _ => return Err(ParseError::Malformed),
        }
    }
    Ok(CloneMark {
        incarnation: incarnation.ok_or(ParseError::Incomplete)?,
        config: config.ok_or(ParseError::Incomplete)?,
    })
}

pub(super) fn parse_guest(body: &[u8]) -> Result<GuestView, ParseError> {
    let text = std::str::from_utf8(body).map_err(|_| ParseError::Malformed)?;
    if !text.ends_with('\n') {
        return Err(ParseError::Incomplete);
    }
    let lines: Vec<&str> = text[..text.len() - 1].split('\n').collect();
    if lines.len() != 6 || lines[0] != "guest v1" || lines[5] != "end" {
        return Err(ParseError::Incomplete);
    }
    Ok(GuestView {
        boot: keyed(lines[1], "boot")?,
        uid: keyed(lines[2], "uid")?,
        desktop: keyed(lines[3], "desktop")?,
        config: keyed(lines[4], "config")?,
    })
}

pub(super) fn parse_probe(body: &[u8]) -> Result<Check, ParseError> {
    match body {
        b"passed" => Ok(Check::Passed),
        b"failed" => Ok(Check::Failed),
        b"unknown" => Ok(Check::Unknown),
        _ => Err(ParseError::Malformed),
    }
}

pub(super) fn parse_ack(body: &[u8], run: &str) -> Result<u64, ParseError> {
    let text = std::str::from_utf8(body).map_err(|_| ParseError::Malformed)?;
    let rest = text
        .strip_prefix("ack v1 run=")
        .ok_or(ParseError::Malformed)?;
    let (got, generation) = rest
        .split_once(" generation=")
        .ok_or(ParseError::Malformed)?;
    if got != run || generation.is_empty() || !generation.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ParseError::Malformed);
    }
    let value: u64 = generation.parse().map_err(|_| ParseError::Malformed)?;
    if value == 0 {
        return Err(ParseError::Malformed);
    }
    Ok(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoginMark {
    Ready,
    Missing,
}

pub(super) fn parse_login(body: &[u8]) -> Result<LoginMark, ParseError> {
    match body {
        b"login v1 ready" => Ok(LoginMark::Ready),
        b"login v1 missing" => Ok(LoginMark::Missing),
        _ => Err(ParseError::Malformed),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScoreMark {
    Pass,
    Fail,
    Partial,
    Timeout,
    Crash,
}

pub(super) fn parse_score(body: &[u8]) -> Result<ScoreMark, ParseError> {
    match body {
        b"score pass" => Ok(ScoreMark::Pass),
        b"score fail" => Ok(ScoreMark::Fail),
        b"score partial" => Ok(ScoreMark::Partial),
        b"score timeout" => Ok(ScoreMark::Timeout),
        b"score crash" => Ok(ScoreMark::Crash),
        _ => Err(ParseError::Malformed),
    }
}

fn token_value(value: &str) -> Result<String, ParseError> {
    if value.is_empty() || value.len() > 64 || value.bytes().any(|byte| !byte.is_ascii_graphic()) {
        return Err(ParseError::Malformed);
    }
    Ok(value.to_owned())
}

fn up_down(value: &str) -> Result<bool, ParseError> {
    match value {
        "up" => Ok(true),
        "exited" => Ok(false),
        _ => Err(ParseError::Malformed),
    }
}

fn keyed(line: &str, key: &str) -> Result<String, ParseError> {
    let prefix = format!("{key}=");
    let value = line.strip_prefix(&prefix).ok_or(ParseError::Malformed)?;
    token_value(value)
}
