//! Windows low-level keyboard hook codes to USB HID keyboard usages.

pub use crate::hid::KeyAction;

/// The low-level hook reports left/right-specific virtual keys for modifiers.
fn modifier_bit(vk: u32) -> Option<u8> {
    Some(match vk {
        0xA2 => 0x01, // left control
        0xA0 => 0x02, // left shift
        0xA4 => 0x04, // left alt
        0x5B => 0x08, // left Windows
        0xA3 => 0x10, // right control
        0xA1 => 0x20, // right shift
        0xA5 => 0x40, // right alt (AltGr)
        0x5C => 0x80, // right Windows
        _ => return None,
    })
}

/// Keys whose scan code is shared or unreliable, resolved by virtual key.
fn usage_by_virtual_key(vk: u32) -> Option<u8> {
    Some(match vk {
        0x13 => 0x48, // Pause
        0x90 => 0x53, // Num Lock
        0x2C => 0x46, // Print Screen
        _ => return None,
    })
}

/// Scan code set 1, as the hook reports it.
fn usage_by_scan_code(scan: u32) -> Option<u8> {
    Some(match scan {
        0x01 => 0x29, // Escape
        0x02 => 0x1E, // 1
        0x03 => 0x1F, // 2
        0x04 => 0x20, // 3
        0x05 => 0x21, // 4
        0x06 => 0x22, // 5
        0x07 => 0x23, // 6
        0x08 => 0x24, // 7
        0x09 => 0x25, // 8
        0x0A => 0x26, // 9
        0x0B => 0x27, // 0
        0x0C => 0x2D, // -
        0x0D => 0x2E, // =
        0x0E => 0x2A, // Backspace
        0x0F => 0x2B, // Tab
        0x10 => 0x14, // Q
        0x11 => 0x1A, // W
        0x12 => 0x08, // E
        0x13 => 0x15, // R
        0x14 => 0x17, // T
        0x15 => 0x1C, // Y
        0x16 => 0x18, // U
        0x17 => 0x0C, // I
        0x18 => 0x12, // O
        0x19 => 0x13, // P
        0x1A => 0x2F, // [
        0x1B => 0x30, // ]
        0x1C => 0x28, // Enter
        0x1E => 0x04, // A
        0x1F => 0x16, // S
        0x20 => 0x07, // D
        0x21 => 0x09, // F
        0x22 => 0x0A, // G
        0x23 => 0x0B, // H
        0x24 => 0x0D, // J
        0x25 => 0x0E, // K
        0x26 => 0x0F, // L
        0x27 => 0x33, // ;
        0x28 => 0x34, // '
        0x29 => 0x35, // `
        0x2B => 0x31, // backslash
        0x2C => 0x1D, // Z
        0x2D => 0x1B, // X
        0x2E => 0x06, // C
        0x2F => 0x19, // V
        0x30 => 0x05, // B
        0x31 => 0x11, // N
        0x32 => 0x10, // M
        0x33 => 0x36, // ,
        0x34 => 0x37, // .
        0x35 => 0x38, // /
        0x37 => 0x55, // Keypad *
        0x39 => 0x2C, // Space
        0x3A => 0x39, // Caps Lock
        0x3B => 0x3A, // F1
        0x3C => 0x3B, // F2
        0x3D => 0x3C, // F3
        0x3E => 0x3D, // F4
        0x3F => 0x3E, // F5
        0x40 => 0x3F, // F6
        0x41 => 0x40, // F7
        0x42 => 0x41, // F8
        0x43 => 0x42, // F9
        0x44 => 0x43, // F10
        0x46 => 0x47, // Scroll Lock
        0x47 => 0x5F, // Keypad 7
        0x48 => 0x60, // Keypad 8
        0x49 => 0x61, // Keypad 9
        0x4A => 0x56, // Keypad -
        0x4B => 0x5C, // Keypad 4
        0x4C => 0x5D, // Keypad 5
        0x4D => 0x5E, // Keypad 6
        0x4E => 0x57, // Keypad +
        0x4F => 0x59, // Keypad 1
        0x50 => 0x5A, // Keypad 2
        0x51 => 0x5B, // Keypad 3
        0x52 => 0x62, // Keypad 0
        0x53 => 0x63, // Keypad .
        0x56 => 0x64, // ISO \
        0x57 => 0x44, // F11
        0x58 => 0x45, // F12
        0x64 => 0x68, // F13
        0x65 => 0x69, // F14
        0x66 => 0x6A, // F15
        0x67 => 0x6B, // F16
        0x68 => 0x6C, // F17
        0x69 => 0x6D, // F18
        0x6A => 0x6E, // F19
        0x6B => 0x6F, // F20
        0x6C => 0x70, // F21
        0x6D => 0x71, // F22
        0x6E => 0x72, // F23
        0x76 => 0x73, // F24
        0x70 => 0x88, // Katakana/Hiragana
        0x73 => 0x87, // Ro
        0x79 => 0x8A, // Henkan
        0x7B => 0x8B, // Muhenkan
        0x7D => 0x89, // Yen
        _ => return None,
    })
}

