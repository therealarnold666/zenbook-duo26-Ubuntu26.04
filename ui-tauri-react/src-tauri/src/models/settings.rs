use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuoSettings {
    #[serde(default = "default_backlight")]
    pub default_backlight: u8,
    #[serde(default = "default_scale")]
    pub default_scale: f64,
    #[serde(default)]
    pub auto_dual_screen: bool,
    #[serde(default)]
    pub auto_rotate: bool,
    #[serde(default)]
    pub keyboard_backlight_power_save: bool,
    #[serde(default)]
    pub auto_quiet_on_battery: bool,
    #[serde(default)]
    pub active_performance_mode: PerformanceMode,
    #[serde(default)]
    pub performance_profiles: PerformanceProfiles,
    #[serde(default)]
    pub sync_brightness: bool,
    /// Last user-selected display brightness, kept as a percentage so it is
    /// portable across the two panels' different raw backlight ranges.
    #[serde(default)]
    pub last_display_brightness_percent: Option<u8>,
    #[serde(default)]
    pub theme: ThemePreference,
    #[serde(default = "default_usb_media_remap_enabled")]
    pub usb_media_remap_enabled: bool,
    #[serde(default)]
    pub setup_completed: bool,
    #[serde(default)]
    pub touchscreen_disabled: Vec<String>,
    #[serde(default = "default_charge_limit_percent")]
    pub charge_limit_percent: u8,
}

impl Default for DuoSettings {
    fn default() -> Self {
        Self {
            default_backlight: default_backlight(),
            default_scale: default_scale(),
            auto_dual_screen: true,
            auto_rotate: false,
            keyboard_backlight_power_save: false,
            auto_quiet_on_battery: false,
            active_performance_mode: PerformanceMode::Balanced,
            performance_profiles: PerformanceProfiles::default(),
            sync_brightness: true,
            last_display_brightness_percent: None,
            theme: ThemePreference::System,
            usb_media_remap_enabled: default_usb_media_remap_enabled(),
            setup_completed: false,
            touchscreen_disabled: Vec::new(),
            charge_limit_percent: default_charge_limit_percent(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PerformanceMode {
    Quiet,
    #[default]
    Balanced,
    Performance,
}

impl PerformanceMode {
    pub fn as_asus_profile(&self) -> &'static str {
        match self {
            Self::Quiet => "quiet",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
        }
    }

    pub fn epp(&self) -> &'static str {
        match self {
            Self::Quiet => "power",
            Self::Balanced => "balance_performance",
            Self::Performance => "performance",
        }
    }

    pub fn powerprofiles_mode(&self) -> &'static str {
        match self {
            Self::Quiet => "power-saver",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PowerLimits {
    pub pl1_watts: u16,
    pub pl2_watts: u16,
    pub pl3_watts: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceProfiles {
    pub quiet: PowerLimits,
    pub balanced: PowerLimits,
    pub performance: PowerLimits,
}

impl Default for PerformanceProfiles {
    fn default() -> Self {
        Self {
            quiet: PowerLimits {
                pl1_watts: 12,
                pl2_watts: 15,
                pl3_watts: 20,
            },
            balanced: PowerLimits {
                pl1_watts: 35,
                pl2_watts: 45,
                pl3_watts: 50,
            },
            performance: PowerLimits {
                pl1_watts: 45,
                pl2_watts: 55,
                pl3_watts: 65,
            },
        }
    }
}

impl PerformanceProfiles {
    pub fn for_mode(&self, mode: &PerformanceMode) -> &PowerLimits {
        match mode {
            PerformanceMode::Quiet => &self.quiet,
            PerformanceMode::Balanced => &self.balanced,
            PerformanceMode::Performance => &self.performance,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

fn default_backlight() -> u8 {
    3
}

fn default_scale() -> f64 {
    1.67
}

fn default_usb_media_remap_enabled() -> bool {
    true
}

fn default_charge_limit_percent() -> u8 {
    100
}

#[cfg(test)]
mod tests {
    use super::DuoSettings;

    #[test]
    fn older_settings_default_to_full_charge() {
        let settings: DuoSettings =
            serde_json::from_str(r#"{"defaultBacklight":2}"#).expect("deserialize settings");

        assert_eq!(settings.charge_limit_percent, 100);
        assert!(!settings.auto_rotate);
        assert!(!settings.keyboard_backlight_power_save);
        assert!(!settings.auto_quiet_on_battery);
    }
}
