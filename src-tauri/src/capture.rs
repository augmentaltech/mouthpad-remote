//! System-wide input capture. While the controller is engaged every mouse and
//! keyboard event is forwarded to the device and swallowed locally, so
//! shortcuts like ⌘Q/⌘Tab (macOS) or Alt+Tab/Win+E (Windows) reach the remote
//! host instead of this computer.

use crate::controller::Controller;
use std::sync::Arc;
use tauri::AppHandle;

#[cfg(target_os = "macos")]
pub use macos::{set_cursor_captured, start};

#[cfg(windows)]
#[path = "capture_windows.rs"]
mod windows_capture;
#[cfg(windows)]
pub use windows_capture::{set_cursor_captured, start};

#[cfg(not(any(target_os = "macos", windows)))]
pub fn start(_controller: Arc<Controller>) -> Result<(), String> {
    Err("Input capture is only implemented on macOS and Windows".into())
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn set_cursor_captured(_app: &AppHandle, _captured: bool) {}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use crate::hid::{InputEvent, MouseButton, ScrollAccumulator};
    use crate::keymap::{self, HID_CAPS_LOCK, KVK_CAPS_LOCK, KVK_P};
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation_sys::preferences::{kCFPreferencesAnyApplication, CFPreferencesCopyAppValue};
    use core_foundation::propertylist::CFPropertyList;
    use core_foundation::mach_port::CFMachPortRef;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::display::CGDisplay;
    use core_graphics::event::{
        CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventType, CallbackResult, EventField,
    };
    use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
    use std::sync::Mutex;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFStringRef;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    }

    static STARTED: AtomicBool = AtomicBool::new(false);
    static TAP_PORT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

    /// Prompts for the Accessibility grant on first call; a suppressing event
    /// tap cannot be created without it.
    fn accessibility_trusted() -> bool {
        let options = CFDictionary::from_CFType_pairs(&[(
            unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) },
            CFBoolean::true_value(),
        )]);
        unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
    }

    pub fn start(controller: Arc<Controller>) -> Result<(), String> {
        if STARTED.load(Ordering::SeqCst) {
            return Ok(());
        }
        if !accessibility_trusted() {
            return Err(
                "Grant Accessibility access to Remote Controller in System Settings → Privacy & Security, then retry."
                    .into(),
            );
        }
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("input-capture".into())
            .spawn(move || run_tap(controller, ready_tx))
            .map_err(|e| e.to_string())?;
        let result = ready_rx.recv().map_err(|e| e.to_string())?;
        if result.is_ok() {
            STARTED.store(true, Ordering::SeqCst);
        }
        result
    }

    /// Scroll deltas in events are already flipped by the user's "natural
    /// scrolling" setting. The remote host applies its own preference, so the
    /// physical direction is what has to be forwarded.
    fn natural_scrolling_enabled() -> bool {
        let key = CFString::new("com.apple.swipescrolldirection");
        let value = unsafe { CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), kCFPreferencesAnyApplication) };
        if value.is_null() {
            return true;
        }
        let value = unsafe { CFPropertyList::wrap_under_create_rule(value) };
        value.downcast::<CFBoolean>().map_or(true, bool::from)
    }

    struct TapState {
        swallow_toggle_key_up: AtomicBool,
        scroll: Mutex<ScrollAccumulator>,
        scroll_sign: f64,
    }

    fn run_tap(controller: Arc<Controller>, ready: std::sync::mpsc::Sender<Result<(), String>>) {
        let state = TapState {
            swallow_toggle_key_up: AtomicBool::new(false),
            scroll: Mutex::default(),
            scroll_sign: if natural_scrolling_enabled() { -1.0 } else { 1.0 },
        };
        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![
                CGEventType::MouseMoved,
                CGEventType::LeftMouseDown,
                CGEventType::LeftMouseUp,
                CGEventType::LeftMouseDragged,
                CGEventType::RightMouseDown,
                CGEventType::RightMouseUp,
                CGEventType::RightMouseDragged,
                CGEventType::OtherMouseDown,
                CGEventType::OtherMouseUp,
                CGEventType::OtherMouseDragged,
                CGEventType::ScrollWheel,
                CGEventType::KeyDown,
                CGEventType::KeyUp,
                CGEventType::FlagsChanged,
            ],
            move |_proxy, etype, event| handle(&controller, &state, etype, event),
        );
        let tap = match tap {
            Ok(tap) => tap,
            Err(()) => {
                let _ = ready.send(Err("Failed to create the input event tap".into()));
                return;
            }
        };
        TAP_PORT.store(tap.mach_port().as_concrete_TypeRef() as *mut _, Ordering::SeqCst);
        let Ok(source) = tap.mach_port().create_runloop_source(0) else {
            let _ = ready.send(Err("Failed to attach the input event tap".into()));
            return;
        };
        CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        let _ = ready.send(Ok(()));
        CFRunLoop::run_current();
    }

    fn is_toggle_chord(flags: CGEventFlags) -> bool {
        flags.contains(CGEventFlags::CGEventFlagCommand | CGEventFlags::CGEventFlagShift)
            && !flags.intersects(CGEventFlags::CGEventFlagControl | CGEventFlags::CGEventFlagAlternate)
    }

    fn handle(
        controller: &Controller,
        state: &TapState,
        etype: CGEventType,
        event: &CGEvent,
    ) -> CallbackResult {
        if matches!(etype, CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput) {
            let port = TAP_PORT.load(Ordering::SeqCst);
            if !port.is_null() {
                unsafe { CGEventTapEnable(port as CFMachPortRef, true) };
            }
            return CallbackResult::Keep;
        }
        if !controller.is_focused() {
            return CallbackResult::Keep;
        }

        let keycode = || event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
        let is_repeat = || event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0;

        match etype {
            CGEventType::KeyDown if keycode() == KVK_P && is_toggle_chord(event.get_flags()) => {
                if !is_repeat() {
                    controller.toggle_pause();
                }
                state.swallow_toggle_key_up.store(true, Ordering::SeqCst);
                return CallbackResult::Drop;
            }
            CGEventType::KeyUp if keycode() == KVK_P && state.swallow_toggle_key_up.swap(false, Ordering::SeqCst) => {
                return CallbackResult::Drop;
            }
            _ => {}
        }

        if !controller.is_engaged() {
            return CallbackResult::Keep;
        }

        let forward = |ev| controller.forward(ev);
        match etype {
            CGEventType::MouseMoved
            | CGEventType::LeftMouseDragged
            | CGEventType::RightMouseDragged
            | CGEventType::OtherMouseDragged => {
                let dx = event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_X) as i32;
                let dy = event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y) as i32;
                if dx != 0 || dy != 0 {
                    forward(InputEvent::MouseMove { dx, dy });
                }
            }
            CGEventType::LeftMouseDown => forward(InputEvent::Button { button: MouseButton::Left, down: true }),
            CGEventType::LeftMouseUp => forward(InputEvent::Button { button: MouseButton::Left, down: false }),
            CGEventType::RightMouseDown => forward(InputEvent::Button { button: MouseButton::Right, down: true }),
            CGEventType::RightMouseUp => forward(InputEvent::Button { button: MouseButton::Right, down: false }),
            CGEventType::ScrollWheel => {
                if let Some(ev) = scroll_event(state, event) {
                    forward(ev);
                }
            }
            CGEventType::KeyDown | CGEventType::KeyUp => {
                let down = matches!(etype, CGEventType::KeyDown);
                if !(down && is_repeat()) {
                    if let Some(usage) = keymap::hid_usage(keycode()) {
                        forward(InputEvent::Key { usage, down });
                    }
                }
            }
            CGEventType::FlagsChanged => {
                let keycode = keycode();
                if let Some(m) = keymap::modifier(keycode) {
                    let down = event.get_flags().bits() & m.device_flag != 0;
                    forward(InputEvent::Modifier { bit: m.hid_bit, down });
                } else if keycode == KVK_CAPS_LOCK {
                    forward(InputEvent::Key { usage: HID_CAPS_LOCK, down: true });
                    forward(InputEvent::Key { usage: HID_CAPS_LOCK, down: false });
                }
            }
            _ => {}
        }
        CallbackResult::Drop
    }

    /// Event axis 1 is vertical (positive = up), axis 2 horizontal (positive =
    /// left); HID pan is positive to the right.
    fn scroll_event(state: &TapState, event: &CGEvent) -> Option<InputEvent> {
        let field = |f| event.get_integer_value_field(f) as f64 * state.scroll_sign;
        if event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS) != 0 {
            let vertical = field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1);
            let horizontal = -field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2);
            return state.scroll.lock().unwrap().add_pixels(vertical, horizontal);
        }
        let vertical = field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1) as i32;
        let horizontal = -field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2) as i32;
        (vertical != 0 || horizontal != 0).then_some(InputEvent::Scroll { vertical, horizontal })
    }

    /// Freezes and hides the local pointer while engaged; the tap still sees
    /// raw deltas because they are read from the event, not the cursor.
    /// Must run on the main thread.
    pub fn set_cursor_captured(_app: &AppHandle, captured: bool) {
        let _ = CGDisplay::associate_mouse_and_mouse_cursor_position(!captured);
        let display = CGDisplay::main();
        let _ = if captured { display.hide_cursor() } else { display.show_cursor() };
    }
}
