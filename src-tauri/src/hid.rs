use mouthpad_proto::mouthware_message::{
    hid_loopback, mouthware_message::MessageBody, ActionType, HidLoopback, KeystrokeType,
    MouseKeyboardAction, MouthwareMessage,
};
use prost::Message as _;

/// The firmware packs each movement axis into a signed 12-bit HID field.
const MAX_MOVE: i32 = 2047;
const MAX_KEYS_PER_REPORT: usize = 6;
/// The firmware sends wheel and pan as signed bytes.
const MAX_SCROLL: i32 = 127;
/// Trackpad scroll arrives as continuous pixel deltas; this many pixels make
/// one wheel tick on the remote host.
const PIXELS_PER_SCROLL_TICK: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    MouseMove { dx: i32, dy: i32 },
    /// Wheel ticks in HID convention: positive `vertical` is up, positive
    /// `horizontal` is right.
    Scroll { vertical: i32, horizontal: i32 },
    Button { button: MouseButton, down: bool },
    Key { usage: u8, down: bool },
    Modifier { bit: u8, down: bool },
    ReleaseAll,
}

/// Merges runs of consecutive moves (and of scrolls) so a slow link sends one
/// report per burst instead of falling behind the pointer.
pub fn coalesce(events: Vec<InputEvent>) -> Vec<InputEvent> {
    let mut out: Vec<InputEvent> = Vec::with_capacity(events.len());
    for ev in events {
        match (out.last_mut(), ev) {
            (Some(InputEvent::MouseMove { dx, dy }), InputEvent::MouseMove { dx: ndx, dy: ndy }) => {
                *dx += ndx;
                *dy += ndy;
            }
            (
                Some(InputEvent::Scroll { vertical, horizontal }),
                InputEvent::Scroll { vertical: nv, horizontal: nh },
            ) => {
                *vertical += nv;
                *horizontal += nh;
            }
            _ => out.push(ev),
        }
    }
    out
}

/// Turns continuous (trackpad) pixel deltas into whole wheel ticks, carrying
/// the fractional remainder so slow scrolls still add up.
#[derive(Default)]
pub struct ScrollAccumulator {
    vertical: f64,
    horizontal: f64,
}

impl ScrollAccumulator {
    pub fn add_pixels(&mut self, vertical: f64, horizontal: f64) -> Option<InputEvent> {
        self.vertical += vertical;
        self.horizontal += horizontal;
        let v = (self.vertical / PIXELS_PER_SCROLL_TICK).trunc();
        let h = (self.horizontal / PIXELS_PER_SCROLL_TICK).trunc();
        self.vertical -= v * PIXELS_PER_SCROLL_TICK;
        self.horizontal -= h * PIXELS_PER_SCROLL_TICK;
        (v != 0.0 || h != 0.0).then_some(InputEvent::Scroll { vertical: v as i32, horizontal: h as i32 })
    }
}

/// Turns host input transitions into HIDLoopback actions, tracking what the
/// device currently reports as held.
///
/// The firmware's keyboard path only ORs modifiers into its held set
/// (`KEYSTROKE_HOLD`) or zeroes it (`AT_CLEAR_KEYSTROKES`), so releasing one
/// modifier while another stays down takes a clear followed by a re-hold.
#[derive(Default)]
pub struct HidTranslator {
    mods: u8,
    keys: Vec<u8>,
    device_mods: u8,
    left: bool,
    right: bool,
}

