//! The device's proto USB CDC port: `[u16 LE payload length][MouthwareMessage]`
//! host->device, the same framing back with `MouthpadToAppMessage`.

use serialport::{SerialPort, SerialPortType, UsbPortInfo};
use std::time::Duration;

const NORDIC_VID: u16 = 0x1915;
const BAUD: u32 = 115_200;
/// The proto port's communication and data interface numbers. Windows and
/// Linux report the former for the port, macOS the latter.
const PROTO_CDC_INTERFACES: [u8; 2] = [2, 3];

fn is_proto_cdc_port(info: &UsbPortInfo) -> bool {
    info.vid == NORDIC_VID && info.interface.is_some_and(|i| PROTO_CDC_INTERFACES.contains(&i))
}

/// macOS lists each port as both a `tty.` and a `cu.` node; opening the `tty.`
/// one blocks waiting for carrier detect.
fn prefer_callout(name: String) -> String {
    match name.strip_prefix("/dev/tty.") {
        Some(rest) => format!("/dev/cu.{rest}"),
        None => name,
    }
}

pub fn find_proto_port() -> Option<String> {
    let mut names: Vec<String> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| match p.port_type {
            SerialPortType::UsbPort(info) if is_proto_cdc_port(&info) => Some(prefer_callout(p.port_name)),
            _ => None,
        })
        .collect();
    names.sort();
    names.dedup();
    names.into_iter().next()
}

/// The firmware doesn't take host data until DTR is asserted. macOS and Linux
/// assert it on open; Windows leaves it off, so writes stall until they time out.
pub fn open(path: &str) -> Result<Box<dyn SerialPort>, String> {
    serialport::new(path, BAUD)
        .timeout(Duration::from_millis(200))
        .dtr_on_open(true)
        .open()
        .map_err(|e| format!("opening {path}: {e}"))
}

pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + payload.len());
    out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// The firmware's largest device->host payload; a longer length can't be a
/// frame boundary.
const MAX_FRAME_LEN: usize = 512;

/// Pops the next complete frame's payload off `accum`, resyncing a byte at a
/// time past lengths that can't be real.
pub fn next_frame(accum: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        if accum.len() < 2 {
            return None;
        }
        let len = u16::from_le_bytes([accum[0], accum[1]]) as usize;
        if len == 0 || len > MAX_FRAME_LEN {
            accum.drain(..1);
            continue;
        }
        if accum.len() < 2 + len {
            return None;
        }
        let payload = accum[2..2 + len].to_vec();
        accum.drain(..2 + len);
        return Some(payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb(vid: u16, interface: Option<u8>) -> UsbPortInfo {
        UsbPortInfo { vid, pid: 0, serial_number: None, manufacturer: None, product: None, interface }
    }

    #[test]
    fn only_nordic_proto_interfaces_match() {
        assert!(is_proto_cdc_port(&usb(NORDIC_VID, Some(2))));
        assert!(is_proto_cdc_port(&usb(NORDIC_VID, Some(3))));
        assert!(!is_proto_cdc_port(&usb(NORDIC_VID, Some(0))));
        assert!(!is_proto_cdc_port(&usb(0x1234, Some(3))));
        assert!(!is_proto_cdc_port(&usb(NORDIC_VID, None)));
    }

    #[test]
    fn frame_prefixes_little_endian_length() {
        assert_eq!(frame(&[0xAA; 300])[..2], [0x2C, 0x01]);
        assert_eq!(frame(&[1, 2]), vec![2, 0, 1, 2]);
    }

    #[test]
    fn next_frame_waits_for_whole_frames_and_resyncs_past_bad_lengths() {
        let mut accum = vec![0x00, 0x00];
        accum.extend(frame(&[7, 8, 9]));
        accum.extend_from_slice(&[5, 0, 1]);
        assert_eq!(next_frame(&mut accum), Some(vec![7, 8, 9]));
        assert_eq!(next_frame(&mut accum), None);
        accum.extend_from_slice(&[2, 3, 4, 5]);
        assert_eq!(next_frame(&mut accum), Some(vec![1, 2, 3, 4, 5]));
        assert!(accum.is_empty());
    }

    #[test]
    fn tty_paths_become_callout_paths() {
        assert_eq!(prefer_callout("/dev/tty.usbmodem1".into()), "/dev/cu.usbmodem1");
        assert_eq!(prefer_callout("COM7".into()), "COM7");
    }
}
