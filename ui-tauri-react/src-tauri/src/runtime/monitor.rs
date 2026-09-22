use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::RwLock;

use crate::models::{ConnectionType, EventCategory, HardwareEvent, PerformanceMode};
use crate::runtime::logger;
use crate::runtime::policy::PolicyAction;
use crate::runtime::state::RuntimeState;

const BLUETOOTH_BACKLIGHT_MAX_ATTEMPTS: usize = 12;
const BLUETOOTH_BACKLIGHT_RETRY_DELAY: Duration = Duration::from_millis(500);

pub fn start(state: Arc<RwLock<RuntimeState>>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;

            let mut next_status = crate::runtime::probe::current_status();
            let session_connected = {
                let guard = state.read().await;
                guard.session_agent.connected
            };
            next_status.service_active = session_connected;

            if let Some(layout) =
                crate::runtime::daemon::session_display_layout(state.clone()).await
            {
                crate::runtime::probe::apply_layout_to_status(&mut next_status, Some(&layout));
                next_status.service_active = true;
            }

            let mut guard = state.write().await;
            let previous = guard.status.clone();

            if previous != next_status {
                let updated = next_status.clone();
                let _ = logger::append_line(format!(
                    "rust-daemon: status transition attached={} monitors={} wifi={} bluetooth={} connection={}",
                    updated.keyboard_attached,
                    updated.monitor_count,
                    updated.wifi_enabled,
                    updated.bluetooth_enabled,
                    connection_label(&updated.connection_type),
                ));
                guard.status = next_status;
                let actions =
                    crate::runtime::policy::apply_transition_policy(&mut guard, &previous);
                let _ = logger::append_line(format!(
                    "rust-daemon: transition radios prev(wifi={},bt={}) current(wifi={},bt={}) actions=[{}]",
                    previous.wifi_enabled,
                    previous.bluetooth_enabled,
                    updated.wifi_enabled,
                    updated.bluetooth_enabled,
                    summarize_actions(&actions)
                ));
                push_status_events(&mut guard, &previous, &updated);
                guard.touch();
                if let Err(err) = guard.save() {
                    log::warn!("failed to save monitored runtime state: {err}");
                }
                drop(guard);
                apply_policy_actions(state.clone(), actions).await;
            } else {
                drop(guard);
            }

            reconcile_usb_media_remap(state.clone()).await;
            reconcile_auto_quiet_on_battery(state.clone()).await;
        }
    });
}

async fn reconcile_auto_quiet_on_battery(state: Arc<RwLock<RuntimeState>>) {
    let action = {
        let guard = state.read().await;
        auto_quiet_action(&guard, crate::hardware::battery::is_discharging())
    };

    match action {
        AutoQuietAction::None => {}
        AutoQuietAction::ClearRestoreMode => {
            let mut guard = state.write().await;
            guard.battery_saver_restore_mode = None;
            guard.touch();
            if let Err(err) = guard.save() {
                log::warn!("failed to clear battery saver restore mode: {err}");
            }
        }
        AutoQuietAction::EnterQuiet {
            restore_mode,
            limits,
        } => match crate::hardware::power::apply_performance_mode(&PerformanceMode::Quiet, &limits)
        {
            Ok(()) => {
                let mut guard = state.write().await;
                guard.battery_saver_restore_mode = Some(restore_mode);
                guard.settings.active_performance_mode = PerformanceMode::Quiet;
                record_auto_performance_change(
                    &mut guard,
                    "Switched performance mode to Quiet on battery power",
                );
                let _ = logger::append_line(
                    "rust-daemon: auto battery saver switched performance mode -> quiet",
                );
            }
            Err(err) => log_auto_performance_error("switch to Quiet on battery power", err),
        },
        AutoQuietAction::Restore { mode, limits } => {
            match crate::hardware::power::apply_performance_mode(&mode, &limits) {
                Ok(()) => {
                    let mut guard = state.write().await;
                    guard.battery_saver_restore_mode = None;
                    guard.settings.active_performance_mode = mode.clone();
                    record_auto_performance_change(
                        &mut guard,
                        &format!(
                            "Restored {} performance mode on AC power",
                            mode_label(&mode)
                        ),
                    );
                    let _ = logger::append_line(format!(
                        "rust-daemon: auto battery saver restored performance mode -> {}",
                        mode_label(&mode)
                    ));
                }
                Err(err) => log_auto_performance_error("restore performance mode on AC power", err),
            }
        }
    }
}