impl HidTranslator {
    pub fn translate(&mut self, ev: InputEvent) -> Vec<MouseKeyboardAction> {
        match ev {
            InputEvent::MouseMove { dx, dy } => move_actions(dx, dy),
            InputEvent::Scroll { vertical, horizontal } => scroll_actions(vertical, horizontal),
            InputEvent::Button { button, down } => self.button(button, down).into_iter().collect(),
            InputEvent::Key { usage, down } => {
                let changed = if down {
                    !self.keys.contains(&usage) && {
                        self.keys.push(usage);
                        true
                    }
                } else {
                    let before = self.keys.len();
                    self.keys.retain(|&k| k != usage);
                    self.keys.len() != before
                };
                if changed {
                    self.keyboard_report()
                } else {
                    vec![]
                }
            }
            InputEvent::Modifier { bit, down } => {
                let mods = if down { self.mods | bit } else { self.mods & !bit };
                if mods == self.mods {
                    return vec![];
                }
                self.mods = mods;
                self.keyboard_report()
            }
            InputEvent::ReleaseAll => {
                let mut out = Vec::new();
                out.extend(self.button(MouseButton::Left, false));
                out.extend(self.button(MouseButton::Right, false));
                self.mods = 0;
                self.keys.clear();
                self.device_mods = 0;
                out.push(keyboard_action(ActionType::AtClearKeystrokes, None, 0, &[]));
                out
            }
        }
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Option<MouseKeyboardAction> {
        let held = match button {
            MouseButton::Left => &mut self.left,
            MouseButton::Right => &mut self.right,
        };
        if *held == down {
            return None;
        }
        *held = down;
        let action_type = match (button, down) {
            (MouseButton::Left, true) => ActionType::AtLeftButtonPress,
            (MouseButton::Left, false) => ActionType::AtLeftButtonRelease,
            (MouseButton::Right, true) => ActionType::AtRightButtonPress,
            (MouseButton::Right, false) => ActionType::AtRightButtonRelease,
        };
        Some(MouseKeyboardAction {
            action_type: action_type as i32,
            ..Default::default()
        })
    }

    fn keyboard_report(&mut self) -> Vec<MouseKeyboardAction> {
        let keys = &self.keys[..self.keys.len().min(MAX_KEYS_PER_REPORT)];
        let hold = |mods| keyboard_action(ActionType::AtKeystrokes, Some(KeystrokeType::KeystrokeHold), mods, keys);
        let out = if self.device_mods & !self.mods == 0 {
            vec![hold(self.mods)]
        } else if self.mods == 0 {
            vec![keyboard_action(ActionType::AtClearKeystrokes, None, 0, keys)]
        } else {
            vec![
                keyboard_action(ActionType::AtClearKeystrokes, None, 0, &[]),
                hold(self.mods),
            ]
        };
        self.device_mods = self.mods;
        out
    }
}

fn keyboard_action(
    action_type: ActionType,
    keystroke_type: Option<KeystrokeType>,
    mods: u8,
    keys: &[u8],
) -> MouseKeyboardAction {
    MouseKeyboardAction {
        action_type: action_type as i32,
        keycodes: keys.iter().map(|&k| k as i32).collect(),
        modifier_keys: (0..8).map(|i| 1u8 << i).filter(|b| mods & b != 0).map(i32::from).collect(),
        keystroke_type: keystroke_type.map(|k| k as i32),
        ..Default::default()
    }
}

fn move_actions(mut dx: i32, mut dy: i32) -> Vec<MouseKeyboardAction> {
    let mut out = Vec::new();
    while dx != 0 || dy != 0 {
        let sx = dx.clamp(-MAX_MOVE, MAX_MOVE);
        let sy = dy.clamp(-MAX_MOVE, MAX_MOVE);
        out.push(MouseKeyboardAction {
            action_type: ActionType::AtCursorMove as i32,
            x_cursor_movement: Some(sx),
            y_cursor_movement: Some(sy),
            ..Default::default()
        });
        dx -= sx;
        dy -= sy;
    }
    out
}

fn scroll_actions(mut vertical: i32, mut horizontal: i32) -> Vec<MouseKeyboardAction> {
    let mut out = Vec::new();
    while vertical != 0 || horizontal != 0 {
        let v = vertical.clamp(-MAX_SCROLL, MAX_SCROLL);
        let h = horizontal.clamp(-MAX_SCROLL, MAX_SCROLL);
        let action_type = match (v.signum(), h.signum()) {
            (1, _) => ActionType::AtScrollUp,
            (-1, _) => ActionType::AtScrollDown,
            (_, -1) => ActionType::AtScrollLeft,
            _ => ActionType::AtScrollRight,
        };
        out.push(MouseKeyboardAction {
            action_type: action_type as i32,
            vertical_scroll: Some(v),
            horizontal_scroll: Some(h),
            ..Default::default()
        });
        vertical -= v;
        horizontal -= h;
    }
    out
}

pub fn encode(action: MouseKeyboardAction, message_index: i32) -> Vec<u8> {
    MouthwareMessage {
        message_index,
        message_body: Some(MessageBody::HidLoopback(HidLoopback {
            input: Some(hid_loopback::Input::KeyboardInput(action)),
        })),
    }
    .encode_to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCMD: u8 = 0x08;
    const LSHIFT: u8 = 0x02;
    const KEY_C: u8 = 0x06;

    fn summary(actions: &[MouseKeyboardAction]) -> Vec<(i32, Vec<i32>, Vec<i32>)> {
        actions
            .iter()
            .map(|a| (a.action_type, a.modifier_keys.clone(), a.keycodes.clone()))
            .collect()
    }

    const HOLD: i32 = ActionType::AtKeystrokes as i32;
    const CLEAR: i32 = ActionType::AtClearKeystrokes as i32;

    #[test]
    fn shortcut_holds_modifier_then_key_then_releases_cleanly() {
        let mut t = HidTranslator::default();
        assert_eq!(summary(&t.translate(InputEvent::Modifier { bit: LCMD, down: true })), vec![(HOLD, vec![8], vec![])]);
        assert_eq!(summary(&t.translate(InputEvent::Key { usage: KEY_C, down: true })), vec![(HOLD, vec![8], vec![6])]);
        assert_eq!(summary(&t.translate(InputEvent::Key { usage: KEY_C, down: false })), vec![(HOLD, vec![8], vec![])]);
        assert_eq!(summary(&t.translate(InputEvent::Modifier { bit: LCMD, down: false })), vec![(CLEAR, vec![], vec![])]);
    }

    #[test]
    fn releasing_one_of_two_modifiers_clears_then_reholds() {
        let mut t = HidTranslator::default();
        t.translate(InputEvent::Modifier { bit: LCMD, down: true });
        t.translate(InputEvent::Modifier { bit: LSHIFT, down: true });
        t.translate(InputEvent::Key { usage: KEY_C, down: true });
        assert_eq!(
            summary(&t.translate(InputEvent::Modifier { bit: LSHIFT, down: false })),
            vec![(CLEAR, vec![], vec![]), (HOLD, vec![8], vec![6])]
        );
    }

    #[test]
    fn hold_uses_keystroke_hold_so_firmware_sends_no_release() {
        let mut t = HidTranslator::default();
        let a = t.translate(InputEvent::Key { usage: KEY_C, down: true });
        assert_eq!(a[0].keystroke_type, Some(KeystrokeType::KeystrokeHold as i32));
    }

    #[test]
    fn duplicate_key_down_and_unknown_key_up_send_nothing() {
        let mut t = HidTranslator::default();
        t.translate(InputEvent::Key { usage: KEY_C, down: true });
        assert!(t.translate(InputEvent::Key { usage: KEY_C, down: true }).is_empty());
        assert!(t.translate(InputEvent::Key { usage: 0x07, down: false }).is_empty());
    }

    #[test]
    fn report_carries_at_most_six_keys() {
        let mut t = HidTranslator::default();
        let mut last = vec![];
        for usage in 4..12 {
            last = t.translate(InputEvent::Key { usage, down: true });
        }
        assert_eq!(last[0].keycodes, vec![4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn button_release_without_press_is_dropped() {
        let mut t = HidTranslator::default();
        assert!(t.translate(InputEvent::Button { button: MouseButton::Left, down: false }).is_empty());
        let press = t.translate(InputEvent::Button { button: MouseButton::Right, down: true });
        assert_eq!(press[0].action_type, ActionType::AtRightButtonPress as i32);
    }

    #[test]
    fn release_all_lets_go_of_held_buttons_and_keys() {
        let mut t = HidTranslator::default();
        t.translate(InputEvent::Button { button: MouseButton::Left, down: true });
        t.translate(InputEvent::Modifier { bit: LCMD, down: true });
        let types: Vec<i32> = t.translate(InputEvent::ReleaseAll).iter().map(|a| a.action_type).collect();
        assert_eq!(types, vec![ActionType::AtLeftButtonRelease as i32, CLEAR]);
        assert_eq!(summary(&t.translate(InputEvent::Modifier { bit: LSHIFT, down: true })), vec![(HOLD, vec![2], vec![])]);
    }

    #[test]
    fn large_moves_split_into_12_bit_reports() {
        let moves = move_actions(5000, -10);
        let xs: Vec<_> = moves.iter().map(|a| (a.x_cursor_movement, a.y_cursor_movement)).collect();
        assert_eq!(xs, vec![(Some(2047), Some(-10)), (Some(2047), Some(0)), (Some(906), Some(0))]);
    }

    #[test]
    fn scroll_carries_signed_amounts_with_a_matching_direction() {
        let a = scroll_actions(-3, 2);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].action_type, ActionType::AtScrollDown as i32);
        assert_eq!((a[0].vertical_scroll, a[0].horizontal_scroll), (Some(-3), Some(2)));
        assert_eq!(scroll_actions(0, -1)[0].action_type, ActionType::AtScrollLeft as i32);
    }

    #[test]
    fn large_scrolls_split_into_signed_byte_reports() {
        let v: Vec<_> = scroll_actions(300, 0).iter().map(|a| a.vertical_scroll).collect();
        assert_eq!(v, vec![Some(127), Some(127), Some(46)]);
    }

    #[test]
    fn trackpad_pixels_accumulate_into_ticks_keeping_the_remainder() {
        let mut acc = ScrollAccumulator::default();
        assert_eq!(acc.add_pixels(5.0, 0.0), None);
        assert_eq!(acc.add_pixels(8.0, 0.0), Some(InputEvent::Scroll { vertical: 1, horizontal: 0 }));
        assert_eq!(acc.add_pixels(11.0, 0.0), Some(InputEvent::Scroll { vertical: 1, horizontal: 0 }));
        assert_eq!(acc.add_pixels(-30.0, -24.0), Some(InputEvent::Scroll { vertical: -2, horizontal: -2 }));
    }

    #[test]
    fn coalesce_merges_only_adjacent_moves() {
        let click = InputEvent::Button { button: MouseButton::Left, down: true };
        let merged = coalesce(vec![
            InputEvent::MouseMove { dx: 1, dy: 2 },
            InputEvent::MouseMove { dx: 3, dy: -1 },
            click,
            InputEvent::MouseMove { dx: 5, dy: 5 },
        ]);
        assert_eq!(merged, vec![InputEvent::MouseMove { dx: 4, dy: 1 }, click, InputEvent::MouseMove { dx: 5, dy: 5 }]);
    }

    #[test]
    fn coalesce_merges_adjacent_scrolls() {
        let merged = coalesce(vec![
            InputEvent::Scroll { vertical: 1, horizontal: 0 },
            InputEvent::Scroll { vertical: 2, horizontal: -1 },
        ]);
        assert_eq!(merged, vec![InputEvent::Scroll { vertical: 3, horizontal: -1 }]);
    }

    #[test]
    fn encodes_as_hid_loopback_keyboard_input() {
        let mut t = HidTranslator::default();
        let action = t.translate(InputEvent::Key { usage: KEY_C, down: true }).remove(0);
        let msg = MouthwareMessage::decode(encode(action.clone(), 7).as_slice()).unwrap();
        assert_eq!(msg.message_index, 7);
        assert_eq!(
            msg.message_body,
            Some(MessageBody::HidLoopback(HidLoopback { input: Some(hid_loopback::Input::KeyboardInput(action)) }))
        );
    }
}
