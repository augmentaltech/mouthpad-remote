//! macOS virtual keycodes (`kVK_*`) to USB HID keyboard usages.

pub const KVK_P: u16 = 35;
pub const KVK_CAPS_LOCK: u16 = 57;
pub const HID_CAPS_LOCK: u8 = 0x39;

/// A modifier key: its HID modifier-byte bit, and the device-dependent
/// `CGEventFlags` bit that is set while that specific side is held.
pub struct Modifier {
    pub hid_bit: u8,
    pub device_flag: u64,
}

pub fn modifier(keycode: u16) -> Option<Modifier> {
    let (hid_bit, device_flag) = match keycode {
        59 => (0x01, 0x0000_0001), // left control
        56 => (0x02, 0x0000_0002), // left shift
        58 => (0x04, 0x0000_0020), // left option
        55 => (0x08, 0x0000_0008), // left command
        62 => (0x10, 0x0000_2000), // right control
        60 => (0x20, 0x0000_0004), // right shift
        61 => (0x40, 0x0000_0040), // right option
        54 => (0x80, 0x0000_0010), // right command
        _ => return None,
    };
    Some(Modifier { hid_bit, device_flag })
}

pub fn hid_usage(keycode: u16) -> Option<u8> {
    Some(match keycode {
        0 => 0x04,   // A
        11 => 0x05,  // B
        8 => 0x06,   // C
        2 => 0x07,   // D
        14 => 0x08,  // E
        3 => 0x09,   // F
        5 => 0x0A,   // G
        4 => 0x0B,   // H
        34 => 0x0C,  // I
        38 => 0x0D,  // J
        40 => 0x0E,  // K
        37 => 0x0F,  // L
        46 => 0x10,  // M
        45 => 0x11,  // N
        31 => 0x12,  // O
        35 => 0x13,  // P
        12 => 0x14,  // Q
        15 => 0x15,  // R
        1 => 0x16,   // S
        17 => 0x17,  // T
        32 => 0x18,  // U
        9 => 0x19,   // V
        13 => 0x1A,  // W
        7 => 0x1B,   // X
        16 => 0x1C,  // Y
        6 => 0x1D,   // Z
        18 => 0x1E,  // 1
        19 => 0x1F,  // 2
        20 => 0x20,  // 3
        21 => 0x21,  // 4
        23 => 0x22,  // 5
        22 => 0x23,  // 6
        26 => 0x24,  // 7
        28 => 0x25,  // 8
        25 => 0x26,  // 9
        29 => 0x27,  // 0
        36 => 0x28,  // Return
        53 => 0x29,  // Escape
        51 => 0x2A,  // Delete (backspace)
        48 => 0x2B,  // Tab
        49 => 0x2C,  // Space
        27 => 0x2D,  // -
        24 => 0x2E,  // =
        33 => 0x2F,  // [
        30 => 0x30,  // ]
        42 => 0x31,  // backslash
        41 => 0x33,  // ;
        39 => 0x34,  // '
        50 => 0x35,  // `
        43 => 0x36,  // ,
        47 => 0x37,  // .
        44 => 0x38,  // /
        122 => 0x3A, // F1
        120 => 0x3B, // F2
        99 => 0x3C,  // F3
        118 => 0x3D, // F4
        96 => 0x3E,  // F5
        97 => 0x3F,  // F6
        98 => 0x40,  // F7
        100 => 0x41, // F8
        101 => 0x42, // F9
        109 => 0x43, // F10
        103 => 0x44, // F11
        111 => 0x45, // F12
        114 => 0x49, // Help / Insert
        115 => 0x4A, // Home
        116 => 0x4B, // Page Up
        117 => 0x4C, // Forward Delete
        119 => 0x4D, // End
        121 => 0x4E, // Page Down
        124 => 0x4F, // Right
        123 => 0x50, // Left
        125 => 0x51, // Down
        126 => 0x52, // Up
        71 => 0x53,  // Keypad Clear / Num Lock
        75 => 0x54,  // Keypad /
        67 => 0x55,  // Keypad *
        78 => 0x56,  // Keypad -
        69 => 0x57,  // Keypad +
        76 => 0x58,  // Keypad Enter
        83 => 0x59,  // Keypad 1
        84 => 0x5A,  // Keypad 2
        85 => 0x5B,  // Keypad 3
        86 => 0x5C,  // Keypad 4
        87 => 0x5D,  // Keypad 5
        88 => 0x5E,  // Keypad 6
        89 => 0x5F,  // Keypad 7
        91 => 0x60,  // Keypad 8
        92 => 0x61,  // Keypad 9
        82 => 0x62,  // Keypad 0
        65 => 0x63,  // Keypad .
        10 => 0x64,  // ISO section
        81 => 0x67,  // Keypad =
        105 => 0x68, // F13
        107 => 0x69, // F14
        113 => 0x6A, // F15
        106 => 0x6B, // F16
        64 => 0x6C,  // F17
        79 => 0x6D,  // F18
        80 => 0x6E,  // F19
        90 => 0x6F,  // F20
        74 => 0x7F,  // Mute
        72 => 0x80,  // Volume Up
        73 => 0x81,  // Volume Down
        95 => 0x85,  // JIS keypad comma
        94 => 0x87,  // JIS underscore (International1)
        93 => 0x89,  // JIS yen (International3)
        104 => 0x90, // JIS kana (Lang1)
        102 => 0x91, // JIS eisu (Lang2)
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_arrows_map_to_hid_usages() {
        assert_eq!(hid_usage(0), Some(0x04));
        assert_eq!(hid_usage(KVK_P), Some(0x13));
        assert_eq!(hid_usage(29), Some(0x27));
        assert_eq!(hid_usage(126), Some(0x52));
        assert_eq!(hid_usage(63), None);
    }

    #[test]
    fn modifiers_are_not_ordinary_keys() {
        for keycode in [54, 55, 56, 58, 59, 60, 61, 62] {
            assert!(modifier(keycode).is_some());
            assert_eq!(hid_usage(keycode), None);
        }
    }
}
