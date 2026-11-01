//! S11a offline contract. No Tart binary, no sleep, no lock.
//!
//! The parent re-execs this binary and polls `try_wait` on a monotonic clock.
//! Each check child is capped at 30s (ADR 0018 §8). That cap is not the whole
//! target. The parent kills a child that does not return and bounds the reap.
//! A child that does not return exists only so this parent can prove the kill.
//! It is not a product flag.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::Ipv4Addr;
use std::num::NonZeroU64;
use std::path::Path;
use std::time::Duration;

use gcu_eval::disposable::tart::{
    AttemptDeadline, AttemptLedger, BoundedOutput, CallError, ClockError, CloneDeadline, CloneId,
    DeadlineError, DeleteDeadline, Digest, FakeClock, FakeGuestChannel, FakeLedger, FakeTartRunner,
    GoldenImage, GuestChannel, GuestCommand, HostClock, IdentityError, InstallManifest,
    LeaseDeadline, LedgerError, LedgerFact, LoginDeadline, ManifestError, MonoInstant, OutputError,
    PolicyError, ProbeDeadline, QueryDeadline, RevokeDeadline, RunDeadline, RunId, ScriptOutcome,
    ScriptedReply, SealDeadline, SoftnetFlags, StageDeadline, StartDeadline, StopDeadline,
    TartCommand, TartRunner, GUEST_CHANNEL_VERSION, MAX_ALLOW_CIDRS, MAX_IDENTITY_LEN,
    MAX_OUTPUT_BYTES, SOFTNET_BLOCK_CIDR,
};
use gcu_eval::disposable::Probe;

macro_rules! ok {
    ($expr:expr) => {
        match $expr {
            Ok(value) => value,
            Err(err) => return Err(format!("{}: {err:?}", stringify!($expr))),
        }
    };
}

#[path = "tart_offline/review_fixes.rs"]
mod review_fixes;
#[path = "tart_offline/s11b/mod.rs"]
mod s11b;
#[path = "tart_offline/watch.rs"]
mod watch;

type Check = fn() -> Result<(), String>;

fn main() {
    // The checks child's exit status becomes this process's status.
    std::process::exit(watch::entry(run_checks));
}

fn run_checks() -> i32 {
    let mut failures = Vec::new();
    let tests: &[(&str, Check)] = &[
        (
            "identity_rejects_empty_overlong_path_dash_and_golden",
            identity_rejects_empty_overlong_path_dash_and_golden,
        ),
        (
            "identity_accepts_bounded_exact_tokens",
            identity_accepts_bounded_exact_tokens,
        ),
        (
            "softnet_flags_reject_non_public_width_and_duplicates",
            softnet_flags_reject_non_public_width_and_duplicates,
        ),
        (
            "deadline_rejects_zero_above_cap_and_overflow",
            deadline_rejects_zero_above_cap_and_overflow,
        ),
        (
            "caps_match_adr_0018_and_have_no_default",
            caps_match_adr_0018_and_have_no_default,
        ),
        (
            "output_keeps_one_mebibyte_and_drops_the_rest",
            output_keeps_one_mebibyte_and_drops_the_rest,
        ),
        (
            "manifest_stores_three_digests",
            manifest_stores_three_digests,
        ),
        (
            "fake_clock_advances_without_sleep_or_rewind",
            fake_clock_advances_without_sleep_or_rewind,
        ),
        (
            "reserve_is_visible_before_clone_and_permit_is_single_use",
            reserve_is_visible_before_clone_and_permit_is_single_use,
        ),
        (
            "ledger_matches_exact_ids_and_seals_without_prefix",
            ledger_matches_exact_ids_and_seals_without_prefix,
        ),
        (
            "ledger_expired_call_does_not_write",
            ledger_expired_call_does_not_write,
        ),
        (
            "fake_runner_records_scripted_faults_and_bounds",
            fake_runner_records_scripted_faults_and_bounds,
        ),
        (
            "expired_runner_call_does_not_consume_the_script",
            expired_runner_call_does_not_consume_the_script,
        ),
        (
            "command_trace_is_fixed_ops_with_required_softnet",
            command_trace_is_fixed_ops_with_required_softnet,
        ),
        (
            "guest_channel_records_versioned_ops_without_a_transport",
            guest_channel_records_versioned_ops_without_a_transport,
        ),
        (
            "sources_have_no_process_launch_or_open_ended_wait",
            sources_have_no_process_launch_or_open_ended_wait,
        ),
        (
            "clone_at_absolute_deadline_expires_without_the_script",
            review_fixes::clone_at_absolute_deadline_expires_without_the_script,
        ),
        (
            "clone_shorter_parent_deadline_is_recorded",
            review_fixes::clone_shorter_parent_deadline_is_recorded,
        ),
        (
            "clone_shorter_call_deadline_is_recorded",
            review_fixes::clone_shorter_call_deadline_is_recorded,
        ),
        (
            "clone_at_call_deadline_expires_while_permit_is_live",
            review_fixes::clone_at_call_deadline_expires_while_permit_is_live,
        ),
        (
            "expired_guest_call_keeps_the_queued_reply",
            review_fixes::expired_guest_call_keeps_the_queued_reply,
        ),
        (
            "bounded_output_debug_hides_text_and_bytes",
            review_fixes::bounded_output_debug_hides_text_and_bytes,
        ),
        (
            "softnet_rejects_special_use_at_prefix_boundaries",
            review_fixes::softnet_rejects_special_use_at_prefix_boundaries,
        ),
    ];
    for (name, test) in tests {
        if let Err(err) = test() {
            failures.push(format!("{name}: {err}"));
        }
    }
    s11b::run(&mut failures);
    if !failures.is_empty() {
        panic!("{}", failures.join("\n"));
    }
    eprintln!("tart_offline ok: {}", tests.len());
    0
}

