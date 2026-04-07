use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::{Mutex, RwLock};

use crate::models::{ConnectionType, EventCategory, HardwareEvent};
use crate::runtime::logger;
use crate::runtime::policy::{PolicyAction, PolicyReason};
use crate::runtime::state::RuntimeState;

const EDGE_DEBOUNCE_MS: u64 = 400;
const EDGE_FAST_TICKS: usize = 10;
const EDGE_FAST_INTERVAL_MS: u64 = 150;
const EDGE_SLOW_TICKS: usize = 14;
const EDGE_SLOW_INTERVAL_MS: u64 = 500;
const EDGE_REQUIRED_STABLE_TICKS: usize = 3;
const EDGE_MAX_CYCLES: usize = 4;

static EDGE_GUARD_COORDINATOR: OnceLock<Mutex<EdgeGuardCoordinator>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EdgeGuardExpectation {
    edge_id: u64,
    attached_after_edge: bool,
    expected_wifi_on: bool,
    expected_bluetooth_on: bool,
}

impl EdgeGuardExpectation {
    fn from_inputs(edge_id: u64, attached_after_edge: bool) -> Self {
        Self {
            edge_id,
            attached_after_edge,
            expected_wifi_on: true,
            expected_bluetooth_on: if attached_after_edge { false } else { true },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeGuardRegisterOutcome {
    Start {
        expectation: EdgeGuardExpectation,
        start_delay: Duration,
    },
    Merged {
        edge_id: u64,
    },
}

#[derive(Debug, Default)]
struct EdgeGuardCoordinator {
    active: bool,
    next_edge_id: u64,
    last_edge_at: Option<Instant>,
    pending: Option<EdgeGuardExpectation>,
}

impl EdgeGuardCoordinator {
    fn register_edge(
        &mut self,
        now: Instant,
        attached_after_edge: bool,
    ) -> EdgeGuardRegisterOutcome {
        self.next_edge_id = self.next_edge_id.saturating_add(1);
        let edge_id = self.next_edge_id;
        let expectation = EdgeGuardExpectation::from_inputs(edge_id, attached_after_edge);
        let previous_edge = self.last_edge_at;
        self.last_edge_at = Some(now);

        if self.active {
            self.pending = Some(expectation);
            return EdgeGuardRegisterOutcome::Merged { edge_id };
        }

        self.active = true;
        let debounce_window = Duration::from_millis(EDGE_DEBOUNCE_MS);
        let start_delay = previous_edge
            .map(|last| debounce_remaining(now, last, debounce_window))
            .unwrap_or(Duration::ZERO);
        EdgeGuardRegisterOutcome::Start {
            expectation,
            start_delay,
        }
    }

    fn take_pending(&mut self) -> Option<EdgeGuardExpectation> {
        self.pending.take()
    }

    fn finish(&mut self) {
        self.active = false;
        self.pending = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EdgeGuardCycleResult {
    recovered: bool,
    checks: usize,
    corrections: usize,
    duration_ms: u128,
}

fn edge_guard_coordinator() -> &'static Mutex<EdgeGuardCoordinator> {
    EDGE_GUARD_COORDINATOR.get_or_init(|| Mutex::new(EdgeGuardCoordinator::default()))
}

fn debounce_remaining(now: Instant, last: Instant, window: Duration) -> Duration {
    let elapsed = now.saturating_duration_since(last);
    if elapsed >= window {
        Duration::ZERO
    } else {
        window - elapsed
    }
}

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
                let keyboard_edge = previous.keyboard_attached != updated.keyboard_attached;
                let _ = logger::append_line(format!(
                    "rust-daemon: transition radios prev(wifi={},bt={}) current(wifi={},bt={}) remembered(wifi={:?},bt={:?}) actions=[{}]",
                    previous.wifi_enabled,
                    previous.bluetooth_enabled,
                    updated.wifi_enabled,
                    updated.bluetooth_enabled,
                    guard.remembered_wifi_enabled,
                    guard.remembered_bluetooth_enabled,
                    summarize_actions(&actions)
                ));
                push_status_events(&mut guard, &previous, &updated);
                guard.touch();
                if let Err(err) = guard.save() {
                    log::warn!("failed to save monitored runtime state: {err}");
                }
                drop(guard);
                apply_policy_actions(state.clone(), actions).await;
                if keyboard_edge {
                    start_edge_radio_guard(state.clone(), updated.keyboard_attached);
                }
            } else {
                drop(guard);
            }

            reconcile_usb_media_remap(state.clone()).await;
        }
    });
}