/// Scan codes that arrive with the extended (E0) flag.
fn usage_by_extended_scan_code(scan: u32) -> Option<u8> {
    Some(match scan {
        0x1C => 0x58, // Keypad Enter
        0x35 => 0x54, // Keypad /
        0x47 => 0x4A, // Home
        0x48 => 0x52, // Up
        0x49 => 0x4B, // Page Up
        0x4B => 0x50, // Left
        0x4D => 0x4F, // Right
        0x4F => 0x4D, // End
        0x50 => 0x51, // Down
        0x51 => 0x4E, // Page Down
        0x52 => 0x49, // Insert
        0x53 => 0x4C, // Delete
        0x5D => 0x65, // Application / Menu
        0x20 => 0x7F, // Mute
        0x2E => 0x81, // Volume Down
        0x30 => 0x80, // Volume Up
        _ => return None,
    })
}

pub fn classify(vk: u32, scan: u32, extended: bool) -> Option<KeyAction> {
    if let Some(bit) = modifier_bit(vk) {
        return Some(KeyAction::Modifier(bit));
    }
    let usage = usage_by_virtual_key(vk).or_else(|| {
        if extended {
            usage_by_extended_scan_code(scan)
        } else {
            usage_by_scan_code(scan)
        }
    })?;
    Some(KeyAction::Key(usage))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VK_LSHIFT: u32 = 0xA0;
    const VK_RCONTROL: u32 = 0xA3;
    const VK_LWIN: u32 = 0x5B;
    const VK_RETURN: u32 = 0x0D;
    const VK_UP: u32 = 0x26;
    const VK_PAUSE: u32 = 0x13;
    const VK_NUMLOCK: u32 = 0x90;

    #[test]
    fn letters_digits_and_enter_map_by_scan_code() {
        assert_eq!(classify(0x41, 0x1E, false), Some(KeyAction::Key(0x04))); // A
        assert_eq!(classify(0x50, 0x19, false), Some(KeyAction::Key(0x13))); // P
        assert_eq!(classify(0x30, 0x0B, false), Some(KeyAction::Key(0x27))); // 0
        assert_eq!(classify(VK_RETURN, 0x1C, false), Some(KeyAction::Key(0x28)));
    }

    #[test]
    fn the_extended_flag_separates_navigation_keys_from_the_keypad() {
        assert_eq!(classify(VK_RETURN, 0x1C, true), Some(KeyAction::Key(0x58))); // keypad Enter
        assert_eq!(classify(VK_UP, 0x48, true), Some(KeyAction::Key(0x52))); // arrow Up
        assert_eq!(classify(0x68, 0x48, false), Some(KeyAction::Key(0x60))); // keypad 8
    }

    #[test]
    fn modifiers_map_left_and_right_to_their_own_bits() {
        assert_eq!(classify(VK_LSHIFT, 0x2A, false), Some(KeyAction::Modifier(0x02)));
        assert_eq!(classify(VK_RCONTROL, 0x1D, true), Some(KeyAction::Modifier(0x10)));
        assert_eq!(classify(VK_LWIN, 0x5B, true), Some(KeyAction::Modifier(0x08)));
    }

    #[test]
    fn pause_and_num_lock_share_a_scan_code_but_not_a_virtual_key() {
        assert_eq!(classify(VK_PAUSE, 0x45, false), Some(KeyAction::Key(0x48)));
        assert_eq!(classify(VK_NUMLOCK, 0x45, true), Some(KeyAction::Key(0x53)));
    }

    #[test]
    fn unknown_keys_are_not_forwarded() {
        assert_eq!(classify(0xFF, 0x7F, false), None);
    }
}
