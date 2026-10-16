//! Deterministic Settings smoke decisions; no Windows token or desktop required.
#![allow(clippy::unwrap_used)]

#[path = "smoke/integrity.rs"]
mod integrity;

use gcu_core::driver::DriverError;
use integrity::{DiscoveryError, TokenApi, TokenFacts, TokenPair};
use std::cell::RefCell;

struct FakeTokens {
    target_rid: u32,
    target_elevated: u32,
    reads: RefCell<Vec<u32>>,
}
impl TokenApi for FakeTokens {
    fn facts(&self, pid: u32) -> Result<TokenFacts, String> {
        self.reads.borrow_mut().push(pid);
        match pid {
            7 => Ok(TokenFacts {
                pid,
                integrity_rid: self.target_rid,
                elevated: self.target_elevated,
            }),
            9 => Ok(TokenFacts {
                pid,
                integrity_rid: 8192,
                elevated: 0,
            }),
            _ => Err("fake OpenProcessToken: access denied".into()),
        }
    }
}
fn api(target_rid: u32, target_elevated: u32) -> FakeTokens {
    FakeTokens {
        target_rid,
        target_elevated,
        reads: RefCell::new(Vec::new()),
    }
}

#[test]
fn settings_elevation_refusal_measured_high_target_skips_with_both_tokens() {
    let api = api(12288, 1);
    let tokens = TokenPair::measure(&api, 7, 9).unwrap();
    let reason = tokens
        .discovery_refusal(DriverError::ElevationRequired.into())
        .unwrap();
    assert_eq!(*api.reads.borrow(), [7, 9]);
    assert!(reason.contains("target=High") && reason.contains("harness=Medium"));
    assert!(reason.contains("pid=7") && reason.contains("pid=9"));
    assert!(reason.contains("TokenElevation)=1") && reason.contains("TokenElevation)=0"));
    assert!(reason.contains("S-1-16-12288") && reason.contains("S-1-16-8192"));
    assert!(reason.contains("driver refused by design"));
}

#[test]
fn settings_elevation_refusal_measured_same_level_fails_even_with_elevation_bit() {
    let api = api(8192, 1);
    let tokens = TokenPair::measure(&api, 7, 9).unwrap();
    let error = tokens
        .discovery_refusal(DriverError::ElevationRequired.into())
        .unwrap_err();
    assert_eq!(*api.reads.borrow(), [7, 9]);
    assert!(error.contains("exact executable/window discovery: elevation required"));
    assert!(error.contains("target=Medium") && error.contains("harness=Medium"));
    assert!(error.contains("TokenElevation)=1"));
}

#[test]
fn settings_refusal_higher_integrity_without_elevation_bit_still_skips() {
    let tokens = TokenPair::measure(&api(12288, 0), 7, 9).unwrap();
    assert!(tokens
        .discovery_refusal(DriverError::ElevationRequired.into())
        .is_ok());
}

#[test]
fn settings_refusal_lower_integrity_and_unrelated_errors_never_skip() {
    let lower = TokenPair::measure(&api(4096, 0), 7, 9).unwrap();
    assert!(lower
        .discovery_refusal(DriverError::ElevationRequired.into())
        .is_err());
    let higher = TokenPair::measure(&api(12288, 1), 7, 9).unwrap();
    for error in [
        DriverError::CapabilityUnavailable.into(),
        DiscoveryError::Oracle("exact smoke HWND missing".into()),
        DiscoveryError::Oracle("elevation required".into()),
    ] {
        assert!(higher.discovery_refusal(error).is_err());
    }
}

#[test]
fn settings_integrity_query_failure_is_not_evidence_for_skip() {
    let error = TokenPair::measure(&api(12288, 1), 7, 10).unwrap_err();
    assert!(error.contains("harness pid=10") && error.contains("access denied"));
}
