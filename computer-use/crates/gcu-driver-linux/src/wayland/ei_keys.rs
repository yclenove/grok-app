//! Bounded keymap decoding away from the reactor. No guessed US keyboard layout.
use gcu_core::driver::{DriverError, Result, DRIVER_DEADLINE};
use std::{collections::BTreeMap, os::fd::OwnedFd};

pub(super) async fn decode(fd: OwnedFd, size: u32) -> Result<BTreeMap<u32, (u32, bool)>> {
    tokio::time::timeout(
        DRIVER_DEADLINE,
        tokio::task::spawn_blocking(move || {
            use std::os::unix::fs::FileExt;
            use xkbcommon::xkb;
            if size == 0 || size > 1024 * 1024 {
                return Err(DriverError::CapabilityUnavailable);
            }
            let file = std::fs::File::from(fd);
            let meta = file
                .metadata()
                .map_err(|_| DriverError::CapabilityUnavailable)?;
            if !meta.is_file() || meta.len() < u64::from(size) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let mut bytes = vec![0; size as usize];
            file.read_exact_at(&mut bytes, 0)
                .map_err(|_| DriverError::CapabilityUnavailable)?;
            if bytes.last() == Some(&0) {
                bytes.pop();
            }
            let text = String::from_utf8(bytes).map_err(|_| DriverError::CapabilityUnavailable)?;
            let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
            let map = xkb::Keymap::new_from_string(
                &context,
                text,
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
            .ok_or(DriverError::CapabilityUnavailable)?;
            let mut keys = BTreeMap::new();
            let min = map.min_keycode().raw();
            let max = map.max_keycode().raw();
            if min < 8 || max > 767 + 8 {
                return Err(DriverError::CapabilityUnavailable);
            }
            for code in min..=max {
                for level in 0..=1 {
                    for sym in map.key_get_syms_by_level(xkb::Keycode::new(code), 0, level) {
                        keys.entry(sym.raw()).or_insert((code - 8, level == 1));
                    }
                }
            }
            Ok(keys)
        }),
    )
    .await
    .map_err(|_| DriverError::Timeout {
        maybe_executed: false,
    })?
    .map_err(|_| DriverError::Crashed {
        maybe_executed: false,
    })?
}