enum AutoQuietAction {
    None,
    ClearRestoreMode,
    EnterQuiet {
        restore_mode: PerformanceMode,
        limits: crate::models::PowerLimits,
    },
    Restore {
        mode: PerformanceMode,
        limits: crate::models::PowerLimits,
    },
}

fn auto_quiet_action(state: &RuntimeState, on_battery_power: bool) -> AutoQuietAction {
    if !state.settings.auto_quiet_on_battery {
        return state
            .battery_saver_restore_mode
            .is_some()
            .then_some(AutoQuietAction::ClearRestoreMode)
            .unwrap_or(AutoQuietAction::None);
    }

    if on_battery_power {
        return match (
            &state.battery_saver_restore_mode,
            &state.settings.active_performance_mode,
        ) {
            (None, mode) if *mode != PerformanceMode::Quiet => AutoQuietAction::EnterQuiet {
                restore_mode: mode.clone(),
                limits: state.settings.performance_profiles.quiet.clone(),
            },
            _ => AutoQuietAction::None,
        };
    }

    state
        .battery_saver_restore_mode
        .as_ref()
        .map(|mode| AutoQuietAction::Restore {
            mode: mode.clone(),
            limits: state.settings.performance_profiles.for_mode(mode).clone(),
        })
        .unwrap_or(AutoQuietAction::None)
}

fn record_auto_performance_change(state: &mut RuntimeState, message: &str) {
    state.recent_events.push(HardwareEvent::info(
        EventCategory::Service,
        message,
        "rust-daemon",
    ));
    state.touch();
    if let Err(err) = state.save() {
        log::warn!("failed to persist automatic performance mode: {err}");
    }
}

fn log_auto_performance_error(action: &str, err: String) {
    log::warn!("failed to {action}: {err}");
    let _ = logger::append_line(format!("rust-daemon: failed to {action}: {err}"));
}

fn mode_label(mode: &PerformanceMode) -> &'static str {
    match mode {
        PerformanceMode::Quiet => "Quiet",
        PerformanceMode::Balanced => "Balanced",
        PerformanceMode::Performance => "Performance",
    }
}

async fn reconcile_usb_media_remap(state: Arc<RwLock<RuntimeState>>) {
    const AUTO_START_RETRY_COOLDOWN_SECS: i64 = 15;

    let (should_run, is_running) = {
        let guard = state.read().await;
        let should_run = guard.status.keyboard_attached
            && matches!(guard.status.connection_type, ConnectionType::Usb)
            && guard.session_agent.connected
            && guard.settings.usb_media_remap_enabled;
        let is_running = crate::commands::usb_media_remap::get_status().running;
        (should_run, is_running)
    };

    if should_run == is_running {
        if should_run {
            let mut guard = state.write().await;
            guard.usb_media_remap_reconcile.last_started_at = None;
            guard.usb_media_remap_reconcile.last_backoff_log_at = None;
        }
        return;
    }

    if should_run {
        let now = Utc::now();
        {
            let mut guard = state.write().await;
            if let Some(last_started_at) = guard.usb_media_remap_reconcile.last_started_at {
                if (now - last_started_at).num_seconds() < AUTO_START_RETRY_COOLDOWN_SECS {
                    if guard
                        .usb_media_remap_reconcile
                        .last_backoff_log_at
                        .map(|last_backoff_log_at| {
                            (now - last_backoff_log_at).num_seconds()
                                >= AUTO_START_RETRY_COOLDOWN_SECS
                        })
                        .unwrap_or(true)
                    {
                        guard.usb_media_remap_reconcile.last_backoff_log_at = Some(now);
                        let _ = logger::append_line(format!(
                            "rust-daemon: usb media remap auto-start backing off for {}s after repeated failures",
                            AUTO_START_RETRY_COOLDOWN_SECS
                        ));
                    }
                    return;
                }
            }
            guard.usb_media_remap_reconcile.last_started_at = Some(now);
            guard.usb_media_remap_reconcile.last_backoff_log_at = None;
        }

        match crate::commands::usb_media_remap::start_remap() {
            Ok(()) => {
                let mut should_log = false;
                {
                    let mut guard = state.write().await;
                    if guard
                        .usb_media_remap_reconcile
                        .last_start_log_at
                        .map(|last_log_at| {
                            (now - last_log_at).num_seconds() >= AUTO_START_RETRY_COOLDOWN_SECS
                        })
                        .unwrap_or(true)
                    {
                        guard.usb_media_remap_reconcile.last_start_log_at = Some(now);
                        should_log = true;
                    }
                }
                if should_log {
                    let _ =
                        logger::append_line("rust-daemon: reconciled usb media remap -> started");
                }
            }
            Err(err) => {
                log::warn!("failed to auto-start usb media remap: {err}");
                crate::runtime::daemon::notify_runtime_error(
                    &state,
                    "Zenbook Duo Runtime Error",
                    &format!("USB media remap auto-start failed: {err}"),
                )
                .await;
                let _ = logger::append_line(format!(
                    "rust-daemon: usb media remap auto-start failed: {}",
                    err
                ));
            }
        }
    } else if let Err(err) = crate::commands::usb_media_remap::stop_remap() {
        log::warn!("failed to auto-stop usb media remap: {err}");
        crate::runtime::daemon::notify_runtime_error(
            &state,
            "Zenbook Duo Runtime Error",
            &format!("USB media remap auto-stop failed: {err}"),
        )
        .await;
        let _ = logger::append_line(format!(
            "rust-daemon: usb media remap auto-stop failed: {}",
            err
        ));
    } else {
        let mut guard = state.write().await;
        guard.usb_media_remap_reconcile.last_started_at = None;
        guard.usb_media_remap_reconcile.last_backoff_log_at = None;
        drop(guard);
        let _ = logger::append_line("rust-daemon: reconciled usb media remap -> stopped");
    }
}

