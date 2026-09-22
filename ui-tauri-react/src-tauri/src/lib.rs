pub mod commands;
pub mod hardware;
pub mod ipc;
pub mod models;
pub mod runtime;
pub mod usb_media_remap_helper;
mod watchers;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    Manager,
};

use commands::events::create_event_buffer;
use hardware::performance::PerformanceSampler;
use models::{PerformanceMetrics, TrayPerformanceMetric, TrayPerformanceSettings};
use std::sync::Mutex;
use std::time::Duration;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let event_buffer = create_event_buffer();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            present_main_window(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .manage(event_buffer.clone())
        .manage(Mutex::new(PerformanceSampler::default()))
        .setup(move |app| {
            let handle = app.handle().clone();

            // Build tray menu
            build_tray(&handle)?;

            // Start background watchers
            watchers::start_all_watchers(&handle, event_buffer.clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            // Minimize to tray on close instead of quitting
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::status::get_status,
            commands::battery::get_battery_status,
            commands::battery::set_charge_limit,
            commands::power::apply_performance_mode,
            commands::performance::get_performance_metrics,
            commands::performance::load_tray_performance_settings,
            commands::performance::save_tray_performance_settings,
            commands::backlight::get_backlight,
            commands::backlight::set_backlight,
            commands::display::get_display_layout,
            commands::display::apply_display_layout,
            commands::display::set_orientation,
            commands::service::is_service_active,
            commands::service::restart_service,
            commands::settings::load_settings,
            commands::settings::save_settings,
            commands::logs::read_log,
            commands::logs::clear_log,
            commands::profiles::list_profiles,
            commands::profiles::save_profile,
            commands::profiles::delete_profile,
            commands::profiles::activate_profile,
            commands::events::get_recent_events,
            commands::diagnostics::diag_list_evdev,
            commands::diagnostics::diag_capture_evdev,
            commands::diagnostics::diag_capture_evdev_multi,
            commands::diagnostics::diag_list_hid,
            commands::diagnostics::diag_read_report_descriptor,
            commands::diagnostics::diag_capture_hidraw_pkexec,
            commands::usb_media_remap::usb_media_remap_status,
            commands::usb_media_remap::usb_media_remap_start,
            commands::usb_media_remap::usb_media_remap_stop,
            commands::usb_media_remap::usb_media_remap_toggle_pause,
            commands::touchscreen::list_touchscreens,
            commands::touchscreen::set_touchscreen_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
    let separator1 = PredefinedMenuItem::separator(app)?;
    let performance_cpu =
        MenuItem::with_id(app, "performance_cpu", "CPU  --", false, None::<&str>)?;
    let performance_gpu =
        MenuItem::with_id(app, "performance_gpu", "GPU  --", false, None::<&str>)?;
    let performance_memory =
        MenuItem::with_id(app, "performance_memory", "Memory  --", false, None::<&str>)?;
    let performance_vram = MenuItem::with_id(
        app,
        "performance_vram",
        "Graphics memory  --",
        false,
        None::<&str>,
    )?;
    let separator_performance = PredefinedMenuItem::separator(app)?;

    let profile_docked = MenuItem::with_id(app, "profile_docked", "Docked", true, None::<&str>)?;
    let profile_tablet = MenuItem::with_id(app, "profile_tablet", "Tablet", true, None::<&str>)?;
    let profile_presentation = MenuItem::with_id(
        app,
        "profile_presentation",
        "Presentation",
        true,
        None::<&str>,
    )?;
    let profiles_submenu = Submenu::with_items(
        app,
        "Profiles",
        true,
        &[&profile_docked, &profile_tablet, &profile_presentation],
    )?;

    let bl_0 = MenuItem::with_id(app, "bl_0", "Backlight Off", true, None::<&str>)?;
    let bl_1 = MenuItem::with_id(app, "bl_1", "Backlight Low", true, None::<&str>)?;
    let bl_2 = MenuItem::with_id(app, "bl_2", "Backlight Medium", true, None::<&str>)?;
    let bl_3 = MenuItem::with_id(app, "bl_3", "Backlight High", true, None::<&str>)?;
    let backlight_submenu =
        Submenu::with_items(app, "Backlight", true, &[&bl_0, &bl_1, &bl_2, &bl_3])?;

    let separator2 = PredefinedMenuItem::separator(app)?;
    let usb_media_remap = MenuItem::with_id(
        app,
        "usb_media_remap",
        "Toggle USB Media Remap",
        true,
        None::<&str>,
    )?;
    let usb_media_remap_pause = MenuItem::with_id(
        app,
        "usb_media_remap_pause",
        "Pause/Resume Remap",
        true,
        None::<&str>,
    )?;
    let separator3 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &show,
            &separator1,
            &performance_cpu,
            &performance_gpu,
            &performance_memory,
            &performance_vram,
            &separator_performance,
            &profiles_submenu,
            &backlight_submenu,
            &separator2,
            &usb_media_remap,
            &usb_media_remap_pause,
            &separator3,
            &quit,
        ],
    )?;

    let tray_builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .title("C --%/--C G --%/--C R --% V --%")
        .tooltip("Zenbook Duo Control")
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                "show" => {
                    present_main_window(app);
                }
                "quit" => {
                    app.exit(0);
                }
                "profile_docked" | "profile_tablet" | "profile_presentation" => {
                    let profile_id = id.strip_prefix("profile_").unwrap_or(id);
                    let _ = commands::profiles::activate_profile(profile_id.to_string());
                }
                id if id.starts_with("bl_") => {
                    if let Ok(level) = id[3..].parse::<u8>() {
                        let _ = commands::backlight::set_backlight_daemon_first(level);
                    }
                }
                "usb_media_remap" => {
                    let status = commands::usb_media_remap::daemon_first_status();
                    if status.running {
                        let _ = commands::usb_media_remap::daemon_first_stop();
                    } else {
                        let _ = commands::usb_media_remap::daemon_first_start();
                    }
                }
                "usb_media_remap_pause" => {
                    let _ = commands::usb_media_remap::daemon_first_toggle_pause();
                }
                _ => {}
            }
        });

    let tray_builder = if let Some(icon) = app.default_window_icon() {
        tray_builder.icon(icon.clone())
    } else {
        tray_builder
    };

    let tray = tray_builder
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                present_main_window(app);
            }
        })
        .build(app)?;

    start_tray_performance_monitor(
        tray,
        performance_cpu,
        performance_gpu,
        performance_memory,
        performance_vram,
    );

    Ok(())
}

