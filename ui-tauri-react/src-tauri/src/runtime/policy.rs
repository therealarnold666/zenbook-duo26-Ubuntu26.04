use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::models::{ConnectionType, DuoStatus, EventCategory, HardwareEvent};
use crate::runtime::state::RuntimeState;

#[derive(Debug, Clone)]
pub enum PolicyAction {
    EnsureBluetoothEnabled,
    SetBacklight(u8),
    SetBluetoothBacklight(u8),
    SetDockMode { attached: bool, scale: f64 },
}

pub fn apply_transition_policy(
    state: &mut RuntimeState,
    previous: &DuoStatus,
) -> Vec<PolicyAction> {
    let mut actions = Vec::new();

    if previous.keyboard_attached == state.status.keyboard_attached {
        // When detach/attach transport settles to Bluetooth (often a second step after
        // the physical detach edge), re-apply the remembered keyboard backlight so the
        // wireless keyboard keeps the same level it had before leaving the dock.
        if !state.status.keyboard_attached
            && !matches!(previous.connection_type, ConnectionType::Bluetooth)
            && matches!(state.status.connection_type, ConnectionType::Bluetooth)
        {
            actions.push(PolicyAction::SetBluetoothBacklight(
                state.status.backlight_level,
            ));
        }
        return actions;
    }

    if state.status.keyboard_attached {
        actions.push(PolicyAction::SetBacklight(state.settings.default_backlight));
        actions.push(PolicyAction::SetDockMode {
            attached: true,
            scale: state.settings.default_scale,
        });
    } else {
        // The detached keyboard needs Bluetooth, but radio policy otherwise belongs
        // to the user and desktop environment.
        actions.push(PolicyAction::EnsureBluetoothEnabled);
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Bluetooth,
            "Ensuring Bluetooth is enabled for detached keyboard",
            "rust-daemon",
        ));
        actions.push(PolicyAction::SetDockMode {
            attached: false,
            scale: state.settings.default_scale,
        });
        // The wireless HID device is normally enumerated after this edge.  Do not
        // write through a disappearing USB device; wait for the Bluetooth transport.
        if matches!(state.status.connection_type, ConnectionType::Bluetooth) {
            actions.push(PolicyAction::SetBluetoothBacklight(
                previous.backlight_level,
            ));
        }
        state.status.backlight_level = previous.backlight_level;
    }

    if state.recent_events.len() > 500 {
        let overflow = state.recent_events.len() - 500;
        state.recent_events.drain(0..overflow);
    }

    actions
}

pub fn ensure_bluetooth_enabled() -> Result<(), String> {
    let output = Command::new("rfkill")
        .args(["unblock", "bluetooth"])
        .output()
        .map_err(|e| format!("Failed to run rfkill: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "rfkill unblock bluetooth failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    // RFKill only removes a block. BlueZ still needs to power the controller
    // before the detached keyboard can reconnect.
    let mut failures = Vec::new();
    for attempt in 1..=3 {
        let output = Command::new("bluetoothctl")
            .args(["power", "on"])
            .output()
            .map_err(|e| format!("Failed to run bluetoothctl: {e}"))?;

        if output.status.success() && crate::runtime::probe::bluetooth_enabled() {
            return Ok(());
        }

        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if detail.is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            detail
        };
        failures.push(format!("attempt {attempt}: {detail}"));
        if attempt < 3 {
            thread::sleep(Duration::from_millis(500));
        }
    }

    Err(format!(
        "BlueZ controller did not reach Powered: yes after rfkill unblock ({})",
        failures.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ConnectionType, DuoSettings, Orientation};
    use chrono::Utc;

    fn make_status(attached: bool, wifi: bool, bluetooth: bool) -> DuoStatus {
        DuoStatus {
            keyboard_attached: attached,
            connection_type: ConnectionType::Usb,
            monitor_count: 2,
            wifi_enabled: wifi,
            bluetooth_enabled: bluetooth,
            backlight_level: 0,
            display_brightness: 0,
            max_brightness: 0,
            service_active: true,
            orientation: Orientation::Normal,
        }
    }

    fn make_state(current: DuoStatus) -> RuntimeState {
        RuntimeState {
            status: current,
            settings: DuoSettings::default(),
            session_agent: Default::default(),
            usb_media_remap_reconcile: Default::default(),
            battery_saver_restore_mode: None,
            secondary_panel_disabled_by_runtime: false,
            secondary_panel_disabled_by_runtime_boot_id: None,
            last_runtime_notification: None,
            last_updated: Utc::now(),
            recent_events: Vec::new(),
        }
    }

    #[test]
    fn stable_cycle_does_not_change_radio_state() {
        let mut state = make_state(make_status(true, true, false));
        let previous = state.status.clone();

        let actions = apply_transition_policy(&mut state, &previous);

        assert!(actions.is_empty());
    }

    #[test]
    fn attach_edge_does_not_change_radio_state() {
        let previous = make_status(false, true, true);
        let mut state = make_state(make_status(true, false, true));

        let actions = apply_transition_policy(&mut state, &previous);

        assert!(!actions
            .iter()
            .any(|action| matches!(action, PolicyAction::EnsureBluetoothEnabled)));
    }

    #[test]
    fn detach_edge_ensures_bluetooth_is_enabled() {
        let mut previous = make_status(true, false, false);
        previous.backlight_level = 3;
        let mut state = make_state(make_status(false, false, false));

        let actions = apply_transition_policy(&mut state, &previous);

        assert!(actions
            .iter()
            .any(|action| matches!(action, PolicyAction::EnsureBluetoothEnabled)));
        assert!(!actions
            .iter()
            .any(|action| matches!(action, PolicyAction::SetBluetoothBacklight(_))));
    }

    #[test]
    fn stable_transition_to_bluetooth_reapplies_backlight() {
        let mut previous = make_status(false, true, true);
        previous.connection_type = ConnectionType::None;

        let mut state = make_state(make_status(false, true, true));
        state.status.connection_type = ConnectionType::Bluetooth;
        state.status.backlight_level = 2;

        let actions = apply_transition_policy(&mut state, &previous);

        assert!(actions
            .iter()
            .any(|action| matches!(action, PolicyAction::SetBluetoothBacklight(2))));
    }
}
