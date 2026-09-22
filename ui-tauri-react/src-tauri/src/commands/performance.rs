use std::{env, fs, path::PathBuf, sync::Mutex};

use tauri::State;

use crate::hardware::performance::PerformanceSampler;
use crate::models::{PerformanceMetrics, TrayPerformanceSettings};

#[tauri::command]
pub fn get_performance_metrics(
    sampler: State<'_, Mutex<PerformanceSampler>>,
) -> Result<PerformanceMetrics, String> {
    sampler
        .lock()
        .map_err(|_| "Performance sampler lock was poisoned".to_string())
        .map(|mut sampler| sampler.sample())
}

fn tray_performance_settings_path() -> PathBuf {
    let config_dir = if let Ok(home_override) = env::var("ZENBOOK_DUO_HOME") {
        PathBuf::from(home_override).join(".config")
    } else {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"))
    }
    .join("zenbook-duo");
    let _ = fs::create_dir_all(&config_dir);
    config_dir.join("tray-performance.json")
}

pub fn load_tray_performance_settings_local() -> TrayPerformanceSettings {
    fs::read_to_string(tray_performance_settings_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn load_tray_performance_settings() -> TrayPerformanceSettings {
    load_tray_performance_settings_local()
}

#[tauri::command]
pub fn save_tray_performance_settings(settings: TrayPerformanceSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|error| format!("Serialize error: {error}"))?;
    fs::write(tray_performance_settings_path(), json)
        .map_err(|error| format!("Write error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tray_settings_include_all_metrics() {
        let settings = TrayPerformanceSettings::default();

        assert!(settings.enabled);
        assert_eq!(settings.items.len(), 6);
        assert!(settings.items.iter().all(|item| item.enabled));
    }
}
