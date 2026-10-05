use crate::capture;
use crate::cdc;
use crate::hid::{self, HidTranslator, InputEvent};
use log::{info, warn};
use mouthpad_proto::mouthware_message::{
    mouthpad_to_app_message::Uplink, mouthware_response::MessageBody as ResponseBody,
    MouthpadToAppMessage,
};
use prost::Message as _;
use serde::Serialize;
use serialport::SerialPort;
use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    connected: bool,
    port: Option<String>,
    focused: bool,
    paused: bool,
    mouthpad_on_host: bool,
    engaged: bool,
    capture_error: Option<String>,
    last_error: Option<String>,
    messages_sent: u64,
    acked: u64,
    rejected: u64,
}

/// Owns the forwarding state. Input is forwarded only while the window is
/// focused, the device's proto CDC port is open, capture is running, the user
/// hasn't paused with ⌘⇧P, and no MouthPad is paired to this machine.
pub struct Controller {
    app: AppHandle,
    events: UnboundedSender<InputEvent>,
    focused: AtomicBool,
    paused: AtomicBool,
    mouthpad_on_host: AtomicBool,
    capture_running: AtomicBool,
    engaged: AtomicBool,
    capture_error: Mutex<Option<String>>,
    last_error: Mutex<Option<String>>,
    port_name: Mutex<Option<String>>,
    port: Mutex<Option<Box<dyn SerialPort>>>,
    /// Bumped on every open so a reader thread from a previous connection
    /// can't tear down its successor.
    generation: AtomicU64,
    messages_sent: AtomicU64,
    acked: AtomicU64,
    rejected: AtomicU64,
    /// Serializes engage/disengage so cursor hide/show calls stay balanced.
    transition: Mutex<()>,
}

impl Controller {
    pub fn new(app: AppHandle, events: UnboundedSender<InputEvent>) -> Self {
        Self {
            app,
            events,
            focused: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            mouthpad_on_host: AtomicBool::new(false),
            capture_running: AtomicBool::new(false),
            engaged: AtomicBool::new(false),
            capture_error: Mutex::new(None),
            last_error: Mutex::new(None),
            port_name: Mutex::new(None),
            port: Mutex::new(None),
            generation: AtomicU64::new(0),
            messages_sent: AtomicU64::new(0),
            acked: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            transition: Mutex::new(()),
        }
    }

    pub fn is_focused(&self) -> bool {
        self.focused.load(Ordering::SeqCst)
    }

    pub fn is_engaged(&self) -> bool {
        self.engaged.load(Ordering::SeqCst)
    }

    fn is_connected(&self) -> bool {
        self.port.lock().unwrap().is_some()
    }

    pub fn forward(&self, ev: InputEvent) {
        let _ = self.events.send(ev);
    }

