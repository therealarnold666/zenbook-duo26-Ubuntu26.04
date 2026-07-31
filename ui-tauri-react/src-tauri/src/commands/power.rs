use crate::ipc::protocol::{DaemonRequest, DaemonResponse};
use crate::models::PerformanceMode;
use crate::runtime::client;

#[tauri::command]
pub fn apply_performance_mode(mode: PerformanceMode) -> Result<(), String> {
    match client::request(DaemonRequest::ApplyPerformanceMode { mode }) {
        Ok(DaemonResponse::Ack) => Ok(()),
        Ok(DaemonResponse::Error { message }) => Err(message),
        Ok(_) => Err("Unexpected daemon response while applying performance mode".into()),
        Err(message) => Err(format!(
            "The system daemon is required to set performance mode: {message}"
        )),
    }
}
