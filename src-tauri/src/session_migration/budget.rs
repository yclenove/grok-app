//! Resource budgets. Changing a value requires fixture + contract table updates.

pub const READ_CONCURRENCY: usize = 2;
pub const PAGE_SIZE: usize = 50;
pub const PAGE_SIZE_MAX: usize = 200;
pub const DISCOVER_BUDGET: usize = 50_000;
/// First listing slice. Later finds are pumped into extra_candidates.
pub const JSON_RECORD_MAX_BYTES: usize = 32 * 1024 * 1024;
pub const JSON_NEST_MAX: usize = 64;
pub const SESSION_BODY_MAX_BYTES: u64 = 1024 * 1024 * 1024;
pub const BATCH_NEW_BYTES_MAX: u64 = 10 * 1024 * 1024 * 1024;
pub const IMAGE_MAX_BYTES: u64 = 40 * 1024 * 1024;
pub const ATTACHMENT_MAX_BYTES: u64 = 200 * 1024 * 1024;
pub const READ_PAGE_EVENTS: usize = 100;
pub const READ_PAGE_TEXT_BYTES: usize = 256 * 1024;
pub const PREVIEW_TTL_SECS: u64 = 30 * 60;
pub const DB_BUSY_MS: u64 = 1000;
pub const DB_BUSY_RETRIES: u32 = 1;
pub const SEARCH_LIMIT_MAX: usize = 20;
pub const READ_MAX_BYTES: usize = 32 * 1024;
pub const HANDOFF_SOFT_TOKEN_FRACTION: f64 = 0.10;
pub const HANDOFF_SOFT_TOKEN_CAP: usize = 12_000;
pub const DISK_RESERVE_BYTES: u64 = 512 * 1024 * 1024;

pub fn page_limit(requested: Option<usize>) -> usize {
    requested.unwrap_or(PAGE_SIZE).clamp(1, PAGE_SIZE_MAX)
}

pub fn search_limit(requested: Option<usize>) -> usize {
    requested.unwrap_or(20).clamp(1, SEARCH_LIMIT_MAX)
}

pub fn read_byte_limit(requested: Option<usize>) -> usize {
    requested.unwrap_or(READ_MAX_BYTES).clamp(1, READ_MAX_BYTES)
}

pub fn disk_reserve(estimated_new: u64) -> u64 {
    DISK_RESERVE_BYTES.max(estimated_new / 10)
}

/// Staging may proceed when the volume has the payload plus max(512 MiB, 10%).
pub fn disk_sufficient(available: u64, estimated_new: u64) -> bool {
    available >= estimated_new.saturating_add(disk_reserve(estimated_new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_page_and_search() {
        assert_eq!(page_limit(None), 50);
        assert_eq!(page_limit(Some(0)), 1);
        assert_eq!(page_limit(Some(9999)), 200);
        assert_eq!(search_limit(Some(99)), 20);
        assert_eq!(read_byte_limit(Some(99_000)), READ_MAX_BYTES);
        assert!(!disk_sufficient(0, 4096));
        assert!(!disk_sufficient(10 * 1024 * 1024, 100));
        assert!(disk_sufficient(600 * 1024 * 1024, 100));
        assert!(!disk_sufficient(
            6 * 1024 * 1024 * 1024,
            10 * 1024 * 1024 * 1024
        ));
        assert!(disk_sufficient(
            12 * 1024 * 1024 * 1024,
            10 * 1024 * 1024 * 1024
        ));
    }
}
