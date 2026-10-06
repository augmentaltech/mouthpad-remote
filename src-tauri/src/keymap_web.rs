//! DOM `KeyboardEvent.code` values to USB HID keyboard usages. `code` names
//! the physical key regardless of layout, which is what a HID report carries.

use crate::hid::KeyAction;

fn modifier_bit(code: &str) -> Option<u8> {
    Some(match code {
        "ControlLeft" => 0x01,
        "ShiftLeft" => 0x02,
        "AltLeft" => 0x04,
        "MetaLeft" => 0x08,
        "ControlRight" => 0x10,
        "ShiftRight" => 0x20,
        "AltRight" => 0x40,
        "MetaRight" => 0x80,
        _ => return None,
    })
}

/// Letters, digits, F-keys and the numpad run in HID usage order.
fn usage_in_range(code: &str) -> Option<u8> {
    if let Some(letter) = code.strip_prefix("Key") {
        let &[c] = letter.as_bytes() else { return None };
        return c.is_ascii_uppercase().then(|| 0x04 + (c - b'A'));
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        let n: u8 = digit.parse().ok().filter(|n| *n <= 9)?;
        return Some(if n == 0 { 0x27 } else { 0x1E + n - 1 });
    }
    if let Some(digit) = code.strip_prefix("Numpad") {
        if let Ok(n) = digit.parse::<u8>() {
            return (n <= 9).then(|| if n == 0 { 0x62 } else { 0x59 + n - 1 });
        }
    }
    let n: u8 = code.strip_prefix('F')?.parse().ok()?;
    match n {
        1..=12 => Some(0x3A + n - 1),
        13..=24 => Some(0x68 + n - 13),
        _ => None,
    }
}

fn usage(code: &str) -> Option<u8> {
    if let Some(usage) = usage_in_range(code) {
        return Some(usage);
    }
    Some(match code {
        "Enter" => 0x28,
        "Escape" => 0x29,
        "Backspace" => 0x2A,
        "Tab" => 0x2B,
        "Space" => 0x2C,
        "Minus" => 0x2D,
        "Equal" => 0x2E,
        "BracketLeft" => 0x2F,
        "BracketRight" => 0x30,
        "Backslash" => 0x31,
        "Semicolon" => 0x33,
        "Quote" => 0x34,
        "Backquote" => 0x35,
        "Comma" => 0x36,
        "Period" => 0x37,
        "Slash" => 0x38,
        "CapsLock" => 0x39,
        "PrintScreen" => 0x46,
        "ScrollLock" => 0x47,
        "Pause" => 0x48,
        "Insert" => 0x49,
        "Home" => 0x4A,
        "PageUp" => 0x4B,
        "Delete" => 0x4C,
        "End" => 0x4D,
        "PageDown" => 0x4E,
        "ArrowRight" => 0x4F,
        "ArrowLeft" => 0x50,
        "ArrowDown" => 0x51,
        "ArrowUp" => 0x52,
        "NumLock" => 0x53,
        "NumpadDivide" => 0x54,
        "NumpadMultiply" => 0x55,
        "NumpadSubtract" => 0x56,
        "NumpadAdd" => 0x57,
        "NumpadEnter" => 0x58,
        "NumpadDecimal" => 0x63,
        "IntlBackslash" => 0x64,
        "ContextMenu" => 0x65,
        "NumpadEqual" => 0x67,
        "AudioVolumeMute" => 0x7F,
        "AudioVolumeUp" => 0x80,
        "AudioVolumeDown" => 0x81,
        "IntlRo" => 0x87,
        "IntlYen" => 0x89,
        _ => return None,
    })
}

pub fn classify(code: &str) -> Option<KeyAction> {
    modifier_bit(code).map(KeyAction::Modifier).or_else(|| usage(code).map(KeyAction::Key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_digits_follow_hid_order() {
        assert_eq!(classify("KeyA"), Some(KeyAction::Key(0x04)));
        assert_eq!(classify("KeyZ"), Some(KeyAction::Key(0x1D)));
        assert_eq!(classify("Digit1"), Some(KeyAction::Key(0x1E)));
        assert_eq!(classify("Digit0"), Some(KeyAction::Key(0x27)));
    }

    #[test]
    fn function_keys_and_numpad_follow_hid_order() {
        assert_eq!(classify("F1"), Some(KeyAction::Key(0x3A)));
        assert_eq!(classify("F12"), Some(KeyAction::Key(0x45)));
        assert_eq!(classify("F13"), Some(KeyAction::Key(0x68)));
        assert_eq!(classify("Numpad1"), Some(KeyAction::Key(0x59)));
        assert_eq!(classify("Numpad0"), Some(KeyAction::Key(0x62)));
        assert_eq!(classify("NumpadEnter"), Some(KeyAction::Key(0x58)));
    }

    #[test]
    fn modifiers_map_left_and_right_to_their_own_bits() {
        assert_eq!(classify("ControlLeft"), Some(KeyAction::Modifier(0x01)));
        assert_eq!(classify("MetaRight"), Some(KeyAction::Modifier(0x80)));
    }

    #[test]
    fn unknown_codes_are_not_forwarded() {
        assert_eq!(classify("KeyAA"), None);
        assert_eq!(classify("Key1"), None);
        assert_eq!(classify("F25"), None);
        assert_eq!(classify("Unidentified"), None);
    }
}
