use std::process::Command;

use crate::models::{ConnectionType, DuoStatus, EventCategory, HardwareEvent};
use crate::runtime::state::RuntimeState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyReason {
    Normal,
    Corrective,
}

#[derive(Debug, Clone)]
pub enum PolicyAction {
    SetWifi { enabled: bool, reason: PolicyReason },
    SetBluetooth { enabled: bool, reason: PolicyReason },
    SetBacklight(u8),
    SetDockMode { attached: bool, scale: f64 },
}

pub fn apply_transition_policy(
    state: &mut RuntimeState,
    previous: &DuoStatus,
) -> Vec<PolicyAction> {
    let mut actions = Vec::new();

    if previous.keyboard_attached == state.status.keyboard_attached {
        // Only refresh remembered radio states during stable periods and when transport
        // is meaningful. rfkill jitter often appears while connection_type is `None`
        // during dock/undock transitions; learning from that state causes preference
        // pollution and unstable corrective behavior.
        let has_reliable_transport = !matches!(state.status.connection_type, ConnectionType::None);
        if has_reliable_transport {
            state.remembered_wifi_enabled = Some(state.status.wifi_enabled);
            state.remembered_bluetooth_enabled = Some(state.status.bluetooth_enabled);
        }

        // When detach/attach transport settles to Bluetooth (often a second step after
        // the physical detach edge), re-apply the remembered keyboard backlight so the
        // wireless keyboard keeps the same level it had before leaving the dock.
        if !state.status.keyboard_attached
            && !matches!(previous.connection_type, ConnectionType::Bluetooth)
            && matches!(state.status.connection_type, ConnectionType::Bluetooth)
        {
            actions.push(PolicyAction::SetBacklight(state.status.backlight_level));
        }
        return actions;
    }

    if state.status.keyboard_attached {
        // Deterministic edge policy (explicitly user requested):
        // - attach => Wi-Fi ON + Bluetooth OFF
        actions.push(PolicyAction::SetWifi {
            enabled: true,
            reason: PolicyReason::Normal,
        });
        actions.push(PolicyAction::SetBluetooth {
            enabled: false,
            reason: PolicyReason::Normal,
        });
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Network,
            "Enforced Wi-Fi enabled on attach",
            "rust-daemon",
        ));
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Bluetooth,
            "Enforced Bluetooth disabled on attach",
            "rust-daemon",
        ));
        state.status.wifi_enabled = true;
        state.status.bluetooth_enabled = false;

        actions.push(PolicyAction::SetBacklight(state.settings.default_backlight));
        actions.push(PolicyAction::SetDockMode {
            attached: true,
            scale: state.settings.default_scale,
        });
    } else {
        // Deterministic edge policy (explicitly user requested):
        // - detach => Wi-Fi ON + Bluetooth ON
        actions.push(PolicyAction::SetWifi {
            enabled: true,
            reason: PolicyReason::Normal,
        });
        actions.push(PolicyAction::SetBluetooth {
            enabled: true,
            reason: PolicyReason::Normal,
        });
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Network,
            "Enforced Wi-Fi enabled on detach",
            "rust-daemon",
        ));
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Bluetooth,
            "Enforced Bluetooth enabled on detach",
            "rust-daemon",
        ));
        actions.push(PolicyAction::SetDockMode {
            attached: false,
            scale: state.settings.default_scale,
        });
        actions.push(PolicyAction::SetBacklight(previous.backlight_level));
        state.status.backlight_level = previous.backlight_level;
        state.status.wifi_enabled = true;
        state.status.bluetooth_enabled = true;
    }

    if state.recent_events.len() > 500 {
        let overflow = state.recent_events.len() - 500;
        state.recent_events.drain(0..overflow);
    }

    actions
}

pub fn set_wifi_enabled(enabled: bool) -> Result<(), String> {
    let target = if enabled { "on" } else { "off" };
    let output = Command::new("nmcli")
        .args(["radio", "wifi", target])
        .output()
        .map_err(|e| format!("Failed to run nmcli: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn set_bluetooth_enabled(enabled: bool) -> Result<(), String> {
    let action = if enabled { "unblock" } else { "block" };
    let output = Command::new("rfkill")
        .args([action, "bluetooth"])
        .output()
        .map_err(|e| format!("Failed to run rfkill: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
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
            last_runtime_notification: None,
            remembered_wifi_enabled: None,
            remembered_bluetooth_enabled: None,
            last_updated: Utc::now(),
            recent_events: Vec::new(),
        }
    }

    #[test]
    fn stable_cycle_updates_remembered_states() {
        let mut state = make_state(make_status(true, true, false));
        state.remembered_wifi_enabled = Some(false);
        state.remembered_bluetooth_enabled = Some(true);
        let previous = state.status.clone();

        let actions = apply_transition_policy(&mut state, &previous);

        assert!(actions.is_empty());
        assert_eq!(state.remembered_wifi_enabled, Some(true));
        assert_eq!(state.remembered_bluetooth_enabled, Some(false));
    }

    #[test]
    fn stable_cycle_with_no_transport_does_not_overwrite_remembered_states() {
        let mut state = make_state(make_status(false, false, false));
        state.status.connection_type = ConnectionType::None;
        state.remembered_wifi_enabled = Some(true);
        state.remembered_bluetooth_enabled = Some(true);
        let mut previous = state.status.clone();
        previous.wifi_enabled = true;
        previous.bluetooth_enabled = true;

        let actions = apply_transition_policy(&mut state, &previous);

        assert!(actions.is_empty());
        assert_eq!(state.remembered_wifi_enabled, Some(true));
        assert_eq!(state.remembered_bluetooth_enabled, Some(true));
    }

    #[test]
    fn attach_edge_forces_wifi_on_and_bluetooth_off() {
        let previous = make_status(false, true, true);
        let mut state = make_state(make_status(true, false, true));

        let actions = apply_transition_policy(&mut state, &previous);

        assert!(actions.iter().any(|action| matches!(
            action,
            PolicyAction::SetWifi {
                enabled: true,
                reason: PolicyReason::Normal
            }
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            PolicyAction::SetBluetooth {
                enabled: false,
                reason: PolicyReason::Normal
            }
        )));
    }

    #[test]
    fn detach_edge_forces_wifi_on_and_bluetooth_on() {
        let mut previous = make_status(true, false, false);
        previous.backlight_level = 3;
        let mut state = make_state(make_status(false, false, false));

        let actions = apply_transition_policy(&mut state, &previous);

        assert!(actions.iter().any(|action| matches!(
            action,
            PolicyAction::SetWifi {
                enabled: true,
                reason: PolicyReason::Normal
            }
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            PolicyAction::SetBluetooth {
                enabled: true,
                reason: PolicyReason::Normal
            }
        )));
        assert!(actions
            .iter()
            .any(|action| matches!(action, PolicyAction::SetBacklight(3))));
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
            .any(|action| matches!(action, PolicyAction::SetBacklight(2))));
    }
}