async fn apply_policy_actions(state: Arc<RwLock<RuntimeState>>, actions: Vec<PolicyAction>) {
    for action in actions {
        match action {
            PolicyAction::EnsureBluetoothEnabled => {
                if let Err(err) = crate::runtime::policy::ensure_bluetooth_enabled() {
                    log::warn!("failed to ensure Bluetooth is enabled: {err}");
                    crate::runtime::daemon::notify_runtime_error(
                        &state,
                        "Zenbook Duo Runtime Error",
                        &format!("Could not enable Bluetooth for detached keyboard: {err}"),
                    )
                    .await;
                    let _ = logger::append_line(format!(
                        "rust-daemon: ensure Bluetooth enabled on detach failed: {}",
                        err
                    ));
                } else {
                    let _ = logger::append_line("rust-daemon: ensured Bluetooth enabled on detach");
                }
            }
            PolicyAction::SetBacklight(level) => {
                if let Err(err) = crate::hardware::hid::set_backlight(level) {
                    log::warn!("failed to set backlight policy action: {err}");
                    crate::runtime::daemon::notify_runtime_error(
                        &state,
                        "Zenbook Duo Runtime Error",
                        &format!("Backlight policy action failed: {err}"),
                    )
                    .await;
                    let _ = logger::append_line(format!(
                        "rust-daemon: backlight policy action failed (level={}): {}",
                        level, err
                    ));
                } else {
                    {
                        let mut guard = state.write().await;
                        guard.status.backlight_level = level;
                        guard.recent_events.push(HardwareEvent::info(
                            EventCategory::Keyboard,
                            format!("Backlight set to {}", level),
                            "rust-daemon",
                        ));
                        guard.touch();
                        if let Err(err) = guard.save() {
                            log::warn!("failed to save backlight policy state: {err}");
                        }
                    }
                    let _ = logger::append_line(format!(
                        "rust-daemon: applied backlight policy action -> {}",
                        level
                    ));
                }
            }
            PolicyAction::SetBluetoothBacklight(level) => {
                match set_bluetooth_backlight_when_ready(level).await {
                    Ok(()) => {
                        let mut guard = state.write().await;
                        guard.status.backlight_level = level;
                        guard.recent_events.push(HardwareEvent::info(
                            EventCategory::Keyboard,
                            format!("Backlight restored to {} over Bluetooth", level),
                            "rust-daemon",
                        ));
                        guard.touch();
                        if let Err(err) = guard.save() {
                            log::warn!("failed to save Bluetooth backlight state: {err}");
                        }
                    }
                    Err(err) => {
                        log::warn!("failed to restore Bluetooth backlight: {err}");
                        crate::runtime::daemon::notify_runtime_error(
                            &state,
                            "Zenbook Duo Runtime Error",
                            &format!("Bluetooth keyboard backlight restore failed: {err}"),
                        )
                        .await;
                    }
                }
            }
            PolicyAction::SetDockMode { attached, scale } => {
                let secondary_panel_was_enabled = crate::hardware::sysfs::secondary_panel_enabled();
                let secondary_panel_disabled_by_runtime = {
                    let guard = state.read().await;
                    guard.secondary_panel_disabled_by_runtime
                };

                if should_skip_detached_secondary_recovery(
                    attached,
                    secondary_panel_was_enabled,
                    secondary_panel_disabled_by_runtime,
                ) {
                    let message = "Skipping automatic dual-screen enable: xe reports eDP-2 disabled after the keyboard transition";
                    log::warn!("{message}");
                    crate::runtime::daemon::notify_runtime_error(
                        &state,
                        "Zenbook Duo Display Protection",
                        "The xe driver has disabled eDP-2. Automatic dual-screen recovery was skipped to avoid a compositor freeze; reconnect the keyboard or reboot before retrying.",
                    )
                    .await;
                    let _ = logger::append_line(format!(
                        "rust-daemon: {message} (attached={}, scale={})",
                        attached, scale
                    ));
                    continue;
                }

                if let Err(err) =
                    crate::runtime::daemon::forward_or_queue_dock_mode(&state, attached, scale)
                        .await
                {
                    if err == "No session agent registered" {
                        let _ = logger::append_line(format!(
                            "rust-daemon: recoverable_pending_replay dock-mode policy action (attached={}, scale={}) because no session agent is registered yet",
                            attached, scale
                        ));
                        continue;
                    }
                    log::warn!("failed to apply dock-mode policy action: {err}");
                    crate::runtime::daemon::notify_runtime_error(
                        &state,
                        "Zenbook Duo Runtime Error",
                        &format!("Dock-mode policy action failed: {err}"),
                    )
                    .await;
                    let _ = logger::append_line(format!(
                        "rust-daemon: dock-mode policy action failed (attached={}, scale={}): {}",
                        attached, scale, err
                    ));
                } else {
                    let mut guard = state.write().await;
                    // Preserve ownership across repeated attached syncs. The replay path
                    // also records this so startup and session-agent re-registration behave
                    // the same as a physical keyboard transition.
                    guard.record_successful_dock_mode(attached, secondary_panel_was_enabled);
                    guard.touch();
                    if let Err(err) = guard.save() {
                        log::warn!("failed to save secondary panel dock state: {err}");
                    }
                    drop(guard);
                    let _ = logger::append_line(format!(
                        "rust-daemon: applied dock-mode policy action (attached={}, scale={})",
                        attached, scale
                    ));
                }
            }
        }
    }
}