fn start_tray_performance_monitor(
    tray: TrayIcon,
    cpu_item: MenuItem<tauri::Wry>,
    gpu_item: MenuItem<tauri::Wry>,
    memory_item: MenuItem<tauri::Wry>,
    vram_item: MenuItem<tauri::Wry>,
) {
    std::thread::spawn(move || {
        let mut sampler = PerformanceSampler::default();

        loop {
            let metrics = sampler.sample();
            let settings = commands::performance::load_tray_performance_settings_local();
            let title = format_tray_title(&metrics, &settings);
            let _ = tray.set_title(title.as_deref());
            let _ = cpu_item.set_text(format_cpu_menu_text(&metrics));
            let _ = gpu_item.set_text(format_gpu_menu_text(&metrics));
            let _ = memory_item.set_text(format_memory_menu_text(&metrics));
            let _ = vram_item.set_text(format_vram_menu_text(&metrics));
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

fn format_tray_title(
    metrics: &PerformanceMetrics,
    settings: &TrayPerformanceSettings,
) -> Option<String> {
    if !settings.enabled {
        return None;
    }

    let parts: Vec<String> = settings
        .items
        .iter()
        .filter(|item| item.enabled)
        .map(|item| match item.metric {
            TrayPerformanceMetric::CpuUsage => {
                format!("CPU {}", percent_text(metrics.cpu_usage_percent))
            }
            TrayPerformanceMetric::CpuTemperature => {
                format!("CPU {}", temperature_text(metrics.cpu_temperature_c))
            }
            TrayPerformanceMetric::GpuUsage => {
                format!("GPU {}", percent_text(metrics.gpu_usage_percent))
            }
            TrayPerformanceMetric::GpuTemperature => {
                format!("GPU {}", temperature_text(metrics.gpu_temperature_c))
            }
            TrayPerformanceMetric::MemoryUsage => format!(
                "RAM {}",
                percent_text(ratio_percent(
                    metrics.memory_used_bytes,
                    metrics.memory_total_bytes
                ))
            ),
            TrayPerformanceMetric::GpuMemoryUsage => format!(
                "VRAM {}",
                percent_text(ratio_percent(
                    metrics.gpu_memory_used_bytes,
                    metrics.gpu_memory_total_bytes
                ))
            ),
        })
        .collect();

    (!parts.is_empty()).then(|| parts.join(" | "))
}

fn format_cpu_menu_text(metrics: &PerformanceMetrics) -> String {
    format!(
        "CPU  usage {}  temperature {}",
        percent_text(metrics.cpu_usage_percent),
        temperature_text(metrics.cpu_temperature_c)
    )
}

fn format_gpu_menu_text(metrics: &PerformanceMetrics) -> String {
    format!(
        "GPU  usage {}  temperature {}",
        percent_text(metrics.gpu_usage_percent),
        temperature_text(metrics.gpu_temperature_c)
    )
}

fn format_memory_menu_text(metrics: &PerformanceMetrics) -> String {
    format!(
        "Memory  {} / {}  ({})",
        gibibytes_text(metrics.memory_used_bytes),
        gibibytes_text(metrics.memory_total_bytes),
        percent_text(ratio_percent(
            metrics.memory_used_bytes,
            metrics.memory_total_bytes
        ))
    )
}

fn format_vram_menu_text(metrics: &PerformanceMetrics) -> String {
    let kind = if metrics.gpu_memory_is_shared {
        "Shared graphics memory"
    } else {
        "VRAM"
    };
    format!(
        "{}  {} / {}  ({})",
        kind,
        gibibytes_text(metrics.gpu_memory_used_bytes),
        gibibytes_text(metrics.gpu_memory_total_bytes),
        percent_text(ratio_percent(
            metrics.gpu_memory_used_bytes,
            metrics.gpu_memory_total_bytes
        ))
    )
}

fn ratio_percent(used: u64, total: u64) -> Option<f64> {
    (total > 0).then(|| used as f64 * 100.0 / total as f64)
}

fn percent_text(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.0}%"))
        .unwrap_or_else(|| "--%".to_string())
}

fn temperature_text(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.0}C"))
        .unwrap_or_else(|| "--C".to_string())
}

fn gibibytes_text(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / 1024_f64.powi(3))
}

#[cfg(test)]
mod tray_performance_tests {
    use super::*;

    fn metrics() -> PerformanceMetrics {
        PerformanceMetrics {
            sampled_at: 0,
            cpu_temperature_c: Some(67.4),
            cpu_usage_percent: Some(21.2),
            gpu_temperature_c: None,
            gpu_usage_percent: Some(34.8),
            gpu_memory_used_bytes: 2 * 1024_u64.pow(3),
            gpu_memory_total_bytes: 8 * 1024_u64.pow(3),
            gpu_memory_is_shared: true,
            memory_used_bytes: 12 * 1024_u64.pow(3),
            memory_total_bytes: 32 * 1024_u64.pow(3),
        }
    }

    #[test]
    fn tray_title_contains_all_requested_metrics() {
        assert_eq!(
            format_tray_title(&metrics(), &TrayPerformanceSettings::default()),
            Some("CPU 21% | CPU 67C | GPU 35% | GPU --C | RAM 38% | VRAM 25%".to_string())
        );
    }

    #[test]
    fn tray_title_respects_switches_and_order() {
        let mut settings = TrayPerformanceSettings::default();
        settings.items.reverse();
        settings.items.retain(|item| {
            matches!(
                item.metric,
                TrayPerformanceMetric::MemoryUsage | TrayPerformanceMetric::CpuUsage
            )
        });

        assert_eq!(
            format_tray_title(&metrics(), &settings),
            Some("RAM 38% | CPU 21%".to_string())
        );

        settings.enabled = false;
        assert_eq!(format_tray_title(&metrics(), &settings), None);
    }

    #[test]
    fn shared_graphics_memory_is_labelled_honestly() {
        assert_eq!(
            format_vram_menu_text(&metrics()),
            "Shared graphics memory  2.0 GiB / 8.0 GiB  (25%)"
        );
    }
}

/// Presents the single Control window above the current application. Wayland
/// can otherwise ignore a focus request that originates from a background
/// keyboard-helper process, so briefly raising the window makes F12 reliable
/// without leaving the application permanently on top.
fn present_main_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = window.set_always_on_top(false);
    });
}
