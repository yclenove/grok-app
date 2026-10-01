//! Read the server's mapping rather than assuming that Mod2 is NumLock.
//! Latched lock state is allowed; physically held keys are checked separately.
use crate::client::{err, Client};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;

impl Client {
    pub fn lock_modifier_mask(&self) -> Result<u16, String> {
        let modifiers = self
            .conn
            .get_modifier_mapping()
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let setup = self.conn.setup();
        let count = setup
            .max_keycode
            .checked_sub(setup.min_keycode)
            .and_then(|n| n.checked_add(1))
            .ok_or("invalid native keycode range")?;
        let keys = self
            .conn
            .get_keyboard_mapping(setup.min_keycode, count)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        lock_mask(
            &modifiers.keycodes,
            setup.min_keycode,
            keys.keysyms_per_keycode,
            &keys.keysyms,
        )
    }
}

fn lock_mask(modifiers: &[u8], min: u8, width: u8, symbols: &[u32]) -> Result<u16, String> {
    if modifiers.is_empty() {
        return Ok(0);
    }
    if !modifiers.len().is_multiple_of(8)
        || width == 0
        || !symbols.len().is_multiple_of(usize::from(width))
    {
        return Err("invalid native modifier/keyboard mapping".into());
    }
    let mut mask = 0;
    for (index, codes) in modifiers.chunks_exact(modifiers.len() / 8).enumerate() {
        let mut assigned = false;
        let mut only_locks = true;
        for code in codes.iter().copied().filter(|code| *code != 0) {
            let offset = code
                .checked_sub(min)
                .map(|n| usize::from(n) * usize::from(width))
                .ok_or("modifier keycode is below native mapping range")?;
            let primary = symbols
                .get(offset)
                .ok_or("modifier keycode is outside native mapping")?;
            assigned = true;
            // CapsLock / NumLock / ScrollLock only. ShiftLock and groups shared
            // with Shift/Ctrl/Alt/Super still block input. No global remapping.
            only_locks &= matches!(primary, 0xffe5 | 0xff7f | 0xff14);
        }
        if assigned && only_locks {
            mask |= 1 << index;
        }
    }
    Ok(mask)
}

#[cfg(test)]
mod tests {
    use super::lock_mask;

    #[test]
    fn maps_only_actual_lock_groups_without_guessing_mod2() {
        // Shift, CapsLock, Ctrl, Alt, NumLock, Super, ScrollLock, ShiftLock.
        let symbols = [
            0xffe1, 0xffe5, 0xffe3, 0xffe9, 0xff7f, 0xffeb, 0xff14, 0xffe6,
        ];
        assert_eq!(
            lock_mask(&[8, 9, 10, 11, 12, 13, 14, 15], 8, 1, &symbols).unwrap(),
            0x52
        );
        assert_eq!(
            lock_mask(&[8, 9, 10, 11, 13, 12, 14, 15], 8, 1, &symbols).unwrap(),
            0x62
        );
    }

    #[test]
    fn shared_modifier_group_and_empty_group_are_not_lock_only() {
        let symbols = [0xffe5, 0xffe1, 0xff7f];
        let mut modifiers = [0; 16];
        modifiers[2] = 8;
        modifiers[3] = 9; // Lock shared with Shift must remain guarded.
        modifiers[8] = 10;
        assert_eq!(lock_mask(&modifiers, 8, 1, &symbols).unwrap(), 0x10);
        assert_eq!(lock_mask(&[0; 8], 8, 1, &symbols).unwrap(), 0);
        assert_eq!(lock_mask(&[], 8, 1, &symbols).unwrap(), 0);
    }

    #[test]
    fn validates_keycode_bounds_and_mapping_shape() {
        assert!(lock_mask(&[8; 7], 8, 1, &[0xffe5]).is_err());
        assert!(lock_mask(&[8; 8], 8, 0, &[0xffe5]).is_err());
        assert!(lock_mask(&[8; 8], 8, 2, &[0xffe5]).is_err());
        assert!(lock_mask(&[7; 8], 8, 1, &[0xffe5]).is_err());
        assert!(lock_mask(&[9; 8], 8, 1, &[0xffe5]).is_err());
    }

    #[test]
    fn uses_primary_symbol_not_a_lock_on_another_level() {
        let symbols = [0xffe1, 0xffe5, 0xff7f, 0];
        let modifiers = [8, 0, 0, 0, 9, 0, 0, 0];
        assert_eq!(lock_mask(&modifiers, 8, 2, &symbols).unwrap(), 0x10);
    }
}
