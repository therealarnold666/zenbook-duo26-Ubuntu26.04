use std::fs;
use std::path::Path;
use std::process::Command;

use crate::hardware::{display_config, sysfs};
use crate::models::{DisplayLayout, DuoStatus, Orientation};

const USB_DEVICES_PATH: &str = "/sys/bus/usb/devices";
const KEYBOARD_USB_VENDOR_ID: &str = "0b05";
const KEYBOARD_USB_PRODUCT_ID: &str = "1cd7";

pub fn current_status() -> DuoStatus {
    let mut status = sysfs::get_full_status();
    status.keyboard_attached = keyboard_attached();
    status.connection_type = sysfs::detect_connection_type();
    status.wifi_enabled = wifi_enabled();
    status.bluetooth_enabled = bluetooth_enabled();
    apply_layout_to_status(
        &mut status,
        display_config::get_display_layout().ok().as_ref(),
    );
    status
}

pub fn apply_layout_to_status(status: &mut DuoStatus, layout: Option<&DisplayLayout>) {
    status.monitor_count = monitor_count(layout, status.monitor_count);
    status.orientation = inferred_orientation(layout).unwrap_or(status.orientation.clone());
}

pub fn keyboard_attached() -> bool {
    usb_device_present(
        Path::new(USB_DEVICES_PATH),
        KEYBOARD_USB_VENDOR_ID,
        KEYBOARD_USB_PRODUCT_ID,
    )
}

fn usb_device_present(devices_path: &Path, vendor_id: &str, product_id: &str) -> bool {
    fs::read_dir(devices_path)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            usb_id_matches(&entry.path().join("idVendor"), vendor_id)
                && usb_id_matches(&entry.path().join("idProduct"), product_id)
        })
}