fn identity_rejects_empty_overlong_path_dash_and_golden() -> Result<(), String> {
    let long = "a".repeat(MAX_IDENTITY_LEN + 1);
    let cases = [
        ("", IdentityError::Empty),
        (long.as_str(), IdentityError::TooLong { len: long.len() }),
        ("a/b", IdentityError::Path),
        ("a\\b", IdentityError::Path),
        (".", IdentityError::Path),
        ("..", IdentityError::Path),
        ("foo/../golden", IdentityError::Path),
        ("foo--bar", IdentityError::DoubleDash),
        ("--flag", IdentityError::DoubleDash),
        ("a---b", IdentityError::DoubleDash),
        ("golden", IdentityError::GoldenName),
        ("golden-1", IdentityError::GoldenName),
        ("Golden", IdentityError::GoldenName),
        ("GOLDEN-vm", IdentityError::GoldenName),
        ("macos-s2-eval", IdentityError::GoldenName),
        ("Macos-S2-Eval-copy", IdentityError::GoldenName),
        ("a b", IdentityError::Illegal),
        ("-abc", IdentityError::Illegal),
        ("abc-", IdentityError::Illegal),
        ("_abc", IdentityError::Illegal),
        ("has.dot", IdentityError::Illegal),
        ("尝试", IdentityError::Illegal),
        ("a\nb", IdentityError::Illegal),
    ];
    for (raw, expected) in cases {
        must_err(RunId::new(raw), expected, &format!("run {raw:?}"))?;
        must_err(CloneId::new(raw), expected, &format!("clone {raw:?}"))?;
    }
    if GoldenImage::MacosS2Eval.name() != "macos-s2-eval" {
        return Err("golden name drifted".into());
    }
    Ok(())
}

fn identity_accepts_bounded_exact_tokens() -> Result<(), String> {
    for raw in ["a", "attempt-1", "clone_1", "Z9", "not-golden", "xgolden"] {
        let run = ok!(RunId::new(raw));
        let clone = ok!(CloneId::new(raw));
        if run.as_str() != raw || clone.as_str() != raw {
            return Err(format!("token changed: {raw}"));
        }
    }
    let capped = "b".repeat(MAX_IDENTITY_LEN);
    let run = ok!(RunId::new(&capped));
    if run.as_str() != capped {
        return Err("64-byte token changed".into());
    }
    let left = ok!(CloneId::new("clone-1"));
    let right = ok!(CloneId::new("clone-10"));
    if left == right {
        return Err("prefix ids compared equal".into());
    }
    Ok(())
}