fn should_skip_detached_secondary_recovery(
    attached: bool,
    secondary_panel_enabled: bool,
    secondary_panel_disabled_by_runtime: bool,
) -> bool {
    !attached && !secondary_panel_enabled && !secondary_panel_disabled_by_runtime
}

fn summarize_actions(actions: &[PolicyAction]) -> String {
    if actions.is_empty() {
        return "none".to_string();
    }
    actions
        .iter()
        .map(|action| match action {
            PolicyAction::EnsureBluetoothEnabled => "bluetooth:ensure-enabled".to_string(),
            PolicyAction::SetBacklight(level) => format!("backlight:{}", level),
            PolicyAction::SetBluetoothBacklight(level) => format!("bluetooth-backlight:{}", level),
            PolicyAction::SetDockMode { attached, scale } => {
                format!("dock:attached={}:scale={}", attached, scale)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

async fn set_bluetooth_backlight_when_ready(level: u8) -> Result<(), String> {
    let mut last_error = String::from("Bluetooth HID device has not appeared yet");

    for attempt in 1..=BLUETOOTH_BACKLIGHT_MAX_ATTEMPTS {
        match tokio::task::spawn_blocking(move || {
            crate::hardware::hid::set_backlight_bluetooth(level)
        })
        .await
        {
            Ok(Ok(())) => {
                let _ = logger::append_line(format!(
                    "rust-daemon: restored Bluetooth backlight level={} attempt={}/{}",
                    level, attempt, BLUETOOTH_BACKLIGHT_MAX_ATTEMPTS
                ));
                return Ok(());
            }
            Ok(Err(err)) => last_error = err,
            Err(err) => last_error = format!("Bluetooth backlight worker failed: {err}"),
        }

        if attempt < BLUETOOTH_BACKLIGHT_MAX_ATTEMPTS {
            tokio::time::sleep(BLUETOOTH_BACKLIGHT_RETRY_DELAY).await;
        }
    }

    Err(format!(
        "after {} attempts: {}",
        BLUETOOTH_BACKLIGHT_MAX_ATTEMPTS, last_error
    ))
}

fn push_status_events(
    state: &mut RuntimeState,
    old: &crate::models::DuoStatus,
    new: &crate::models::DuoStatus,
) {
    if old.keyboard_attached != new.keyboard_attached {
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Usb,
            if new.keyboard_attached {
                "Keyboard attached"
            } else {
                "Keyboard detached"
            },
            "rust-daemon",
        ));
    }

    if old.connection_type != new.connection_type {
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Keyboard,
            format!(
                "Connection type changed to {}",
                connection_label(&new.connection_type)
            ),
            "rust-daemon",
        ));
    }

    if old.wifi_enabled != new.wifi_enabled {
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Network,
            if new.wifi_enabled {
                "Wi-Fi enabled"
            } else {
                "Wi-Fi disabled"
            },
            "rust-daemon",
        ));
    }

    if old.bluetooth_enabled != new.bluetooth_enabled {
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Bluetooth,
            if new.bluetooth_enabled {
                "Bluetooth enabled"
            } else {
                "Bluetooth disabled"
            },
            "rust-daemon",
        ));
    }

    if old.monitor_count != new.monitor_count {
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Display,
            format!("Monitor count changed to {}", new.monitor_count),
            "rust-daemon",
        ));
    }

    if old.orientation != new.orientation {
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Rotation,
            format!(
                "Orientation changed to {}",
                orientation_label(&new.orientation)
            ),
            "rust-daemon",
        ));
    }

    if old.backlight_level != new.backlight_level {
        state.recent_events.push(HardwareEvent::info(
            EventCategory::Keyboard,
            format!("Backlight level changed to {}", new.backlight_level),
            "rust-daemon",
        ));
    }

    if state.recent_events.len() > 500 {
        let overflow = state.recent_events.len() - 500;
        state.recent_events.drain(0..overflow);
    }
}