fn start_edge_radio_guard(state: Arc<RwLock<RuntimeState>>, attached_after_edge: bool) {
    tokio::spawn(async move {
        let register_outcome = {
            let mut coordinator = edge_guard_coordinator().lock().await;
            coordinator.register_edge(Instant::now(), attached_after_edge)
        };

        let mut expectation = match register_outcome {
            EdgeGuardRegisterOutcome::Merged { edge_id } => {
                let _ = logger::append_line(format!(
                    "rust-daemon: edge radio guard merged new edge into active guard (edge_id={})",
                    edge_id
                ));
                return;
            }
            EdgeGuardRegisterOutcome::Start {
                expectation,
                start_delay,
            } => {
                if !start_delay.is_zero() {
                    let _ = logger::append_line(format!(
                        "rust-daemon: edge radio guard debounce delay {}ms before start (edge_id={})",
                        start_delay.as_millis(),
                        expectation.edge_id
                    ));
                    tokio::time::sleep(start_delay).await;
                }
                expectation
            }
        };

        for cycle_index in 0..EDGE_MAX_CYCLES {
            let cycle = run_edge_guard_cycle(state.clone(), expectation).await;
            let has_pending = {
                let mut coordinator = edge_guard_coordinator().lock().await;
                if let Some(next) = coordinator.take_pending() {
                    expectation = next;
                    true
                } else {
                    coordinator.finish();
                    false
                }
            };

            let _ = logger::append_line(format!(
                "rust-daemon: edge radio guard cycle done edge_id={} recovered={} checks={} corrections={} duration_ms={} pending={}",
                expectation.edge_id,
                cycle.recovered,
                cycle.checks,
                cycle.corrections,
                cycle.duration_ms,
                has_pending
            ));

            if !has_pending {
                break;
            }

            if cycle_index + 1 == EDGE_MAX_CYCLES {
                let mut coordinator = edge_guard_coordinator().lock().await;
                coordinator.finish();
                let _ = logger::append_line(format!(
                    "rust-daemon: edge radio guard reached max cycles {}; finishing with latest edge_id={}",
                    EDGE_MAX_CYCLES,
                    expectation.edge_id
                ));
            }
        }
    });
}