fn softnet_flags_reject_non_public_width_and_duplicates() -> Result<(), String> {
    let rejected = [
        Ipv4Addr::new(0, 0, 0, 0),
        Ipv4Addr::new(10, 1, 2, 3),
        Ipv4Addr::new(127, 0, 0, 1),
        Ipv4Addr::new(169, 254, 169, 254),
        Ipv4Addr::new(192, 168, 64, 1),
        Ipv4Addr::new(172, 16, 0, 1),
        Ipv4Addr::new(172, 31, 255, 255),
        Ipv4Addr::new(100, 64, 0, 1),
        Ipv4Addr::new(224, 0, 0, 1),
        Ipv4Addr::new(240, 0, 0, 1),
        Ipv4Addr::new(255, 255, 255, 255),
        Ipv4Addr::new(192, 0, 2, 1),
        Ipv4Addr::new(198, 51, 100, 1),
        Ipv4Addr::new(203, 0, 113, 1),
        Ipv4Addr::new(198, 18, 0, 1),
    ];
    for addr in rejected {
        must_err(
            SoftnetFlags::try_new(&[addr]),
            PolicyError::NotPublic { addr },
            "non-public",
        )?;
    }
    must_err(SoftnetFlags::try_new(&[]), PolicyError::Empty, "empty")?;
    let seventeen: Vec<Ipv4Addr> = (1..=17).map(|host| Ipv4Addr::new(1, 2, 3, host)).collect();
    must_err(
        SoftnetFlags::try_new(&seventeen),
        PolicyError::TooMany {
            len: seventeen.len(),
        },
        "too many",
    )?;
    if seventeen.len() != MAX_ALLOW_CIDRS + 1 {
        return Err("allow cap drifted".into());
    }
    let again = Ipv4Addr::new(1, 1, 1, 1);
    must_err(
        SoftnetFlags::try_new(&[again, Ipv4Addr::new(8, 8, 8, 8), again]),
        PolicyError::Duplicate { addr: again },
        "duplicate",
    )?;
    let first = Ipv4Addr::new(8, 8, 8, 8);
    let second = Ipv4Addr::new(1, 1, 1, 1);
    let flags = ok!(SoftnetFlags::try_new(&[
        first,
        second,
        Ipv4Addr::new(172, 15, 0, 1),
        Ipv4Addr::new(100, 63, 0, 1),
    ]));
    if flags.allow()
        != [
            first,
            second,
            Ipv4Addr::new(172, 15, 0, 1),
            Ipv4Addr::new(100, 63, 0, 1),
        ]
    {
        return Err("allow order drifted".into());
    }
    if SoftnetFlags::block_cidr() != "0.0.0.0/0" || SOFTNET_BLOCK_CIDR != "0.0.0.0/0" {
        return Err("softnet block is not 0.0.0.0/0".into());
    }
    Ok(())
}

fn deadline_rejects_zero_above_cap_and_overflow() -> Result<(), String> {
    let now = MonoInstant::EPOCH;
    must_err(
        QueryDeadline::after(now, Duration::ZERO),
        DeadlineError::Zero,
        "zero",
    )?;
    must_err(
        QueryDeadline::after(now, QueryDeadline::CAP + Duration::from_nanos(1)),
        DeadlineError::AboveCap,
        "above cap",
    )?;
    must_err(
        QueryDeadline::after(now, Duration::MAX),
        DeadlineError::AboveCap,
        "max duration",
    )?;
    must_err(
        QueryDeadline::after(MonoInstant::from_nanos(u64::MAX), Duration::from_nanos(1)),
        DeadlineError::Overflow,
        "overflow",
    )?;
    let built = ok!(QueryDeadline::after(now, Duration::from_millis(1)));
    if built.at() <= now {
        return Err("deadline did not move forward".into());
    }
    let full = ok!(AttemptDeadline::after(now, AttemptDeadline::CAP));
    let cap_nanos = u64::try_from(AttemptDeadline::CAP.as_nanos()).unwrap_or(0);
    if cap_nanos == 0 || full.at().nanos() != cap_nanos {
        return Err("attempt cap was not applied".into());
    }
    must_err(
        AttemptDeadline::after(now, AttemptDeadline::CAP + Duration::from_nanos(1)),
        DeadlineError::AboveCap,
        "attempt above cap",
    )?;
    Ok(())
}

