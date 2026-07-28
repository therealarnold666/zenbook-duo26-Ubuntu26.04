use crate::commands::settings;
use crate::hardware::battery;
use crate::ipc::protocol::{DaemonRequest, DaemonResponse};
use crate::models::BatteryStatus;
use crate::runtime::client;

#[tauri::command]
pub fn get_battery_status() -> Result<BatteryStatus, String> {
    match client::request(DaemonRequest::GetBatteryStatus) {
        Ok(DaemonResponse::BatteryStatus { status }) => Ok(status),
        Ok(DaemonResponse::Error { message }) => Err(message),
        Ok(_) => Err("Unexpected daemon response while reading battery status".into()),
        Err(_) => {
            let configured = settings::load_settings_local().charge_limit_percent;
            Ok(battery::read_status(configured))
        }
    }
}

#[tauri::command]
pub fn set_charge_limit(limit: u8) -> Result<BatteryStatus, String> {
    match client::request(DaemonRequest::SetChargeLimit { limit }) {
        Ok(DaemonResponse::BatteryStatus { status }) => Ok(status),
        Ok(DaemonResponse::Error { message }) => Err(message),
        Ok(_) => Err("Unexpected daemon response while setting charge limit".into()),
        Err(message) => Err(format!(
            "The system daemon is required to set the charge limit: {message}"
        )),
    }
}