async fn run_edge_guard_cycle(
    state: Arc<RwLock<RuntimeState>>,
    expectation: EdgeGuardExpectation,
) -> EdgeGuardCycleResult {
    let started = Instant::now();
    let mut checks = 0usize;
    let mut corrections = 0usize;
    let mut stable_streak = 0usize;
    let phase_specs = [
        ("fast", EDGE_FAST_TICKS, EDGE_FAST_INTERVAL_MS),
        ("slow", EDGE_SLOW_TICKS, EDGE_SLOW_INTERVAL_MS),
    ];

    let _ = logger::append_line(format!(
        "rust-daemon: edge radio guard start edge_id={} attached={} expected(wifi_on={},bt_on={}) phases=fast({}x{}ms),slow({}x{}ms)",
        expectation.edge_id,
        expectation.attached_after_edge,
        expectation.expected_wifi_on,
        expectation.expected_bluetooth_on,
        EDGE_FAST_TICKS,
        EDGE_FAST_INTERVAL_MS,
        EDGE_SLOW_TICKS,
        EDGE_SLOW_INTERVAL_MS
    ));

    for (phase_name, phase_ticks, phase_interval_ms) in phase_specs {
        for _ in 0..phase_ticks {
            tokio::time::sleep(Duration::from_millis(phase_interval_ms)).await;
            checks = checks.saturating_add(1);

            let mut actual_wifi = crate::runtime::probe::wifi_enabled();
            let mut actual_bluetooth = crate::runtime::probe::bluetooth_enabled();

            if expectation.expected_wifi_on && !actual_wifi {
                if let Err(err) = crate::runtime::policy::set_wifi_enabled(true) {
                    let _ = logger::append_line(format!(
                        "rust-daemon: edge radio guard corrective action failed edge_id={} phase={} action=wifi_on err={}",
                        expectation.edge_id, phase_name, err
                    ));
                } else {
                    corrections = corrections.saturating_add(1);
                    let _ = logger::append_line(format!(
                        "rust-daemon: edge radio guard corrective action applied edge_id={} phase={} action=wifi_on",
                        expectation.edge_id, phase_name
                    ));
                    actual_wifi = crate::runtime::probe::wifi_enabled();
                    let mut guard = state.write().await;
                    guard.status.wifi_enabled = actual_wifi;
                    guard.touch();
                    let _ = guard.save();
                }
            }

            if expectation.expected_bluetooth_on && !actual_bluetooth {
                if let Err(err) = crate::runtime::policy::set_bluetooth_enabled(true) {
                    let _ = logger::append_line(format!(
                        "rust-daemon: edge radio guard corrective action failed edge_id={} phase={} action=bluetooth_on err={}",
                        expectation.edge_id, phase_name, err
                    ));
                } else {
                    corrections = corrections.saturating_add(1);
                    let _ = logger::append_line(format!(
                        "rust-daemon: edge radio guard corrective action applied edge_id={} phase={} action=bluetooth_on",
                        expectation.edge_id, phase_name
                    ));
                    actual_bluetooth = crate::runtime::probe::bluetooth_enabled();
                    let mut guard = state.write().await;
                    guard.status.bluetooth_enabled = actual_bluetooth;
                    guard.touch();
                    let _ = guard.save();
                }
            }

            let wifi_ok = !expectation.expected_wifi_on || actual_wifi;
            let bluetooth_ok = !expectation.expected_bluetooth_on || actual_bluetooth;
            if wifi_ok && bluetooth_ok {
                stable_streak = stable_streak.saturating_add(1);
                if stable_streak >= EDGE_REQUIRED_STABLE_TICKS {
                    let duration_ms = started.elapsed().as_millis();
                    return EdgeGuardCycleResult {
                        recovered: true,
                        checks,
                        corrections,
                        duration_ms,
                    };
                }
            } else {
                stable_streak = 0;
            }
        }
    }

    let final_wifi = crate::runtime::probe::wifi_enabled();
    let final_bluetooth = crate::runtime::probe::bluetooth_enabled();
    let recovered = (!expectation.expected_wifi_on || final_wifi)
        && (!expectation.expected_bluetooth_on || final_bluetooth);

    EdgeGuardCycleResult {
        recovered,
        checks,
        corrections,
        duration_ms: started.elapsed().as_millis(),
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
            PolicyAction::SetWifi { enabled, reason } => {
                if let Err(err) = crate::runtime::policy::set_wifi_enabled(enabled) {
                    log::warn!("failed to set wifi policy action: {err}");
                    crate::runtime::daemon::notify_runtime_error(
                        &state,
                        "Zenbook Duo Runtime Error",
                        &format!("Wi-Fi policy action failed: {err}"),
                    )
                    .await;
                    let _ = logger::append_line(format!(
                        "rust-daemon: wifi policy action failed (enabled={}, reason={}): {}",
                        enabled,
                        reason_label(reason),
                        err
                    ));
                } else {
                    let _ = logger::append_line(format!(
                        "rust-daemon: applied wifi policy action -> {} ({})",
                        enabled,
                        reason_label(reason)
                    ));
                }
            }
            PolicyAction::SetBluetooth { enabled, reason } => {
                if let Err(err) = crate::runtime::policy::set_bluetooth_enabled(enabled) {
                    log::warn!("failed to set bluetooth policy action: {err}");
                    crate::runtime::daemon::notify_runtime_error(
                        &state,
                        "Zenbook Duo Runtime Error",
                        &format!("Bluetooth policy action failed: {err}"),
                    )
                    .await;
                    let _ = logger::append_line(format!(
                        "rust-daemon: bluetooth policy action failed (enabled={}, reason={}): {}",
                        enabled,
                        reason_label(reason),
                        err
                    ));
                } else {
                    let _ = logger::append_line(format!(
                        "rust-daemon: applied bluetooth policy action -> {} ({})",
                        enabled,
                        reason_label(reason)
                    ));
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
            PolicyAction::SetDockMode { attached, scale } => {
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
                    let _ = logger::append_line(format!(
                        "rust-daemon: applied dock-mode policy action (attached={}, scale={})",
                        attached, scale
                    ));
                }
            }
        }
    }
}