fn caps_match_adr_0018_and_have_no_default() -> Result<(), String> {
    let caps = [
        QueryDeadline::CAP,
        CloneDeadline::CAP,
        StartDeadline::CAP,
        StopDeadline::CAP,
        DeleteDeadline::CAP,
        ProbeDeadline::CAP,
        gcu_eval::disposable::tart::GroupDeadline::CAP,
        StageDeadline::CAP,
        LoginDeadline::CAP,
        RunDeadline::CAP,
        LeaseDeadline::CAP,
        RevokeDeadline::CAP,
        SealDeadline::CAP,
        AttemptDeadline::CAP,
    ];
    let expected = [
        Duration::from_secs(5),
        Duration::from_secs(60),
        Duration::from_secs(120),
        Duration::from_secs(20),
        Duration::from_secs(20),
        Duration::from_secs(2),
        Duration::from_secs(60),
        Duration::from_secs(5),
        Duration::from_secs(5 * 60),
        Duration::from_secs(15 * 60),
        Duration::from_secs(3),
        Duration::from_secs(10),
        Duration::from_secs(10),
        Duration::from_secs(10 * 60 + 15 * 60 + 60),
    ];
    if caps != expected {
        return Err(format!("caps drifted: {caps:?}"));
    }
    if caps.iter().any(|cap| cap.is_zero()) {
        return Err("a cap is zero".into());
    }
    Ok(())
}

fn output_keeps_one_mebibyte_and_drops_the_rest() -> Result<(), String> {
    if MAX_OUTPUT_BYTES != 1024 * 1024 {
        return Err("output cap drifted".into());
    }
    let exact = ok!(BoundedOutput::try_new(vec![7; MAX_OUTPUT_BYTES]));
    if exact.len() != MAX_OUTPUT_BYTES || exact.is_empty() {
        return Err("exact cap was not retained".into());
    }
    must_err(
        BoundedOutput::try_new(vec![7; MAX_OUTPUT_BYTES + 1]),
        OutputError::TooLong {
            len: MAX_OUTPUT_BYTES + 1,
        },
        "overlong body",
    )?;
    must_err(
        ScriptedReply::success(vec![1; MAX_OUTPUT_BYTES + 1]),
        OutputError::TooLong {
            len: MAX_OUTPUT_BYTES + 1,
        },
        "overlong success",
    )?;
    let malformed = ok!(ScriptedReply::malformed(vec![0xff, 0xfe]));
    match malformed {
        ScriptedReply::Malformed(body) => {
            if body.as_bytes() != [0xff, 0xfe] {
                return Err("malformed bytes changed".into());
            }
        }
        _ => return Err("malformed was reclassified".into()),
    }
    match ScriptedReply::overlong() {
        ScriptedReply::Overlong => Ok(()),
        _ => Err("overlong carries a body".into()),
    }
}

fn manifest_stores_three_digests() -> Result<(), String> {
    must_err(
        Digest::try_from_bytes([0; 32]),
        ManifestError::ZeroDigest,
        "zero digest",
    )?;
    let program = ok!(Digest::try_from_bytes([1; 32]));
    let repeated = InstallManifest::new(program, program, program);
    if repeated.program() != program || repeated.golden() != program || repeated.policy() != program
    {
        return Err("equal digests were not stored".into());
    }
    let golden = ok!(Digest::try_from_bytes([2; 32]));
    let policy = ok!(Digest::try_from_bytes([3; 32]));
    let manifest = InstallManifest::new(program, golden, policy);
    if manifest.program() != program || manifest.golden() != golden || manifest.policy() != policy {
        return Err("manifest did not keep its digests".into());
    }
    Ok(())
}

fn fake_clock_advances_without_sleep_or_rewind() -> Result<(), String> {
    let clock = FakeClock::new();
    let shared = clock.clone();
    if clock.now() != MonoInstant::EPOCH {
        return Err("clock did not start at the epoch".into());
    }
    // 45s would blow the 30s watchdog if advance slept.
    ok!(clock.advance(Duration::from_secs(45)));
    if shared.now().nanos() != 45 * 1_000_000_000 {
        return Err("shared clock did not observe the jump".into());
    }
    let before = clock.now();
    ok!(clock.advance(Duration::from_millis(1)));
    if clock.now() <= before {
        return Err("clock moved backwards".into());
    }
    must_err(
        clock.advance(Duration::ZERO),
        ClockError::ZeroStep,
        "zero step",
    )?;
    must_err(
        clock.advance(Duration::MAX),
        ClockError::Overflow,
        "max step",
    )?;
    Ok(())
}

