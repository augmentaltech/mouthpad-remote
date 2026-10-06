//! Windows capture through low-level keyboard and mouse hooks. A hook that
//! returns non-zero swallows the event before any window sees it, which is what
//! lets shortcuts like Alt+Tab reach the remote host. Windows still handles
//! Ctrl+Alt+Del and Win+L itself; no hook can block them.

use super::*;
use crate::hid::{InputEvent, MouseButton, ScrollAccumulator};
use crate::keymap_windows::{self, KeyAction};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{Manager, PhysicalPosition};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetCursorPos, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WH_MOUSE_LL,
};

const HC_ACTION: i32 = 0;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_MOUSEHWHEEL: u32 = 0x020E;
const LLKHF_EXTENDED: u32 = 0x01;
const LLKHF_INJECTED: u32 = 0x10;
const LLMHF_INJECTED: u32 = 0x01;
const SCAN_CODE_P: u32 = 0x19;
/// Moving the cursor to the window's centre on engage produces one move event
/// that isn't the user's; moves this soon afterwards are swallowed unforwarded.
const RECENTER_SETTLE: Duration = Duration::from_millis(100);

struct HookState {
    controller: Arc<Controller>,
    swallow_toggle_key_up: AtomicBool,
    scroll: Mutex<ScrollAccumulator>,
}

static STATE: OnceLock<HookState> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);
static RECENTERED_AT: Mutex<Option<Instant>> = Mutex::new(None);

pub fn start(controller: Arc<Controller>) -> Result<(), String> {
    if STARTED.load(Ordering::SeqCst) {
        return Ok(());
    }
    STATE.get_or_init(|| HookState {
        controller,
        swallow_toggle_key_up: AtomicBool::new(false),
        scroll: Mutex::default(),
    });
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("input-capture".into())
        .spawn(move || run_hooks(ready_tx))
        .map_err(|e| e.to_string())?;
    let result = ready_rx.recv().map_err(|e| e.to_string())?;
    if result.is_ok() {
        STARTED.store(true, Ordering::SeqCst);
    }
    result
}

/// Low-level hooks are called on the thread that installed them, so that
/// thread has to keep pumping messages for as long as capture runs.
fn run_hooks(ready: std::sync::mpsc::Sender<Result<(), String>>) {
    let installed = unsafe {
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0)
            .and_then(|_| SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0))
    };
    if let Err(e) = installed {
        let _ = ready.send(Err(format!("Failed to install input hooks: {e}")));
        return;
    }
    let _ = ready.send(Ok(()));
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {}
}

fn key_held(vk: VIRTUAL_KEY) -> bool { (unsafe { GetAsyncKeyState(vk.0 as i32) } as u16) & 0x8000 != 0 }

fn is_toggle_chord() -> bool {
    key_held(VK_CONTROL) && key_held(VK_SHIFT) && !key_held(VK_MENU) && !key_held(VK_LWIN) && !key_held(VK_RWIN)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION {
        if let Some(state) = STATE.get() {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            if handle_key(state, wparam.0 as u32, info) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Returns whether to swallow the event.
fn handle_key(state: &HookState, message: u32, info: &KBDLLHOOKSTRUCT) -> bool {
    if info.flags.0 & LLKHF_INJECTED != 0 || !state.controller.is_focused() {
        return false;
    }
    let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
    let up = message == WM_KEYUP || message == WM_SYSKEYUP;
    if !down && !up {
        return false;
    }
    let extended = info.flags.0 & LLKHF_EXTENDED != 0;

    // Ctrl+Shift+P toggles pause. Key repeat re-sends the down event, so only
    // the first down (before the matching up has been swallowed) toggles.
    if info.scanCode == SCAN_CODE_P && !extended {
        if down && is_toggle_chord() {
            if !state.swallow_toggle_key_up.swap(true, Ordering::SeqCst) {
                state.controller.toggle_pause();
            }
            return true;
        }
        if up && state.swallow_toggle_key_up.swap(false, Ordering::SeqCst) {
            return true;
        }
    }

    if !state.controller.is_engaged() {
        return false;
    }
    match keymap_windows::classify(info.vkCode, info.scanCode, extended) {
        Some(KeyAction::Modifier(bit)) => state.controller.forward(InputEvent::Modifier { bit, down }),
        Some(KeyAction::Key(usage)) => state.controller.forward(InputEvent::Key { usage, down }),
        None => {}
    }
    true
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION {
        if let Some(state) = STATE.get() {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            if handle_mouse(state, wparam.0 as u32, info) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn recently_recentered() -> bool {
    RECENTERED_AT.lock().unwrap().is_some_and(|at| at.elapsed() < RECENTER_SETTLE)
}

/// Returns whether to swallow the event.
fn handle_mouse(state: &HookState, message: u32, info: &MSLLHOOKSTRUCT) -> bool {
    if info.flags & LLMHF_INJECTED != 0 || !state.controller.is_engaged() {
        return false;
    }
    let forward = |ev| state.controller.forward(ev);
    match message {
        // The hook runs before the cursor moves, and swallowing every move keeps
        // the cursor where it is, so the event's point minus the cursor is the
        // user's (pointer-accelerated) motion.
        WM_MOUSEMOVE if !recently_recentered() => {
            let mut cursor = POINT::default();
            if unsafe { GetCursorPos(&mut cursor) }.is_ok() {
                let (dx, dy) = (info.pt.x - cursor.x, info.pt.y - cursor.y);
                if dx != 0 || dy != 0 {
                    forward(InputEvent::MouseMove { dx, dy });
                }
            }
        }
        WM_LBUTTONDOWN => forward(InputEvent::Button { button: MouseButton::Left, down: true }),
        WM_LBUTTONUP => forward(InputEvent::Button { button: MouseButton::Left, down: false }),
        WM_RBUTTONDOWN => forward(InputEvent::Button { button: MouseButton::Right, down: true }),
        WM_RBUTTONUP => forward(InputEvent::Button { button: MouseButton::Right, down: false }),
        // The high word of mouseData is a signed WHEEL_DELTA count: positive is
        // wheel up / tilt right, matching HID wheel and pan.
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let delta = f64::from((info.mouseData >> 16) as u16 as i16);
            let (vertical, horizontal) = if message == WM_MOUSEWHEEL { (delta, 0.0) } else { (0.0, delta) };
            if let Some(ev) = state.scroll.lock().unwrap().add_wheel_delta(vertical, horizontal) {
                forward(ev);
            }
        }
        _ => {}
    }
    true
}

/// Hides the pointer over the window and parks it at the window's centre, so the
/// cursor stays hidden and far from the screen edges that would clip motion.
pub fn set_cursor_captured(app: &AppHandle, captured: bool) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.set_cursor_visible(!captured);
    if captured {
        if let Ok(size) = window.inner_size() {
            *RECENTERED_AT.lock().unwrap() = Some(Instant::now());
            let _ = window.set_cursor_position(PhysicalPosition::new((size.width / 2) as i32, (size.height / 2) as i32));
        }
    }
}
