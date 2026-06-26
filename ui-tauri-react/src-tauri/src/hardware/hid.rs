use std::fs;
use std::os::unix::io::AsRawFd;

use rusb::UsbContext;

use crate::hardware::sysfs;
use crate::models::ConnectionType;

const FEATURE_REPORT_LEN: usize = 16;
const HIDIOCSFEATURE_16: libc::c_ulong = 0xC010_4806;
const HIDIOCGFEATURE_16: libc::c_ulong = 0xC010_4807;

/// USB HID SET_REPORT for keyboard backlight control using rusb.
///
/// Protocol:
///   Report ID: 0x5A
///   Data: [0x5A, 0xBA, 0xC5, 0xC4, level, 0x00 x 11]
///   wValue: 0x035A, wIndex: 4, wLength: 16
pub fn set_backlight_usb(level: u8) -> Result<(), String> {
    let level = level.min(3);

    let context = rusb::Context::new().map_err(|e| format!("USB context error: {e}"))?;
    let devices = context
        .devices()
        .map_err(|e| format!("USB device list error: {e}"))?;

    for device in devices.iter() {
        let desc: rusb::DeviceDescriptor = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        // ASUS Zenbook Duo keyboard: vendor 0x0B05
        if desc.vendor_id() != 0x0B05 {
            continue;
        }

        let handle: rusb::DeviceHandle<rusb::Context> = match device.open() {
            Ok(h) => h,
            Err(_) => continue,
        };

        // Check if this is the keyboard by reading product string
        if let Ok(product) = handle.read_product_string_ascii(&desc) {
            if !product.contains("Zenbook Duo Keyboard") && !product.contains("ASUS_DUO") {
                continue;
            }
        } else {
            continue;
        }

        // Detach kernel driver if needed
        let interface = 4u8;
        let _ = handle.set_auto_detach_kernel_driver(true);
        let _ = handle.claim_interface(interface as u8);

        let data = build_backlight_report(level);

        // HID SET_REPORT: bmRequestType=0x21, bRequest=0x09
        // wValue = 0x0300 | report_id = 0x035A
        // wIndex = interface number
        let request_type = 0x21; // Host-to-device, class, interface
        let request = 0x09; // SET_REPORT
        let value = 0x035A; // Feature report, ID 0x5A
        let index = interface as u16;
        let timeout = std::time::Duration::from_secs(2);

        handle
            .write_control(request_type, request, value, index, &data, timeout)
            .map_err(|e| format!("USB write error: {e}"))?;

        return Ok(());
    }

    Err("Zenbook Duo keyboard not found via USB".into())
}

/// Bluetooth HID Feature Report for keyboard backlight using ioctl HIDIOCSFEATURE.
pub fn set_backlight_bluetooth(level: u8) -> Result<(), String> {
    try_bluetooth_backlight(level)
}

/// Set backlight, preferring the active keyboard transport.
pub fn set_backlight(level: u8) -> Result<(), String> {
    let bt_first = !matches!(sysfs::detect_connection_type(), ConnectionType::Usb);
    if bt_first {
        set_backlight_bluetooth(level).or_else(|bt_err| {
            set_backlight_usb(level).map_err(|usb_err| both_failed(usb_err, bt_err))
        })
    } else {
        set_backlight_usb(level).or_else(|usb_err| {
            set_backlight_bluetooth(level).map_err(|bt_err| both_failed(usb_err, bt_err))
        })
    }
}

fn try_bluetooth_backlight(level: u8) -> Result<(), String> {
    for attempt in 0..2 {
        let paths = list_bluetooth_hidraw_paths();
        if paths.is_empty() {
            return Err("Bluetooth hidraw device not found".into());
        }

        let mut errors = Vec::new();
        for path in &paths {
            match set_backlight_on_hidraw(path, level) {
                Ok(()) => return Ok(()),
                Err(err) => errors.push(format!("{path}: {err}")),
            }
        }

        if attempt == 0 && reset_bluetooth_keyboard_hid().is_ok() {
            continue;
        }

        return Err(format!("bluetooth hidraw failed ({})", errors.join("; ")));
    }

    unreachable!()
}