fn reserve_is_visible_before_clone_and_permit_is_single_use() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut ledger = FakeLedger::new(clock.clone());
    let runner = FakeTartRunner::new(clock.clone());
    let run = ok!(RunId::new("run-7"));
    let clone = ok!(CloneId::new("clone-7"));
    must_err(
        query_dyn(&ledger, &clone, &clock)?,
        LedgerError::NotFound,
        "query before reserve",
    )?;
    if !runner.trace().is_empty() {
        return Err("runner recorded a command before reserve".into());
    }
    let absolute = ok!(AttemptDeadline::after(clock.now(), Duration::from_secs(60)));
    let permit = noted(reserve_dyn(
        &mut ledger,
        run.clone(),
        clone.clone(),
        absolute,
        &clock,
    ))?;
    if permit.run() != &run || permit.clone_id() != &clone {
        return Err("permit does not name the reserved identity".into());
    }
    if permit.absolute_deadline() != absolute.at() {
        return Err("permit deadline differs from the reserved instant".into());
    }
    let view = ok!(query_dyn(&ledger, &clone, &clock)?);
    if view.run() != &run
        || view.clone_id() != &clone
        || view.absolute_deadline() != absolute.at()
        || !view.facts().is_empty()
        || view.is_sealed()
    {
        return Err("reservation was not visible before clone".into());
    }
    if !runner.trace().is_empty() {
        return Err("reserve invoked clone".into());
    }
    std::mem::drop(permit);
    if !runner.trace().is_empty() {
        return Err("dropping a permit invoked clone".into());
    }
    let still = ok!(query_dyn(&ledger, &clone, &clock)?);
    if still.is_sealed() {
        return Err("dropping a permit sealed the row".into());
    }
    must_err(
        reserve_dyn(
            &mut ledger,
            ok!(RunId::new("run-8")),
            clone,
            absolute,
            &clock,
        ),
        LedgerError::DuplicateClone,
        "second permit",
    )?;
    must_err(
        reserve_dyn(
            &mut ledger,
            run,
            ok!(CloneId::new("clone-8")),
            absolute,
            &clock,
        ),
        LedgerError::DuplicateRun,
        "second run",
    )?;
    Ok(())
}

fn ledger_matches_exact_ids_and_seals_without_prefix() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut ledger = FakeLedger::new(clock.clone());
    let short = ok!(CloneId::new("clone-1"));
    let longer = ok!(CloneId::new("clone-10"));
    let short_run = ok!(RunId::new("run-1"));
    let longer_run = ok!(RunId::new("run-10"));
    let absolute = ok!(AttemptDeadline::after(clock.now(), AttemptDeadline::CAP));
    let _short_permit = noted(reserve_dyn(
        &mut ledger,
        short_run.clone(),
        short.clone(),
        absolute,
        &clock,
    ))?;
    let _longer_permit = noted(reserve_dyn(
        &mut ledger,
        longer_run,
        longer.clone(),
        absolute,
        &clock,
    ))?;
    let found = ok!(query_dyn(&ledger, &short, &clock)?);
    if found.run() != &short_run {
        return Err("exact query returned another run".into());
    }
    ok!(ledger.append(
        &short,
        LedgerFact::CloneIssued,
        ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP)),
    ));
    ok!(ledger.append(
        &longer,
        LedgerFact::RecoveryNoted,
        ok!(QueryDeadline::after(clock.now(), Duration::from_millis(1))),
    ));
    let after = ok!(query_dyn(&ledger, &short, &clock)?);
    if after.facts() != [LedgerFact::CloneIssued] {
        return Err("append leaked across ids".into());
    }
    ok!(ledger.seal(
        &short,
        ok!(SealDeadline::after(clock.now(), SealDeadline::CAP)),
    ));
    must_err(
        ledger.append(
            &short,
            LedgerFact::RecoveryNoted,
            ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP)),
        ),
        LedgerError::Sealed,
        "append after seal",
    )?;
    must_err(
        ledger.seal(
            &short,
            ok!(SealDeadline::after(clock.now(), Duration::from_secs(1))),
        ),
        LedgerError::Sealed,
        "second seal",
    )?;
    let sealed = ok!(query_dyn(&ledger, &short, &clock)?);
    if !sealed.is_sealed() || sealed.facts() != [LedgerFact::CloneIssued] {
        return Err("seal rewrote the row".into());
    }
    must_err(
        ledger.query(
            &ok!(CloneId::new("clone-2")),
            ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP)),
        ),
        LedgerError::NotFound,
        "missing id",
    )?;
    Ok(())
}

