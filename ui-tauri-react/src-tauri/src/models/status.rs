use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DuoStatus {
    pub keyboard_attached: bool,
    pub connection_type: ConnectionType,
    pub monitor_count: u32,
    pub wifi_enabled: bool,
    pub bluetooth_enabled: bool,
    pub backlight_level: u8,
    pub display_brightness: u32,
    pub max_brightness: u32,
    pub service_active: bool,
    pub orientation: Orientation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatteryStatus {
    pub present: bool,
    pub capacity_percent: Option<u8>,
    pub energy_wh: Option<f64>,
    pub discharge_power_w: Option<f64>,
    pub state: String,
    pub charge_limit_supported: bool,
    pub configured_charge_limit_percent: u8,
    pub active_charge_limit_percent: Option<u8>,
}

impl Default for BatteryStatus {
    fn default() -> Self {
        Self {
            present: false,
            capacity_percent: None,
            energy_wh: None,
            discharge_power_w: None,
            state: "Unknown".into(),
            charge_limit_supported: false,
            configured_charge_limit_percent: 100,
            active_charge_limit_percent: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionType {
    Usb,
    Bluetooth,
    #[default]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    #[default]
    Normal,
    Left,
    Right,
    Inverted,
}

impl Orientation {
    pub fn as_duo_arg(&self) -> &str {
        match self {
            Orientation::Normal => "normal",
            Orientation::Left => "left-up",
            Orientation::Right => "right-up",
            Orientation::Inverted => "bottom-up",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RefreshPolicy {
    #[default]
    Fixed,
    Dynamic,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DisplayMode {
    pub mode_id: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub connector: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: f64,
    pub scale: f64,
    pub x: i32,
    pub y: i32,
    pub transform: u32,
    pub primary: bool,
    #[serde(default)]
    pub enabled: bool,
    pub current_mode: DisplayMode,
    pub available_modes: Vec<DisplayMode>,
    pub refresh_policy: RefreshPolicy,
    pub supports_dynamic_refresh: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayLayout {
    pub displays: Vec<DisplayInfo>,
}