fn set_backlight_on_hidraw(path: &str, level: u8) -> Result<(), String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("Failed to open {path}: {e}"))?;

    let fd = file.as_raw_fd();

    let mut probe = [0u8; FEATURE_REPORT_LEN];
    probe[0] = 0x5A;
    if unsafe { libc::ioctl(fd, HIDIOCGFEATURE_16, probe.as_mut_ptr()) } < 0 {
        return Err(format!(
            "HIDIOCGFEATURE probe failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    let mut data = build_backlight_report(level);
    if unsafe { libc::ioctl(fd, HIDIOCSFEATURE_16, data.as_mut_ptr()) } < 0 {
        return Err(format!(
            "ioctl HIDIOCSFEATURE failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    Ok(())
}

fn build_backlight_report(level: u8) -> [u8; FEATURE_REPORT_LEN] {
    let mut data = [0u8; FEATURE_REPORT_LEN];
    data[0] = 0x5A;
    data[1] = 0xBA;
    data[2] = 0xC5;
    data[3] = 0xC4;
    data[4] = level.min(3);
    data
}

/// The BT keyboard exposes both hid-generic and hid-multitouch hidraw nodes; prefer generic.
fn list_bluetooth_hidraw_paths() -> Vec<String> {
    let mut paths: Vec<(u32, String)> = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/hidraw") else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let uevent_path = entry.path().join("device/uevent");
        let Ok(contents) = fs::read_to_string(&uevent_path) else {
            continue;
        };
        if !is_zenbook_bluetooth_uevent(&contents) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        paths.push((hidraw_priority(&contents), format!("/dev/{name}")));
    }

    paths.sort_by_key(|(priority, _)| *priority);
    paths.into_iter().map(|(_, path)| path).collect()
}

fn is_zenbook_bluetooth_uevent(contents: &str) -> bool {
    (contents.contains("Zenbook Duo Keyboard") || contents.contains("ASUS_DUO"))
        && contents.contains("HID_ID=0005:")
}

fn hidraw_priority(contents: &str) -> u32 {
    if contents.contains("DRIVER=hid-multitouch") || contents.contains("g0004") {
        100
    } else if contents.contains("DRIVER=hid-generic") || contents.contains("g0001") {
        0
    } else {
        50
    }
}

fn reset_bluetooth_keyboard_hid() -> Result<(), String> {
    let device_id = find_bluetooth_hid_device_id("hid-generic")
        .ok_or_else(|| "bluetooth keyboard hid device not found".to_string())?;

    fs::write(
        "/sys/bus/hid/drivers/hid-generic/unbind",
        &device_id,
    )
    .map_err(|e| format!("Failed to unbind {device_id}: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(250));
    fs::write("/sys/bus/hid/drivers/hid-generic/bind", &device_id)
        .map_err(|e| format!("Failed to rebind {device_id}: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(())
}

fn find_bluetooth_hid_device_id(driver: &str) -> Option<String> {
    let entries = fs::read_dir("/sys/bus/hid/devices").ok()?;
    for entry in entries.flatten() {
        let device_path = entry.path();
        let uevent = fs::read_to_string(device_path.join("uevent")).ok()?;
        if !is_zenbook_bluetooth_uevent(&uevent) {
            continue;
        }
        let linked_driver = fs::read_link(device_path.join("driver"))
            .ok()
            .and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))?;
        if linked_driver != driver {
            continue;
        }
        return Some(entry.file_name().to_string_lossy().into_owned());
    }
    None
}

fn both_failed(usb_err: String, bt_err: String) -> String {
    format!("Failed to set keyboard backlight natively (usb: {usb_err}; bt: {bt_err})")
}

#[cfg(test)]
mod tests {
    use super::hidraw_priority;

    #[test]
    fn prefers_generic_hidraw_over_multitouch() {
        let generic = "DRIVER=hid-generic\nMODALIAS=hid:b0005g0001v00000B05p00001CD8\n";
        let multitouch = "DRIVER=hid-multitouch\nMODALIAS=hid:b0005g0004v00000B05p00001CD8\n";
        assert!(hidraw_priority(generic) < hidraw_priority(multitouch));
    }
}