fn ledger_expired_call_does_not_write() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut ledger = FakeLedger::new(clock.clone());
    let run = ok!(RunId::new("run-9"));
    let clone = ok!(CloneId::new("clone-9"));
    let stale_call = ok!(QueryDeadline::after(clock.now(), Duration::from_secs(1)));
    let absolute = ok!(AttemptDeadline::after(clock.now(), Duration::from_secs(30)));
    ok!(clock.advance(Duration::from_secs(1)));
    must_err(
        ledger.reserve(run.clone(), clone.clone(), absolute, stale_call),
        LedgerError::Expired,
        "expired reserve",
    )?;
    let live = ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP));
    must_err(
        ledger.query(&clone, live),
        LedgerError::NotFound,
        "expired reserve wrote a row",
    )?;
    let soon = ok!(AttemptDeadline::after(clock.now(), Duration::from_secs(1)));
    ok!(clock.advance(Duration::from_secs(1)));
    must_err(
        ledger.reserve(
            run,
            clone.clone(),
            soon,
            ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP)),
        ),
        LedgerError::AttemptAlreadyExpired,
        "expired attempt",
    )?;
    let absolute = ok!(AttemptDeadline::after(clock.now(), Duration::from_secs(60)));
    let _permit = noted(reserve_dyn(
        &mut ledger,
        ok!(RunId::new("run-9b")),
        clone.clone(),
        absolute,
        &clock,
    ))?;
    let append_deadline = ok!(QueryDeadline::after(clock.now(), Duration::from_secs(1)));
    ok!(clock.advance(Duration::from_secs(1)));
    must_err(
        ledger.append(&clone, LedgerFact::CloneIssued, append_deadline),
        LedgerError::Expired,
        "expired append",
    )?;
    let view = ok!(query_dyn(&ledger, &clone, &clock)?);
    if !view.facts().is_empty() {
        return Err("expired append wrote a fact".into());
    }
    Ok(())
}

fn fake_runner_records_scripted_faults_and_bounds() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut runner = FakeTartRunner::new(clock.clone());
    runner.push(ok!(ScriptedReply::success(b"ok".to_vec())));
    runner.push(ok!(ScriptedReply::failed(b"no".to_vec())));
    runner.push(ScriptedReply::timed_out());
    runner.push(ok!(ScriptedReply::malformed(vec![0xff, 0xfe])));
    runner.push(ScriptedReply::overlong());
    let clone = ok!(CloneId::new("clone-3"));
    let deadline = ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP));
    let body = ok!(runner.status(&clone, deadline));
    if body.as_bytes() != b"ok" {
        return Err("success body mismatch".into());
    }
    match runner.status(&clone, deadline) {
        Err(CallError::Failed(body)) if body.as_bytes() == b"no" => {}
        other => return Err(format!("failed: {other:?}")),
    }
    must_err(
        runner.status(&clone, deadline),
        CallError::TimedOut,
        "timeout",
    )?;
    match runner.status(&clone, deadline) {
        Err(CallError::Malformed(body)) if body.as_bytes() == [0xff, 0xfe] => {}
        other => return Err(format!("malformed: {other:?}")),
    }
    must_err(
        runner.status(&clone, deadline),
        CallError::OutputTooLong,
        "overlong",
    )?;
    must_err(
        runner.status(&clone, deadline),
        CallError::ScriptExhausted,
        "exhausted",
    )?;
    let outcomes = [
        ScriptOutcome::Success,
        ScriptOutcome::Failed,
        ScriptOutcome::TimedOut,
        ScriptOutcome::Malformed,
        ScriptOutcome::Overlong,
        ScriptOutcome::ScriptExhausted,
    ];
    if runner.trace().len() != outcomes.len() {
        return Err("trace length mismatch".into());
    }
    for (entry, outcome) in runner.trace().iter().zip(outcomes) {
        match &entry.command {
            TartCommand::Status { clone: recorded } if recorded == &clone => {}
            _ => return Err("status trace recorded another command".into()),
        }
        if entry.outcome != outcome || entry.deadline != deadline.at() {
            return Err("trace outcome or deadline mismatch".into());
        }
    }
    Ok(())
}