fn summarize_actions(actions: &[PolicyAction]) -> String {
    if actions.is_empty() {
        return "none".to_string();
    }
    actions
        .iter()
        .map(|action| match action {
            PolicyAction::SetWifi { enabled, reason } => {
                format!("wifi:{}:{}", enabled, reason_label(*reason))
            }
            PolicyAction::SetBluetooth { enabled, reason } => {
                format!("bluetooth:{}:{}", enabled, reason_label(*reason))
            }
            PolicyAction::SetBacklight(level) => format!("backlight:{}", level),
            PolicyAction::SetDockMode { attached, scale } => {
                format!("dock:attached={}:scale={}", attached, scale)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn reason_label(reason: PolicyReason) -> &'static str {
    match reason {
        PolicyReason::Normal => "normal",
        PolicyReason::Corrective => "corrective",
    }
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
    use super::*;

    #[test]
    fn edge_expectation_matches_forced_edge_network_policy() {
        let attached = EdgeGuardExpectation::from_inputs(1, true);
        assert!(attached.expected_wifi_on);
        assert!(!attached.expected_bluetooth_on);

        let detached = EdgeGuardExpectation::from_inputs(2, false);
        assert!(detached.expected_wifi_on);
        assert!(detached.expected_bluetooth_on);
    }

    #[test]
    fn debounce_remaining_returns_zero_outside_window() {
        let base = Instant::now();
        let outside = base + Duration::from_millis(EDGE_DEBOUNCE_MS + 10);
        assert_eq!(
            debounce_remaining(outside, base, Duration::from_millis(EDGE_DEBOUNCE_MS),),
            Duration::ZERO
        );
    }

    #[test]
    fn register_edge_merges_when_guard_is_active() {
        let mut coordinator = EdgeGuardCoordinator::default();
        let base = Instant::now();
        let first = coordinator.register_edge(base, true);
        let first_expectation = match first {
            EdgeGuardRegisterOutcome::Start {
                expectation,
                start_delay,
            } => {
                assert_eq!(start_delay, Duration::ZERO);
                expectation
            }
            EdgeGuardRegisterOutcome::Merged { .. } => panic!("first edge should start guard"),
        };
        assert_eq!(first_expectation.edge_id, 1);

        let second = coordinator.register_edge(base + Duration::from_millis(100), false);
        let merged_edge_id = match second {
            EdgeGuardRegisterOutcome::Merged { edge_id } => edge_id,
            EdgeGuardRegisterOutcome::Start { .. } => panic!("second edge should be merged"),
        };
        assert_eq!(merged_edge_id, 2);
        assert!(coordinator.pending.is_some());
    }

    #[test]
    fn register_edge_applies_debounce_delay_when_recent_edge_exists() {
        let mut coordinator = EdgeGuardCoordinator::default();
        let base = Instant::now();
        let _ = coordinator.register_edge(base, true);
        coordinator.finish();

        let second = coordinator.register_edge(base + Duration::from_millis(120), true);
        match second {
            EdgeGuardRegisterOutcome::Start { start_delay, .. } => {
                assert!(start_delay > Duration::ZERO);
                assert!(start_delay <= Duration::from_millis(EDGE_DEBOUNCE_MS));
            }
            EdgeGuardRegisterOutcome::Merged { .. } => {
                panic!("inactive coordinator should start after debounce")
            }
        }
    }
}
