//! Single preflight for discovery, observation and every future input route.
use crate::api::WinApi;
use gcu_core::driver::{DriverError, Result};

/// Windows token mandatory integrity ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntegrityLevel {
    /// Untrusted token.
    Untrusted,
    /// Low mandatory label.
    Low,
    /// Normal interactive process (includes Medium Plus).
    Medium,
    /// Elevated administrator token.
    High,
    /// System or protected process token.
    System,
}

/// Security inputs from tokens/session/desktop queries, never from a client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessSecurity {
    /// Mandatory integrity.
    pub integrity: IntegrityLevel,
    /// TokenUIAccess; any engine UIAccess is forbidden.
    pub ui_access: bool,
    /// OS session identifier.
    pub session_id: u32,
    /// Session is active and attached to an interactive station.
    pub interactive: bool,
    /// Input desktop is not the normal Default desktop (UAC/login/lock).
    pub secure_desktop: bool,
    /// Process creation FILETIME, checked against enumeration.
    pub created: u64,
}

/// Pure engine startup gate. High/System/UIAccess is never silently downgraded.
pub fn check_engine(engine: &ProcessSecurity) -> Result<()> {
    if engine.integrity >= IntegrityLevel::High || engine.ui_access {
        return Err(DriverError::ElevationRequired);
    }
    if engine.secure_desktop || !engine.interactive {
        return Err(DriverError::SecureDesktop);
    }
    Ok(())
}

/// Uniform gate. Native token/desktop state is refreshed on each check.
#[derive(Clone, Copy, Debug, Default)]
pub struct WinGate;
impl WinGate {
    /// Check target before any observation/input, including accessibility and
    /// targeted messages. Foreground delivery additionally needs a current desktop.
    pub fn check(
        &self,
        api: &dyn WinApi,
        pid: u32,
        foreground: bool,
        desktop: Option<bool>,
    ) -> Result<ProcessSecurity> {
        let engine = api.engine_security()?;
        check_engine(&engine)?;
        let target = api.process_security(pid)?;
        check_target(&engine, &target, foreground, desktop)?;
        Ok(target)
    }
}

/// Pure target permission decision, shared by all delivery routes.
pub fn check_target(
    engine: &ProcessSecurity,
    target: &ProcessSecurity,
    foreground: bool,
    desktop: Option<bool>,
) -> Result<()> {
    check_engine(engine)?;
    if target.secure_desktop || !target.interactive || target.session_id != engine.session_id {
        return Err(DriverError::SecureDesktop);
    }
    if target.integrity > engine.integrity || target.ui_access {
        return Err(DriverError::ElevationRequired);
    }
    if foreground && desktop != Some(true) {
        return Err(DriverError::CapabilityUnavailable);
    }
    Ok(())
}
