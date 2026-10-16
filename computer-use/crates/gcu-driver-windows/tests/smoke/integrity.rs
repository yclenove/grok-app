//! Settings-only smoke classification using independent, measured token facts.
use gcu_core::driver::DriverError;
use std::fmt;

#[derive(Debug)]
pub enum DiscoveryError {
    Driver(DriverError),
    Oracle(String),
}
impl From<DriverError> for DiscoveryError {
    fn from(error: DriverError) -> Self {
        Self::Driver(error)
    }
}
impl From<String> for DiscoveryError {
    fn from(error: String) -> Self {
        Self::Oracle(error)
    }
}
impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Driver(error) => error.fmt(f),
            Self::Oracle(error) => error.fmt(f),
        }
    }
}

#[derive(Debug)]
pub struct TokenFacts {
    pub pid: u32,
    pub integrity_rid: u32,
    pub elevated: u32,
}
impl fmt::Display for TokenFacts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.integrity_rid {
            0..4096 => "Untrusted",
            4096..8192 => "Low",
            8192..8448 => "Medium",
            8448..12288 => "Medium Plus",
            12288..16384 => "High",
            16384..20480 => "System",
            _ => "Protected",
        };
        write!(f, "{level} [pid={}; GetTokenInformation(TokenIntegrityLevel)=S-1-16-{} (0x{:x}); GetTokenInformation(TokenElevation)={}]", self.pid, self.integrity_rid, self.integrity_rid, self.elevated)
    }
}

pub trait TokenApi {
    fn facts(&self, pid: u32) -> Result<TokenFacts, String>;
}

#[derive(Debug)]
pub struct TokenPair {
    target: TokenFacts,
    harness: TokenFacts,
}
impl TokenPair {
    pub fn measure(api: &impl TokenApi, target: u32, harness: u32) -> Result<Self, String> {
        let target = api
            .facts(target)
            .map_err(|e| format!("Settings target pid={target} token measurement failed: {e}"))?;
        let harness = api
            .facts(harness)
            .map_err(|e| format!("Settings harness pid={harness} token measurement failed: {e}"))?;
        Ok(Self { target, harness })
    }

    pub fn discovery_refusal(&self, error: DiscoveryError) -> Result<String, String> {
        if matches!(
            error,
            DiscoveryError::Driver(DriverError::ElevationRequired)
        ) && self.target.integrity_rid > self.harness.integrity_rid
        {
            return Ok(format!("measured {self}; target integrity higher; driver refused by design (ElevationRequired/UIPI); Settings search not executed"));
        }
        Err(format!("exact executable/window discovery: {error}; measured {self}; higher-integrity ElevationRequired Skip condition not met"))
    }
}
impl fmt::Display for TokenPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "target={}; harness={}", self.target, self.harness)
    }
}