    pub fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::SeqCst);
        self.refresh();
    }

    pub fn toggle_pause(&self) {
        let paused = !self.paused.fetch_xor(true, Ordering::SeqCst);
        info!("forwarding {}", if paused { "paused" } else { "resumed" });
        self.refresh();
    }

    pub fn set_mouthpad_on_host(&self, present: bool) {
        if self.mouthpad_on_host.swap(present, Ordering::SeqCst) != present {
            info!(
                "MouthPad {} this machine over BLE; forwarding {}",
                if present { "connected to" } else { "disconnected from" },
                if present { "paused" } else { "allowed" }
            );
            self.refresh();
        }
    }

    pub fn start_capture(self: &Arc<Self>) {
        let result = capture::start(self.clone());
        self.capture_running.store(result.is_ok(), Ordering::SeqCst);
        *self.capture_error.lock().unwrap() = result.err();
        self.refresh();
    }

    pub fn status(&self) -> Status {
        Status {
            connected: self.is_connected(),
            port: self.port_name.lock().unwrap().clone(),
            focused: self.is_focused(),
            paused: self.paused.load(Ordering::SeqCst),
            mouthpad_on_host: self.mouthpad_on_host.load(Ordering::SeqCst),
            engaged: self.is_engaged(),
            capture_error: self.capture_error.lock().unwrap().clone(),
            last_error: self.last_error.lock().unwrap().clone(),
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            acked: self.acked.load(Ordering::Relaxed),
            rejected: self.rejected.load(Ordering::Relaxed),
        }
    }

    fn refresh(&self) {
        {
            let _guard = self.transition.lock().unwrap();
            let engage = self.is_focused()
                && !self.paused.load(Ordering::SeqCst)
                && !self.mouthpad_on_host.load(Ordering::SeqCst)
                && self.is_connected()
                && self.capture_running.load(Ordering::SeqCst);
            if self.engaged.swap(engage, Ordering::SeqCst) != engage {
                if !engage {
                    self.forward(InputEvent::ReleaseAll);
                }
                let _ = self.app.run_on_main_thread(move || capture::set_cursor_captured(engage));
            }
        }
        let _ = self.app.emit("status", self.status());
    }

    fn set_error(&self, error: Option<String>) {
        *self.last_error.lock().unwrap() = error;
    }

    pub fn connect(self: &Arc<Self>) -> Result<(), String> {
        if self.is_connected() {
            return Ok(());
        }
        let result = self.open_port();
        self.set_error(result.as_ref().err().cloned());
        self.refresh();
        result
    }

    fn open_port(self: &Arc<Self>) -> Result<(), String> {
        let path = cdc::find_proto_port()
            .ok_or("No MouthPad proto CDC port found. Is the device plugged in over USB?")?;
        let port = cdc::open(&path)?;
        let reader = port.try_clone().map_err(|e| format!("cloning {path}: {e}"))?;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        info!("opened proto CDC port {path}");
        *self.port.lock().unwrap() = Some(port);
        *self.port_name.lock().unwrap() = Some(path);

        let this = self.clone();
        std::thread::Builder::new()
            .name("cdc-reader".into())
            .spawn(move || this.drain_responses(reader, generation))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Reads device->host frames so the OS buffer never fills, tallies the
    /// device's verdict on each HIDLoopback, and notices unplugging.
    fn drain_responses(&self, mut reader: Box<dyn SerialPort>, generation: u64) {
        let mut buf = [0u8; 512];
        let mut accum = Vec::new();
        while self.generation.load(Ordering::SeqCst) == generation {
            match reader.read(&mut buf) {
                Ok(n) => {
                    accum.extend_from_slice(&buf[..n]);
                    while let Some(payload) = cdc::next_frame(&mut accum) {
                        self.record_reply(&payload);
                    }
                }
                Err(e) if e.kind() == ErrorKind::TimedOut => {}
                Err(e) => {
                    if self.generation.load(Ordering::SeqCst) == generation {
                        warn!("proto CDC port closed: {e}");
                        self.close_port(Some(format!("Device disconnected ({e})")));
                    }
                    return;
                }
            }
        }
    }

    fn record_reply(&self, payload: &[u8]) {
        let Ok(MouthpadToAppMessage { uplink: Some(Uplink::MouthwareResponse(resp)) }) =
            MouthpadToAppMessage::decode(payload)
        else {
            return;
        };
        if !matches!(resp.message_body, Some(ResponseBody::HidLoopbackResponse(_))) {
            return;
        }
        let code = resp.request_result.map_or(0, |r| r.code);
        if code == 0 {
            self.acked.fetch_add(1, Ordering::Relaxed);
        } else {
            self.rejected.fetch_add(1, Ordering::Relaxed);
            warn!("device rejected HIDLoopback #{} with error code {code}", resp.message_index);
            self.set_error(Some(format!("Device rejected HIDLoopback (error code {code})")));
        }
    }

    pub fn disconnect(&self) {
        self.close_port(None);
    }

    fn close_port(&self, error: Option<String>) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *self.port.lock().unwrap() = None;
        *self.port_name.lock().unwrap() = None;
        self.set_error(error);
        self.refresh();
    }

    pub fn shutdown(&self) {
        if self.engaged.swap(false, Ordering::SeqCst) {
            capture::set_cursor_captured(false);
        }
    }

    /// Drains captured input and writes it to the device. Moves that pile up
    /// behind a write are merged so latency can't grow with the backlog.
    pub fn run_writer(self: Arc<Self>, mut rx: UnboundedReceiver<InputEvent>) {
        let mut translator = HidTranslator::default();
        let mut message_index: i32 = 0;
        while let Some(first) = rx.blocking_recv() {
            let mut batch = vec![first];
            while let Ok(ev) = rx.try_recv() {
                batch.push(ev);
            }
            let mut out = Vec::new();
            let mut count = 0;
            for action in hid::coalesce(batch).into_iter().flat_map(|ev| translator.translate(ev)) {
                message_index = message_index.wrapping_add(1);
                out.extend(cdc::frame(&hid::encode(action, message_index)));
                count += 1;
            }
            if out.is_empty() {
                continue;
            }
            let result = match self.port.lock().unwrap().as_mut() {
                Some(port) => port.write_all(&out).and_then(|()| port.flush()),
                None => continue,
            };
            match result {
                Ok(()) => {
                    self.messages_sent.fetch_add(count, Ordering::Relaxed);
                }
                Err(e) => {
                    warn!("HIDLoopback write failed: {e}");
                    self.close_port(Some(format!("Write failed: {e}")));
                }
            }
        }
    }
}