fn expired_runner_call_does_not_consume_the_script() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut runner = FakeTartRunner::new(clock.clone());
    runner.push(ok!(ScriptedReply::success(b"kept".to_vec())));
    let clone = ok!(CloneId::new("clone-4"));
    let stale = ok!(QueryDeadline::after(clock.now(), Duration::from_secs(1)));
    ok!(clock.advance(Duration::from_secs(1)));
    must_err(runner.status(&clone, stale), CallError::Expired, "expired")?;
    let live = ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP));
    let body = ok!(runner.status(&clone, live));
    if body.as_bytes() != b"kept" {
        return Err("expired call consumed the script".into());
    }
    let outcomes: Vec<ScriptOutcome> = runner.trace().iter().map(|entry| entry.outcome).collect();
    if outcomes != [ScriptOutcome::Expired, ScriptOutcome::Success] {
        return Err(format!("expiry trace: {outcomes:?}"));
    }
    Ok(())
}

fn command_trace_is_fixed_ops_with_required_softnet() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut ledger = FakeLedger::new(clock.clone());
    let mut runner = FakeTartRunner::new(clock.clone());
    for _ in 0..6 {
        runner.push(ok!(ScriptedReply::success(b"y".to_vec())));
    }
    let run = ok!(RunId::new("run-5"));
    let clone = ok!(CloneId::new("clone-5"));
    let absolute = ok!(AttemptDeadline::after(clock.now(), Duration::from_secs(90)));
    let permit = noted(reserve_dyn(
        &mut ledger,
        run,
        clone.clone(),
        absolute,
        &clock,
    ))?;
    let flags = ok!(SoftnetFlags::try_new(&[Ipv4Addr::new(1, 1, 1, 1)]));
    {
        let runner: &mut dyn TartRunner = &mut runner;
        ok!(runner.clone_vm(
            permit,
            GoldenImage::MacosS2Eval,
            ok!(CloneDeadline::after(clock.now(), Duration::from_secs(30))),
        ));
        ok!(runner.start(
            &clone,
            &flags,
            ok!(StartDeadline::after(clock.now(), StartDeadline::CAP)),
        ));
        ok!(runner.stop(
            &clone,
            ok!(StopDeadline::after(clock.now(), Duration::from_secs(1))),
        ));
        ok!(runner.delete(
            &clone,
            ok!(DeleteDeadline::after(clock.now(), DeleteDeadline::CAP)),
        ));
        ok!(runner.list(ok!(QueryDeadline::after(
            clock.now(),
            Duration::from_millis(5)
        ))));
        ok!(runner.status(
            &clone,
            ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP)),
        ));
    }
    let commands: Vec<TartCommand> = runner
        .trace()
        .iter()
        .map(|entry| entry.command.clone())
        .collect();
    let expected = vec![
        TartCommand::Clone {
            golden: GoldenImage::MacosS2Eval,
            clone: clone.clone(),
        },
        TartCommand::Start {
            clone: clone.clone(),
            softnet: flags,
        },
        TartCommand::Stop {
            clone: clone.clone(),
        },
        TartCommand::Delete {
            clone: clone.clone(),
        },
        TartCommand::List,
        TartCommand::Status { clone },
    ];
    if commands != expected {
        return Err(format!("command trace: {commands:?}"));
    }
    if runner
        .trace()
        .iter()
        .any(|entry| entry.deadline <= MonoInstant::EPOCH)
    {
        return Err("a call recorded a deadline that is not after the epoch".into());
    }
    Ok(())
}

