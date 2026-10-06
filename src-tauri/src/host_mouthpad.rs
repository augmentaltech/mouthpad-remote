//! Detects a MouthPad paired to *this* machine over BLE. Its HID reports would
//! land back in our own capture and be forwarded again in a loop, so the
//! controller pauses while one is attached.

use crate::controller::Controller;
use std::collections::HashSet;
use std::sync::Arc;

/// The BLE Device Information PnP ID the firmware advertises.
pub const MOUTHPAD_VENDOR_ID: i32 = 0x1915;
pub const MOUTHPAD_PRODUCT_ID: i32 = 0xEEEE;

/// macOS may expose one paired MouthPad as several HID devices, so presence is
/// tracked per device handle.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Default)]
pub struct Presence(HashSet<usize>);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Presence {
    /// Returns whether any MouthPad is attached after the change.
    pub fn update(&mut self, device: usize, attached: bool) -> bool {
        if attached {
            self.0.insert(device);
        } else {
            self.0.remove(&device);
        }
        !self.0.is_empty()
    }
}

#[cfg(target_os = "macos")]
pub use macos::start;

#[cfg(windows)]
pub use windows_watch::start;

#[cfg(not(any(target_os = "macos", windows)))]
pub fn start(_controller: Arc<Controller>) {}

/// Windows has no cheap HID arrival callback without a window to receive it, so
/// the HID device list is polled.
#[cfg(windows)]
mod windows_watch {
    use super::*;
    use std::time::Duration;

    const POLL_INTERVAL: Duration = Duration::from_secs(2);

    pub fn start(controller: Arc<Controller>) {
        let spawned = std::thread::Builder::new().name("host-mouthpad".into()).spawn(move || {
            let mut api = match hidapi::HidApi::new() {
                Ok(api) => api,
                Err(e) => {
                    log::warn!("MouthPad host detection unavailable: {e}");
                    return;
                }
            };
            loop {
                let present = api.refresh_devices().is_ok()
                    && api.device_list().any(|d| {
                        i32::from(d.vendor_id()) == MOUTHPAD_VENDOR_ID && i32::from(d.product_id()) == MOUTHPAD_PRODUCT_ID
                    });
                controller.set_mouthpad_on_host(present);
                std::thread::sleep(POLL_INTERVAL);
            }
        });
        if let Err(e) = spawned {
            log::warn!("could not start MouthPad host detection: {e}");
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use core_foundation::base::{kCFAllocatorDefault, CFAllocatorRef, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRef};
    use core_foundation::string::{CFString, CFStringRef};
    use std::ffi::c_void;
    use std::sync::Mutex;

    type IOHIDManagerRef = *mut c_void;
    type IOHIDDeviceCallback = extern "C" fn(*mut c_void, i32, *mut c_void, *mut c_void);

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDManagerCreate(allocator: CFAllocatorRef, options: u32) -> IOHIDManagerRef;
        fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
        fn IOHIDManagerRegisterDeviceMatchingCallback(m: IOHIDManagerRef, cb: IOHIDDeviceCallback, ctx: *mut c_void);
        fn IOHIDManagerRegisterDeviceRemovalCallback(m: IOHIDManagerRef, cb: IOHIDDeviceCallback, ctx: *mut c_void);
        fn IOHIDManagerScheduleWithRunLoop(m: IOHIDManagerRef, run_loop: CFRunLoopRef, mode: CFStringRef);
        fn IOHIDManagerOpen(m: IOHIDManagerRef, options: u32) -> i32;
    }

    struct Watch {
        controller: Arc<Controller>,
        presence: Mutex<Presence>,
    }

    impl Watch {
        fn update(&self, device: *mut c_void, attached: bool) {
            let present = self.presence.lock().unwrap().update(device as usize, attached);
            self.controller.set_mouthpad_on_host(present);
        }
    }

    extern "C" fn on_matched(ctx: *mut c_void, _result: i32, _sender: *mut c_void, device: *mut c_void) {
        unsafe { &*(ctx as *const Watch) }.update(device, true);
    }

    extern "C" fn on_removed(ctx: *mut c_void, _result: i32, _sender: *mut c_void, device: *mut c_void) {
        unsafe { &*(ctx as *const Watch) }.update(device, false);
    }

    pub fn start(controller: Arc<Controller>) {
        // The watch lives for the whole process; the callbacks hold a raw pointer to it.
        let watch: &'static Watch = Box::leak(Box::new(Watch { controller, presence: Mutex::default() }));
        let spawned = std::thread::Builder::new().name("host-mouthpad".into()).spawn(move || {
            let matching = CFDictionary::from_CFType_pairs(&[
                (CFString::new("VendorID"), CFNumber::from(MOUTHPAD_VENDOR_ID)),
                (CFString::new("ProductID"), CFNumber::from(MOUTHPAD_PRODUCT_ID)),
            ]);
            let ctx = watch as *const Watch as *mut c_void;
            unsafe {
                let manager = IOHIDManagerCreate(kCFAllocatorDefault, 0);
                IOHIDManagerSetDeviceMatching(manager, matching.as_concrete_TypeRef());
                IOHIDManagerRegisterDeviceMatchingCallback(manager, on_matched, ctx);
                IOHIDManagerRegisterDeviceRemovalCallback(manager, on_removed, ctx);
                IOHIDManagerScheduleWithRunLoop(manager, CFRunLoop::get_current().as_concrete_TypeRef(), kCFRunLoopDefaultMode);
                // Matching callbacks fire even if opening the devices is refused
                // (no Input Monitoring grant), so the result doesn't matter.
                let _ = IOHIDManagerOpen(manager, 0);
            }
            CFRunLoop::run_current();
        });
        if let Err(e) = spawned {
            log::warn!("could not start MouthPad host detection: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_until_every_handle_is_removed() {
        let mut p = Presence::default();
        assert!(p.update(1, true));
        assert!(p.update(2, true));
        assert!(p.update(1, false));
        assert!(!p.update(2, false));
    }

    #[test]
    fn removing_an_unknown_handle_keeps_absent() {
        assert!(!Presence::default().update(9, false));
    }
}
