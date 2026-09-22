use std::fs;
use std::process::Command;

use crate::models::{PerformanceMode, PowerLimits};

const ASUS_PROFILE: &str =
    "/sys/devices/platform/asus-nb-wmi/platform-profile/platform-profile-0/profile";
const RAPL_MMIO: &str = "/sys/devices/virtual/powercap/intel-rapl-mmio/intel-rapl-mmio:0";

pub fn apply_performance_mode(mode: &PerformanceMode, limits: &PowerLimits) -> Result<(), String> {
    validate_limits(limits)?;
    let status = Command::new("powerprofilesctl")
        .args(["set", mode.powerprofiles_mode()])
        .status()
        .map_err(|e| format!("Failed to run powerprofilesctl: {e}"))?;
    if !status.success() {
        return Err(format!(
            "powerprofilesctl rejected {} mode",
            mode.powerprofiles_mode()
        ));
    }
    fs::write(ASUS_PROFILE, mode.as_asus_profile()).map_err(|e| {
        format!(
            "Failed to select ASUS {} profile: {e}",
            mode.as_asus_profile()
        )
    })?;

    for entry in fs::read_dir("/sys/devices/system/cpu/cpufreq")
        .map_err(|e| format!("Failed to enumerate CPU policies: {e}"))?
    {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("policy"))
        {
            let epp = path.join("energy_performance_preference");
            if epp.exists() {
                fs::write(&epp, mode.epp())
                    .map_err(|e| format!("Failed to set {}: {e}", epp.display()))?;
            }
        }
    }

    for (index, watts) in [
        (0, limits.pl1_watts),
        (1, limits.pl2_watts),
        (2, limits.pl3_watts),
    ] {
        let path = format!("{RAPL_MMIO}/constraint_{index}_power_limit_uw");
        fs::write(&path, (u64::from(watts) * 1_000_000).to_string())
            .map_err(|e| format!("Failed to set PL{} to {} W: {e}", index + 1, watts))?;
    }
    Ok(())
}

fn validate_limits(limits: &PowerLimits) -> Result<(), String> {
    if !(5..=100).contains(&limits.pl1_watts) || limits.pl2_watts > 150 || limits.pl3_watts > 200 {
        return Err("Power limits must be within PL1 5-100 W, PL2 5-150 W, PL3 5-200 W".into());
    }
    if limits.pl1_watts > limits.pl2_watts || limits.pl2_watts > limits.pl3_watts {
        return Err("Power limits must satisfy PL1 ≤ PL2 ≤ PL3".into());
    }
    Ok(())
}