fn connection_label(connection_type: &ConnectionType) -> &'static str {
    match connection_type {
        ConnectionType::Usb => "usb",
        ConnectionType::Bluetooth => "bluetooth",
        ConnectionType::None => "none",
    }
}

fn orientation_label(orientation: &crate::models::Orientation) -> &'static str {
    match orientation {
        crate::models::Orientation::Normal => "normal",
        crate::models::Orientation::Left => "left",
        crate::models::Orientation::Right => "right",
        crate::models::Orientation::Inverted => "inverted",
    }
}

#[cfg(test)]
mod tests {
    use super::{auto_quiet_action, should_skip_detached_secondary_recovery, AutoQuietAction};
    use crate::models::{PerformanceMode, PowerLimits};
    use crate::runtime::state::RuntimeState;

    #[test]
    fn battery_saver_restores_the_pre_battery_mode_on_ac() {
        let mut state = RuntimeState::default();
        state.settings.auto_quiet_on_battery = true;
        state.settings.active_performance_mode = PerformanceMode::Performance;

        let enter = auto_quiet_action(&state, true);
        assert!(matches!(
            enter,
            AutoQuietAction::EnterQuiet {
                restore_mode: PerformanceMode::Performance,
                ..
            }
        ));

        state.settings.active_performance_mode = PerformanceMode::Quiet;
        state.battery_saver_restore_mode = Some(PerformanceMode::Performance);
        let restore = auto_quiet_action(&state, false);
        assert!(matches!(
            restore,
            AutoQuietAction::Restore {
                mode: PerformanceMode::Performance,
                limits: PowerLimits { pl1_watts: 45, .. },
            }
        ));
    }

    #[test]
    fn detached_recovery_is_allowed_after_runtime_disabled_the_panel() {
        assert!(!should_skip_detached_secondary_recovery(false, false, true));
    }

    #[test]
    fn detached_recovery_is_blocked_when_xe_disabled_the_panel() {
        assert!(should_skip_detached_secondary_recovery(false, false, false));
    }

    #[test]
    fn enabled_panel_never_needs_detached_recovery_protection() {
        assert!(!should_skip_detached_secondary_recovery(false, true, false));
    }
}