fn usb_id_matches(path: &Path, expected: &str) -> bool {
    fs::read_to_string(path)
        .map(|value| value.trim().eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

pub fn wifi_enabled() -> bool {
    Command::new("nmcli")
        .args(["radio", "wifi"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.lines().next().unwrap_or_default().trim() == "enabled")
        .unwrap_or(false)
}

pub fn bluetooth_enabled() -> bool {
    Command::new("bluetoothctl")
        .arg("show")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| bluetooth_controller_powered(&s))
        .unwrap_or(false)
}

fn bluetooth_controller_powered(show_output: &str) -> bool {
    show_output
        .lines()
        .any(|line| line.trim() == "Powered: yes")
}

fn monitor_count(layout: Option<&crate::models::DisplayLayout>, current: u32) -> u32 {
    layout
        .map(|layout| layout.displays.len() as u32)
        .filter(|count| *count > 0)
        .or_else(|| if current > 0 { Some(current) } else { None })
        .unwrap_or(0)
}

fn inferred_orientation(layout: Option<&crate::models::DisplayLayout>) -> Option<Orientation> {
    let layout = layout?;

    // In a real dual-screen GNOME layout eDP-2 is the stable transform source.
    // eDP-1 can be reported as `normal` while mutter rebuilds the paired layout,
    // which previously made every orientation look inverted.
    if let Some(secondary) = layout
        .displays
        .iter()
        .find(|display| display.connector == "eDP-2" && display.enabled)
    {
        return Some(match secondary.transform {
            90 => Orientation::Right,
            180 => Orientation::Inverted,
            270 => Orientation::Left,
            _ => Orientation::Normal,
        });
    }

    let display = layout
        .displays
        .iter()
        // eDP-1 is the physical main panel and defines the app's orientation.
        // GNOME can transiently move its primary marker while rebuilding a
        // two-screen layout, so do not use that marker as the first choice.
        .find(|display| display.connector == "eDP-1")
        .or_else(|| layout.displays.iter().find(|display| display.primary))
        .or_else(|| layout.displays.first())?;

    if display.connector == "eDP-1" {
        return Some(display_config::zenbook_duo_primary_orientation(
            display.transform,
        ));
    }

    Some(match display.transform {
        90 => Orientation::Left,
        180 => Orientation::Inverted,
        270 => Orientation::Right,
        _ => Orientation::Normal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    fn test_usb_root() -> std::path::PathBuf {
        let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("zenbook-duo-usb-probe-{}-{id}", std::process::id()));
        fs::create_dir_all(&root).expect("create USB probe test directory");
        root
    }

    fn write_usb_identity(root: &Path, name: &str, vendor_id: &str, product_id: &str) {
        let device = root.join(name);
        fs::create_dir_all(&device).expect("create test USB device");
        fs::write(device.join("idVendor"), format!("{vendor_id}\n")).expect("write test vendor id");
        fs::write(device.join("idProduct"), format!("{product_id}\n"))
            .expect("write test product id");
    }

    #[test]
    fn applies_primary_display_transform_to_status() {
        let mut status = DuoStatus::default();
        let layout = DisplayLayout {
            displays: vec![
                crate::models::DisplayInfo {
                    connector: "eDP-1".into(),
                    width: 2880,
                    height: 1800,
                    refresh_rate: 120.0,
                    scale: 1.25,
                    x: 0,
                    y: 0,
                    transform: 90,
                    primary: false,
                    enabled: true,
                    current_mode: crate::models::DisplayMode {
                        mode_id: "2880x1800@120".into(),
                        width: 2880,
                        height: 1800,
                        refresh_rate: 120.0,
                    },
                    available_modes: vec![crate::models::DisplayMode {
                        mode_id: "2880x1800@120".into(),
                        width: 2880,
                        height: 1800,
                        refresh_rate: 120.0,
                    }],
                    refresh_policy: crate::models::RefreshPolicy::Fixed,
                    supports_dynamic_refresh: false,
                },
                crate::models::DisplayInfo {
                    connector: "eDP-2".into(),
                    width: 2880,
                    height: 1800,
                    refresh_rate: 120.0,
                    scale: 1.25,
                    x: 0,
                    y: 1800,
                    transform: 270,
                    primary: true,
                    enabled: true,
                    current_mode: crate::models::DisplayMode {
                        mode_id: "2880x1800@120".into(),
                        width: 2880,
                        height: 1800,
                        refresh_rate: 120.0,
                    },
                    available_modes: vec![crate::models::DisplayMode {
                        mode_id: "2880x1800@120".into(),
                        width: 2880,
                        height: 1800,
                        refresh_rate: 120.0,
                    }],
                    refresh_policy: crate::models::RefreshPolicy::Fixed,
                    supports_dynamic_refresh: false,
                },
            ],
        };

        apply_layout_to_status(&mut status, Some(&layout));

        assert_eq!(status.orientation, Orientation::Left);
        assert_eq!(status.monitor_count, 2);
    }

    #[test]
    fn bluetooth_enabled_requires_a_powered_bluez_controller() {
        assert!(bluetooth_controller_powered("Controller AA:BB\n\tPowered: yes\n"));
        assert!(!bluetooth_controller_powered("Controller AA:BB\n\tPowered: no\n"));
    }

    #[test]
    fn finds_attached_keyboard_by_usb_identity() {
        let root = test_usb_root();
        write_usb_identity(&root, "3-6", "0B05", "1CD7");

        assert!(usb_device_present(&root, "0b05", "1cd7"));

        fs::remove_dir_all(root).expect("remove USB probe test directory");
    }

    #[test]
    fn ignores_other_asus_usb_devices() {
        let root = test_usb_root();
        write_usb_identity(&root, "3-7", "0b05", "1234");

        assert!(!usb_device_present(&root, "0b05", "1cd7"));

        fs::remove_dir_all(root).expect("remove USB probe test directory");
    }

    #[test]
    fn missing_usb_devices_directory_is_not_attached() {
        let root = test_usb_root();
        fs::remove_dir_all(&root).expect("remove USB probe test directory");

        assert!(!usb_device_present(&root, "0b05", "1cd7"));
    }
}
