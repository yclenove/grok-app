//! Deterministic interpretation of measurements, never of SendInput acceptance alone.
#![cfg_attr(not(windows), allow(dead_code))]
use serde::{Deserialize, Serialize};

pub const MODES: &[(&str, &str, usize)] = &[
    ("ZeroMove", "zero_move", 1),
    ("ZeroWheel", "zero_wheel", 1),
    ("ZeroHWheel", "zero_hwheel", 1),
    ("AbsoluteSamePosition", "absolute_same_position", 1),
    ("RelativePair", "relative_pair", 2),
    ("XButtonPair", "xbutton_pair", 2),
];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Row {
    pub mode: String,
    pub marked: bool,
    pub sent: Option<u32>,
    pub requested: usize,
    pub win32: Option<u32>,
    pub elapsed_us: u128,
    pub callback_seen: bool,
    pub injected_callback_seen: bool,
    pub dispatch_error: Option<String>,
    pub callbacks: Vec<serde_json::Value>,
    pub before: (i32, i32),
    pub after: (i32, i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Run { probe: String, marked: bool },
    NoCallbacks(String),
}

pub fn decide(rows: &[Row]) -> Result<Decision, String> {
    // Stable preference: zero-effect candidates before motion/buttons, and a
    // tagged injected callback before unmarked evidence. Never skip a working row.
    for evidence in 0..3 {
        for marked in [true, false] {
            for (mode, probe, _) in MODES {
                if rows.iter().any(|r| {
                    r.mode == *mode
                        && r.marked == marked
                        && match evidence {
                            0 => r.injected_callback_seen,
                            1 => r.callback_seen,
                            _ => !r.callbacks.is_empty(),
                        }
                }) {
                    return Ok(Decision::Run {
                        probe: (*probe).into(),
                        marked,
                    });
                }
            }
        }
    }
    if rows.len() != 12 {
        return Err(format!("incomplete mouse matrix: {}/12 rows", rows.len()));
    }
    for (mode, _, requested) in MODES {
        for marked in [true, false] {
            let matches: Vec<_> = rows
                .iter()
                .filter(|r| r.mode == *mode && r.marked == marked)
                .collect();
            if matches.len() != 1 {
                return Err(format!(
                    "mouse matrix missing/duplicate {mode}, marked={marked}"
                ));
            }
            let row = matches[0];
            if row.requested != *requested
                || row.sent != Some(*requested as u32)
                || row.win32 != Some(0)
                || row.dispatch_error.is_some()
                || row.elapsed_us < 1_000_000
            {
                return Err(format!("unproven mouse matrix row: {row:?}"));
            }
        }
    }
    let positions: Vec<_> = rows
        .iter()
        .map(|r| {
            format!(
                "{} marked={}: {:?}->{:?}",
                r.mode, r.marked, r.before, r.after
            )
        })
        .collect();
    let position_summary = if rows
        .iter()
        .all(|r| r.before == rows[0].before && r.after == rows[0].before)
    {
        format!("all 12 before==after=={:?}", rows[0].before)
    } else {
        positions.join(", ")
    };
    Ok(Decision::NoCallbacks(format!(
        "same-process empirical mouse matrix: 12 probe variants (6 kinds x marked/unmarked); all sent==requested, Win32=0; all callback_seen=false, injected_callback_seen=false, callbacks=[] after >=1000ms each; physical cursor positions: {}; physical-mouse Windows retest required",
        position_summary
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn silent() -> Vec<Row> {
        MODES
            .iter()
            .flat_map(|(mode, _, count)| {
                [true, false].map(|marked| Row {
                    mode: (*mode).into(),
                    marked,
                    sent: Some(*count as u32),
                    requested: *count,
                    win32: Some(0),
                    elapsed_us: 1_000_000,
                    callback_seen: false,
                    injected_callback_seen: false,
                    dispatch_error: None,
                    callbacks: vec![],
                    before: (512, 384),
                    after: (512, 384),
                })
            })
            .collect()
    }
    #[test]
    fn complete_silent_matrix_allows_only_measured_environment_skip() {
        let Decision::NoCallbacks(reason) = decide(&silent()).unwrap() else {
            panic!("expected measured silence")
        };
        let outcome = super::super::harness::foreground_unavailable(
            &["Stop listener hook proof timeout".into()],
            &Decision::NoCallbacks(reason.clone()),
        )
        .unwrap();
        assert!(
            matches!(outcome, super::super::harness::Outcome::Skip(ref summary) if summary.contains(&reason))
        );
        for fact in [
            "12 probe variants",
            "callbacks=[]",
            "(512, 384)",
            ">=1000ms",
        ] {
            assert!(reason.contains(fact), "{reason}");
        }
    }
    #[test]
    fn serialized_native_rows_keep_the_decision_and_injected_evidence_has_priority() {
        let rows = silent();
        let decoded: Vec<Row> =
            serde_json::from_value(serde_json::to_value(&rows).unwrap()).unwrap();
        assert_eq!(decide(&rows), decide(&decoded));
        let mut rows = decoded;
        rows[0].callback_seen = true;
        rows[4].injected_callback_seen = true;
        assert_eq!(
            decide(&rows).unwrap(),
            Decision::Run {
                probe: "zero_hwheel".into(),
                marked: true
            }
        );
    }
    #[test]
    fn every_working_variant_prevents_skip_and_selects_its_probe() {
        for index in 0..12 {
            for field in 0..3 {
                let mut rows = silent();
                match field {
                    0 => rows[index].callback_seen = true,
                    1 => rows[index].injected_callback_seen = true,
                    _ => rows[index].callbacks.push(serde_json::json!({"flags":1})),
                }
                let expected = MODES.iter().find(|m| m.0 == rows[index].mode).unwrap().1;
                assert_eq!(
                    decide(&rows).unwrap(),
                    Decision::Run {
                        probe: expected.into(),
                        marked: rows[index].marked
                    }
                );
                rows.reverse();
                assert!(matches!(decide(&rows), Ok(Decision::Run { .. })));
            }
        }
    }
    #[test]
    fn incomplete_duplicate_failed_or_short_measurements_never_skip() {
        for fault in 0..7 {
            let mut rows = silent();
            match fault {
                0 => {
                    rows.pop();
                }
                1 => rows[0] = rows[1].clone(),
                2 => rows[0].sent = Some(0),
                3 => rows[0].win32 = Some(5),
                4 => rows[0].dispatch_error = Some("pending send".into()),
                5 => rows[0].elapsed_us = 999_999,
                _ => rows[0].requested = 2,
            }
            assert!(decide(&rows).is_err(), "fault={fault}");
        }
        assert!(decide(&[]).is_err());
    }
}
