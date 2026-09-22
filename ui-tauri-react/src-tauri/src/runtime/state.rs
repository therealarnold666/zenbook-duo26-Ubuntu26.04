use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;

use crate::ipc::protocol::SessionBackend;
use crate::models::{DuoSettings, DuoStatus, HardwareEvent, PerformanceMode};
use crate::runtime::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub status: DuoStatus,
    pub settings: DuoSettings,
    pub session_agent: SessionAgentState,
    #[serde(default)]
    pub usb_media_remap_reconcile: UsbMediaRemapReconcileState,
    /// The user-selected performance mode to restore after an automatic
    /// battery-saver Quiet override ends.
    #[serde(default)]
    pub battery_saver_restore_mode: Option<PerformanceMode>,
    /// True only when this runtime successfully turned off an already-enabled eDP-2
    /// while the keyboard was attached. It distinguishes an intentional dock-mode
    /// change from xe having already disabled the connector after a link failure.
    #[serde(default)]
    pub secondary_panel_disabled_by_runtime: bool,
    /// The kernel boot that owns `secondary_panel_disabled_by_runtime`. Ownership
    /// survives a daemon restart, but is never trusted across a system reboot.
    #[serde(default)]
    pub secondary_panel_disabled_by_runtime_boot_id: Option<String>,
    #[serde(default)]
    pub last_runtime_notification: Option<RuntimeNotificationState>,
    pub last_updated: DateTime<Utc>,
    pub recent_events: Vec<HardwareEvent>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            status: DuoStatus::default(),
            settings: DuoSettings::default(),
            session_agent: SessionAgentState::default(),
            usb_media_remap_reconcile: UsbMediaRemapReconcileState::default(),
            battery_saver_restore_mode: None,
            secondary_panel_disabled_by_runtime: false,
            secondary_panel_disabled_by_runtime_boot_id: None,
            last_runtime_notification: None,
            last_updated: Utc::now(),
            recent_events: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionAgentState {
    pub connected: bool,
    pub session_id: Option<String>,
    pub backend: Option<SessionBackend>,
    pub socket_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsbMediaRemapReconcileState {
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_start_log_at: Option<DateTime<Utc>>,
    pub last_backoff_log_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeNotificationState {
    pub key: String,
    pub emitted_at: DateTime<Utc>,
}

impl RuntimeState {
    pub fn record_successful_dock_mode(
        &mut self,
        attached: bool,
        secondary_panel_was_enabled: bool,
    ) {
        self.secondary_panel_disabled_by_runtime =
            attached && (self.secondary_panel_disabled_by_runtime || secondary_panel_was_enabled);
        self.secondary_panel_disabled_by_runtime_boot_id =
            if self.secondary_panel_disabled_by_runtime {
                current_boot_id()
            } else {
                None
            };
    }

    pub fn validate_secondary_panel_ownership_for_current_boot(&mut self) {
        let current = current_boot_id();
        let ownership_is_current = self.secondary_panel_disabled_by_runtime
            && current.is_some()
            && self.secondary_panel_disabled_by_runtime_boot_id == current;

        if !ownership_is_current {
            self.secondary_panel_disabled_by_runtime = false;
            self.secondary_panel_disabled_by_runtime_boot_id = None;
        }
    }

    pub fn touch(&mut self) {
        self.last_updated = Utc::now();
    }

    pub fn load() -> Self {
        fs::read_to_string(paths::state_file_path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = paths::state_file_path().parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create runtime state dir: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize runtime state: {e}"))?;
        fs::write(paths::state_file_path(), json)
            .map_err(|e| format!("Failed to write runtime state: {e}"))
    }
}

fn current_boot_id() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::RuntimeState;

    #[test]
    fn successful_attached_mode_records_runtime_ownership() {
        let mut state = RuntimeState::default();

        state.record_successful_dock_mode(true, true);

        assert!(state.secondary_panel_disabled_by_runtime);
        assert!(state.secondary_panel_disabled_by_runtime_boot_id.is_some());
    }

    #[test]
    fn repeated_attached_mode_preserves_runtime_ownership() {
        let mut state = RuntimeState {
            secondary_panel_disabled_by_runtime: true,
            ..RuntimeState::default()
        };

        state.record_successful_dock_mode(true, false);

        assert!(state.secondary_panel_disabled_by_runtime);
    }

    #[test]
    fn attached_mode_does_not_claim_an_already_failed_panel() {
        let mut state = RuntimeState::default();

        state.record_successful_dock_mode(true, false);

        assert!(!state.secondary_panel_disabled_by_runtime);
    }

    #[test]
    fn successful_detached_mode_clears_runtime_ownership() {
        let mut state = RuntimeState {
            secondary_panel_disabled_by_runtime: true,
            ..RuntimeState::default()
        };

        state.record_successful_dock_mode(false, false);

        assert!(!state.secondary_panel_disabled_by_runtime);
        assert!(state.secondary_panel_disabled_by_runtime_boot_id.is_none());
    }

    #[test]
    fn current_boot_ownership_survives_daemon_restart() {
        let mut state = RuntimeState::default();
        state.record_successful_dock_mode(true, true);

        state.validate_secondary_panel_ownership_for_current_boot();

        assert!(state.secondary_panel_disabled_by_runtime);
    }

    #[test]
    fn stale_boot_ownership_is_discarded() {
        let mut state = RuntimeState {
            secondary_panel_disabled_by_runtime: true,
            secondary_panel_disabled_by_runtime_boot_id: Some("previous-boot".into()),
            ..RuntimeState::default()
        };

        state.validate_secondary_panel_ownership_for_current_boot();

        assert!(!state.secondary_panel_disabled_by_runtime);
        assert!(state.secondary_panel_disabled_by_runtime_boot_id.is_none());
    }
}
