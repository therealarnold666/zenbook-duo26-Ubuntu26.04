use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::models::BatteryStatus;

const POWER_SUPPLY_DIR: &str = "/sys/class/power_supply";
const ALLOWED_CHARGE_LIMITS: [u8; 3] = [80, 90, 100];

pub fn read_status(configured_charge_limit_percent: u8) -> BatteryStatus {
    let Some(path) = find_battery_path(Path::new(POWER_SUPPLY_DIR)) else {
        return BatteryStatus {
            configured_charge_limit_percent,
            ..BatteryStatus::default()
        };
    };

    read_status_from(&path, configured_charge_limit_percent)
}

pub fn set_charge_limit(limit: u8) -> Result<BatteryStatus, String> {
    validate_charge_limit(limit)?;
    let battery_path = find_battery_path(Path::new(POWER_SUPPLY_DIR))
        .ok_or_else(|| "No system battery was found".to_string())?;
    let threshold_path = battery_path.join("charge_control_end_threshold");

    if !threshold_path.exists() {
        return Err("The kernel does not expose an ASUS charge-limit interface".into());
    }

    fs::write(&threshold_path, format!("{limit}\n")).map_err(|error| {
        format!(
            "Failed to set the battery charge limit through {}: {}",
            threshold_path.display(),
            explain_threshold_error(&error)
        )
    })?;

    let status = read_status_from(&battery_path, limit);
    if status.active_charge_limit_percent != Some(limit) {
        return Err(format!(
            "The firmware accepted {limit}% but the kernel did not confirm the new limit"
        ));
    }

    Ok(status)
}

pub fn validate_charge_limit(limit: u8) -> Result<(), String> {
    if ALLOWED_CHARGE_LIMITS.contains(&limit) {
        Ok(())
    } else {
        Err(format!(
            "Unsupported charge limit {limit}%; choose 80%, 90%, or 100%"
        ))
    }
}

fn find_battery_path(power_supply_dir: &Path) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(power_supply_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| read_trimmed(path.join("type")).as_deref() == Some("Battery"))
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.into_iter().next()
}

fn read_status_from(path: &Path, configured_charge_limit_percent: u8) -> BatteryStatus {
    let state = read_trimmed(path.join("status")).unwrap_or_else(|| "Unknown".into());
    let energy_wh = read_micro_value(path.join("energy_now")).or_else(|| {
        let charge_uah = read_number(path.join("charge_now"))?;
        let voltage_uv = read_number(path.join("voltage_now"))?;
        Some((charge_uah as f64 * voltage_uv as f64) / 1_000_000_000_000.0)
    });
    let power_w = read_micro_value(path.join("power_now")).or_else(|| {
        let current_ua = read_number(path.join("current_now"))?;
        let voltage_uv = read_number(path.join("voltage_now"))?;
        Some((current_ua as f64 * voltage_uv as f64) / 1_000_000_000_000.0)
    });

    BatteryStatus {
        present: read_trimmed(path.join("present")).as_deref() != Some("0"),
        capacity_percent: read_number(path.join("capacity"))
            .and_then(|value| value.try_into().ok()),
        energy_wh,
        discharge_power_w: state
            .eq_ignore_ascii_case("discharging")
            .then_some(power_w)
            .flatten(),
        state,
        charge_limit_supported: path.join("charge_control_end_threshold").exists(),
        configured_charge_limit_percent,
        active_charge_limit_percent: read_number(path.join("charge_control_end_threshold"))
            .and_then(|value| value.try_into().ok()),
    }
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_number(path: impl AsRef<Path>) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

fn read_micro_value(path: impl AsRef<Path>) -> Option<f64> {
    read_number(path).map(|value| value as f64 / 1_000_000.0)
}

fn explain_threshold_error(error: &std::io::Error) -> String {
    match error.kind() {
        ErrorKind::PermissionDenied => {
            "permission denied; reinstall or restart the root system daemon".into()
        }
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let unique = format!(
            "zenbook-duo-battery-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn reads_energy_and_discharge_power_in_si_units() {
        let root = temp_dir();
        let battery = root.join("BAT0");
        fs::create_dir(&battery).expect("create battery");
        fs::write(battery.join("type"), "Battery\n").expect("write type");
        fs::write(battery.join("present"), "1\n").expect("write present");
        fs::write(battery.join("capacity"), "83\n").expect("write capacity");
        fs::write(battery.join("status"), "Discharging\n").expect("write status");
        fs::write(battery.join("energy_now"), "79236000\n").expect("write energy");
        fs::write(battery.join("power_now"), "10950000\n").expect("write power");
        fs::write(battery.join("charge_control_end_threshold"), "80\n").expect("write threshold");

        let path = find_battery_path(&root).expect("find battery");
        let status = read_status_from(&path, 80);

        assert_eq!(status.capacity_percent, Some(83));
        assert_eq!(status.energy_wh, Some(79.236));
        assert_eq!(status.discharge_power_w, Some(10.95));
        assert_eq!(status.active_charge_limit_percent, Some(80));
        fs::remove_dir_all(root).expect("remove temp dir");
    }

    #[test]
    fn charging_power_is_not_reported_as_discharge() {
        let root = temp_dir();
        fs::write(root.join("status"), "Charging\n").expect("write status");
        fs::write(root.join("power_now"), "20000000\n").expect("write power");

        let status = read_status_from(&root, 100);

        assert_eq!(status.discharge_power_w, None);
        fs::remove_dir_all(root).expect("remove temp dir");
    }

    #[test]
    fn accepts_only_product_charge_limit_choices() {
        assert!(validate_charge_limit(80).is_ok());
        assert!(validate_charge_limit(90).is_ok());
        assert!(validate_charge_limit(100).is_ok());
        assert!(validate_charge_limit(79).is_err());
    }
}
