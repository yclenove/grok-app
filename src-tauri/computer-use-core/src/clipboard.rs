//! Clipboard policy for Computer Use.
//!
//! Observations never include clipboard contents. If a desktop action must
//! paste, Host writes only the task text and restores the previous clipboard
//! when the sequence number shows no concurrent user copy.

/// Restore the saved clipboard only when every sequence bump is ours.
/// `our_writes` counts Host clipboard mutations between `seq_before` and
/// `seq_after` (not including a restore write that has not happened yet).
pub fn should_restore(seq_before: u32, seq_after: u32, our_writes: u32) -> bool {
    seq_after.wrapping_sub(seq_before) == our_writes
}

#[cfg(test)]
mod tests {
    use super::should_restore;

    #[test]
    fn restore_when_only_our_write_landed() {
        assert!(should_restore(10, 11, 1));
        assert!(should_restore(u32::MAX, 0, 1));
    }

    #[test]
    fn keep_user_copy_when_sequence_jumps() {
        assert!(!should_restore(10, 12, 1));
        assert!(!should_restore(10, 15, 1));
    }

    #[test]
    fn missing_or_unaccounted_writes_are_not_ownership() {
        assert!(!should_restore(10, 10, 1));
        assert!(!should_restore(10, 11, 2));
        assert!(should_restore(u32::MAX - 1, 1, 3));
        assert!(!should_restore(u32::MAX - 1, 2, 3));
    }
}