fn guest_channel_records_versioned_ops_without_a_transport() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut guest = FakeGuestChannel::new(clock.clone());
    if NonZeroU64::new(0).is_some() {
        return Err("lease sequence accepted zero".into());
    }
    let seq = ok!(NonZeroU64::new(1).ok_or("lease seq"));
    for _ in 0..7 {
        guest.push(ok!(ScriptedReply::success(b"g".to_vec())));
    }
    {
        let guest: &mut dyn GuestChannel = &mut guest;
        if guest.protocol_version() != 1 || GUEST_CHANNEL_VERSION != 1 {
            return Err("guest version drifted".into());
        }
        ok!(guest.inventory(ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP))));
        ok!(guest.probe(
            Probe::LanDenied,
            ok!(ProbeDeadline::after(clock.now(), ProbeDeadline::CAP)),
        ));
        ok!(guest.bootstrap(ok!(StageDeadline::after(
            clock.now(),
            Duration::from_secs(1)
        ))));
        ok!(guest.login(ok!(LoginDeadline::after(
            clock.now(),
            Duration::from_secs(1)
        ))));
        ok!(guest.run(ok!(RunDeadline::after(clock.now(), Duration::from_secs(1)))));
        ok!(guest.lease(
            seq,
            ok!(LeaseDeadline::after(clock.now(), LeaseDeadline::CAP)),
        ));
        ok!(guest.revoke(ok!(RevokeDeadline::after(clock.now(), RevokeDeadline::CAP))));
    }
    let commands: Vec<GuestCommand> = guest
        .trace()
        .iter()
        .map(|entry| entry.command.clone())
        .collect();
    let expected = vec![
        GuestCommand::Inventory,
        GuestCommand::Probe(Probe::LanDenied),
        GuestCommand::Bootstrap,
        GuestCommand::Login,
        GuestCommand::Run,
        GuestCommand::Lease { seq },
        GuestCommand::Revoke,
    ];
    if commands != expected {
        return Err(format!("guest trace: {commands:?}"));
    }
    if guest
        .trace()
        .iter()
        .any(|entry| entry.outcome != ScriptOutcome::Success)
    {
        return Err("guest script was not consumed in order".into());
    }
    Ok(())
}

fn sources_have_no_process_launch_or_open_ended_wait() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/disposable/tart");
    let mut files = Vec::new();
    walk(&root, &mut files)?;
    if files.len() < 8 {
        return Err(format!("tart sources missing: {}", files.len()));
    }
    // Spellings only. This list does not prove every Rust alias is absent.
    let forbidden = [
        "Command::new",
        "std::process",
        "process::Command",
        "tokio::process",
        "AdmittedLaunch",
        "std::thread",
        ".spawn(",
        "unsafe",
        "env!(",
        "option_env!",
        "std::env",
        "std::fs",
        "OpenOptions",
        "include!",
        "include_str!",
        "include_bytes!",
        "derive(Default)",
        "Duration::MAX",
        "tart-disposable",
    ];
    let mut permit_literals = 0;
    for path in &files {
        let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
        for needle in forbidden {
            if text.contains(needle) {
                return Err(format!("{} contains {needle}", path.display()));
            }
        }
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("impl Default for ") && trimmed != "impl Default for FakeClock {"
            {
                return Err(format!(
                    "{} defaults a type other than FakeClock: {trimmed}",
                    path.display()
                ));
            }
        }
        if path.ends_with("ledger.rs") {
            permit_literals += permit_constructions(&text);
        } else if text.contains("ClonePermit {") {
            return Err(format!("{} constructs a permit", path.display()));
        }
    }
    if permit_literals != 1 {
        return Err(format!("permit constructions: {permit_literals}"));
    }
    Ok(())
}

fn permit_constructions(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.contains("Ok(ClonePermit {") || trimmed == "ClonePermit {"
        })
        .count()
}

fn walk(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|err| err.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            walk(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn reserve_dyn(
    ledger: &mut dyn AttemptLedger,
    run: RunId,
    clone: CloneId,
    absolute: AttemptDeadline,
    clock: &dyn HostClock,
) -> Result<gcu_eval::disposable::tart::ClonePermit, LedgerError> {
    let call =
        QueryDeadline::after(clock.now(), QueryDeadline::CAP).map_err(|_| LedgerError::Expired)?;
    ledger.reserve(run, clone, absolute, call)
}

fn query_dyn(
    ledger: &dyn AttemptLedger,
    clone: &CloneId,
    clock: &dyn HostClock,
) -> Result<Result<gcu_eval::disposable::tart::Reservation, LedgerError>, String> {
    let call = QueryDeadline::after(clock.now(), QueryDeadline::CAP)
        .map_err(|err| format!("query deadline: {err:?}"))?;
    Ok(ledger.query(clone, call))
}

fn noted<T, E: std::fmt::Debug>(result: Result<T, E>) -> Result<T, String> {
    result.map_err(|err| format!("{err:?}"))
}

fn must_err<T: std::fmt::Debug, E: PartialEq + std::fmt::Debug>(
    result: Result<T, E>,
    expected: E,
    what: &str,
) -> Result<(), String> {
    match result {
        Err(got) if got == expected => Ok(()),
        other => Err(format!("{what}: {other:?}")),
    }
}
