use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::PerformanceMetrics;

#[derive(Debug, Clone, Copy)]
struct CpuCounters {
    total: u64,
    idle: u64,
}

#[derive(Debug, Clone, Default)]
struct GpuCounters {
    engines: HashMap<String, (u64, u64)>,
    resident_bytes: u64,
}

#[derive(Default)]
pub struct PerformanceSampler {
    previous_cpu: Option<CpuCounters>,
    previous_gpu: Option<GpuCounters>,
}

impl PerformanceSampler {
    pub fn sample(&mut self) -> PerformanceMetrics {
        let cpu = read_cpu_counters();
        let gpu = read_gpu_counters();
        let (memory_total_bytes, memory_used_bytes) = read_memory_usage();
        let dedicated_vram_total = read_dedicated_vram_total();

        let cpu_usage_percent = self
            .previous_cpu
            .zip(cpu)
            .and_then(|(previous, current)| usage_between(previous, current));
        let gpu_usage_percent = self
            .previous_gpu
            .as_ref()
            .zip(gpu.as_ref())
            .and_then(|(previous, current)| gpu_usage_between(previous, current));

        self.previous_cpu = cpu;
        self.previous_gpu = gpu.clone();

        PerformanceMetrics {
            sampled_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            cpu_temperature_c: read_cpu_temperature(),
            cpu_usage_percent,
            gpu_temperature_c: read_gpu_temperature(),
            gpu_usage_percent,
            gpu_memory_used_bytes: gpu.map(|value| value.resident_bytes).unwrap_or(0),
            gpu_memory_total_bytes: dedicated_vram_total.unwrap_or(memory_total_bytes),
            gpu_memory_is_shared: dedicated_vram_total.is_none(),
            memory_used_bytes,
            memory_total_bytes,
        }
    }
}

fn read_cpu_counters() -> Option<CpuCounters> {
    let raw = fs::read_to_string("/proc/stat").ok()?;
    parse_cpu_counters(&raw)
}

fn parse_cpu_counters(raw: &str) -> Option<CpuCounters> {
    let fields = raw.lines().find(|line| line.starts_with("cpu "))?;
    let values = fields
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse::<u64>().ok())
        .collect::<Vec<_>>();
    if values.len() < 4 {
        return None;
    }

    Some(CpuCounters {
        total: values.iter().sum(),
        idle: values[3] + values.get(4).copied().unwrap_or(0),
    })
}

fn usage_between(previous: CpuCounters, current: CpuCounters) -> Option<f64> {
    let total = current.total.checked_sub(previous.total)?;
    let idle = current.idle.checked_sub(previous.idle)?;
    if total == 0 {
        return None;
    }
    Some((((total.saturating_sub(idle)) as f64 / total as f64) * 100.0).clamp(0.0, 100.0))
}

fn read_cpu_temperature() -> Option<f64> {
    find_hwmon_temperature(&["coretemp", "k10temp"], &["package id 0", "tctl"])
        .or_else(|| find_thermal_zone_temperature(&["x86_pkg_temp", "tcpu_pci", "tcpu"]))
}

fn read_gpu_temperature() -> Option<f64> {
    find_hwmon_temperature(
        &["amdgpu", "i915", "xe", "nouveau", "nvidia"],
        &["edge", "junction", "gpu"],
    )
    .or_else(|| find_thermal_zone_temperature(&["gpu", "xgpu", "dgpu"]))
}

fn find_hwmon_temperature(names: &[&str], preferred_labels: &[&str]) -> Option<f64> {
    let entries = fs::read_dir("/sys/class/hwmon").ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = read_trimmed(path.join("name")) else {
            continue;
        };
        let name = name.to_ascii_lowercase();
        if !names.iter().any(|candidate| name.contains(candidate)) {
            continue;
        }

        let mut fallback = None;
        for index in 1..=64 {
            let input = path.join(format!("temp{index}_input"));
            let Some(value) = read_millidegrees(&input) else {
                continue;
            };
            let label = read_trimmed(path.join(format!("temp{index}_label")))
                .unwrap_or_default()
                .to_ascii_lowercase();
            if preferred_labels
                .iter()
                .any(|candidate| label.contains(candidate))
            {
                return Some(value);
            }
            fallback.get_or_insert(value);
        }
        if fallback.is_some() {
            return fallback;
        }
    }
    None
}

fn find_thermal_zone_temperature(names: &[&str]) -> Option<f64> {
    let entries = fs::read_dir("/sys/class/thermal").ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("thermal_zone")
        {
            continue;
        }
        let Some(kind) = read_trimmed(path.join("type")) else {
            continue;
        };
        let kind = kind.to_ascii_lowercase();
        if names.iter().any(|candidate| kind.contains(candidate)) {
            if let Some(value) = read_millidegrees(&path.join("temp")) {
                return Some(value);
            }
        }
    }
    None
}

fn read_millidegrees(path: &Path) -> Option<f64> {
    let raw = read_trimmed(path)?.parse::<f64>().ok()?;
    let celsius = if raw.abs() >= 1000.0 {
        raw / 1000.0
    } else {
        raw
    };
    (celsius > 0.0 && celsius < 150.0).then_some(celsius)
}

fn read_memory_usage() -> (u64, u64) {
    let raw = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    parse_memory_usage(&raw)
}

fn parse_memory_usage(raw: &str) -> (u64, u64) {
    let mut total_kib = 0;
    let mut available_kib = 0;
    for line in raw.lines() {
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("MemTotal:") => {
                total_kib = fields.next().and_then(|v| v.parse().ok()).unwrap_or(0)
            }
            Some("MemAvailable:") => {
                available_kib = fields.next().and_then(|v| v.parse().ok()).unwrap_or(0)
            }
            _ => {}
        }
    }
    let total = total_kib * 1024;
    (total, total.saturating_sub(available_kib * 1024))
}

fn read_gpu_counters() -> Option<GpuCounters> {
    let mut clients = HashSet::new();
    let mut counters = GpuCounters::default();

    for pid in gpu_process_ids() {
        let fd_dir = PathBuf::from(format!("/proc/{pid}/fd"));
        let Ok(fd_entries) = fs::read_dir(&fd_dir) else {
            continue;
        };
        for entry in fd_entries.flatten() {
            let Ok(target) = fs::read_link(entry.path()) else {
                continue;
            };
            if !target.starts_with("/dev/dri") {
                continue;
            }
            let fd = entry.file_name();
            let Ok(raw) =
                fs::read_to_string(format!("/proc/{pid}/fdinfo/{}", fd.to_string_lossy()))
            else {
                continue;
            };
            let Some(parsed) = parse_drm_fdinfo(&raw) else {
                continue;
            };
            if !clients.insert(parsed.client_key) {
                continue;
            }
            counters.resident_bytes = counters
                .resident_bytes
                .saturating_add(parsed.resident_bytes);
            for (engine, cycles, total) in parsed.engines {
                let aggregate = counters.engines.entry(engine).or_default();
                aggregate.0 = aggregate.0.saturating_add(cycles);
                aggregate.1 = aggregate.1.max(total);
            }
        }
    }

    (!counters.engines.is_empty() || counters.resident_bytes > 0).then_some(counters)
}

fn gpu_process_ids() -> HashSet<u32> {
    let mut pids = HashSet::new();
    let Some(processes) = fs::read_dir("/proc").ok() else {
        return pids;
    };
    for process in processes.flatten() {
        let Some(pid) = process.file_name().to_string_lossy().parse::<u32>().ok() else {
            continue;
        };
        let Ok(fds) = fs::read_dir(process.path().join("fd")) else {
            continue;
        };
        if fds.flatten().any(|fd| {
            fs::read_link(fd.path())
                .ok()
                .is_some_and(|target| target.starts_with("/dev/dri"))
        }) {
            pids.insert(pid);
        }
    }
    pids
}

struct ParsedDrmFdinfo {
    client_key: String,
    engines: Vec<(String, u64, u64)>,
    resident_bytes: u64,
}

fn parse_drm_fdinfo(raw: &str) -> Option<ParsedDrmFdinfo> {
    let mut driver = None;
    let mut client_id = None;
    let mut pdev = None;
    let mut values = HashMap::new();
    let mut resident_bytes = 0u64;

    for line in raw.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let mut value_parts = value.split_whitespace();
        let value = value_parts.next().unwrap_or_default();
        let unit = value_parts.next().unwrap_or_default();
        match key {
            "drm-driver" => driver = Some(value.to_string()),
            "drm-client-id" => client_id = Some(value.to_string()),
            "drm-pdev" => pdev = Some(value.to_string()),
            key if key.starts_with("drm-resident-") => {
                let multiplier = if unit.eq_ignore_ascii_case("kib") {
                    1024
                } else {
                    1
                };
                resident_bytes = resident_bytes
                    .saturating_add(value.parse::<u64>().unwrap_or(0).saturating_mul(multiplier));
            }
            key if key.starts_with("drm-cycles-") || key.starts_with("drm-total-cycles-") => {
                values.insert(key.to_string(), value.parse::<u64>().unwrap_or(0));
            }
            _ => {}
        }
    }

    let driver = driver?;
    if !matches!(driver.as_str(), "xe" | "i915" | "amdgpu" | "nouveau") {
        return None;
    }
    let client_id = client_id?;
    let pdev = pdev.unwrap_or_else(|| "unknown".into());
    let mut engines = Vec::new();
    for (key, cycles) in &values {
        let Some(engine) = key.strip_prefix("drm-cycles-") else {
            continue;
        };
        let total = values
            .get(&format!("drm-total-cycles-{engine}"))
            .copied()
            .unwrap_or(0);
        if total > 0 {
            engines.push((engine.to_string(), *cycles, total));
        }
    }

    Some(ParsedDrmFdinfo {
        client_key: format!("{driver}:{pdev}:{client_id}"),
        engines,
        resident_bytes,
    })
}

fn gpu_usage_between(previous: &GpuCounters, current: &GpuCounters) -> Option<f64> {
    current
        .engines
        .iter()
        .filter_map(|(engine, (cycles, total))| {
            let (previous_cycles, previous_total) = previous.engines.get(engine)?;
            let cycles_delta = cycles.checked_sub(*previous_cycles)?;
            let total_delta = total.checked_sub(*previous_total)?;
            (total_delta > 0).then_some(cycles_delta as f64 / total_delta as f64 * 100.0)
        })
        .reduce(f64::max)
        .map(|value| value.clamp(0.0, 100.0))
}

fn read_dedicated_vram_total() -> Option<u64> {
    drm_card_paths().into_iter().find_map(|card| {
        read_trimmed(card.join("device/mem_info_vram_total"))?
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
    })
}

fn drm_card_paths() -> Vec<PathBuf> {
    fs::read_dir("/sys/class/drm")
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.strip_prefix("card").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
            })
        })
        .map(|entry| entry.path())
        .collect()
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_cpu_usage_from_counter_deltas() {
        let previous = parse_cpu_counters("cpu  100 0 50 850 0 0 0 0\n").unwrap();
        let current = parse_cpu_counters("cpu  140 0 60 900 0 0 0 0\n").unwrap();
        assert_eq!(usage_between(previous, current), Some(50.0));
    }

    #[test]
    fn parses_available_memory_as_used_memory() {
        let (total, used) =
            parse_memory_usage("MemTotal:       1000000 kB\nMemAvailable:    250000 kB\n");
        assert_eq!(total, 1_024_000_000);
        assert_eq!(used, 768_000_000);
    }

    #[test]
    fn parses_xe_cycles_and_shared_gpu_memory() {
        let parsed = parse_drm_fdinfo(
            "drm-driver:\txe\ndrm-client-id:\t7\ndrm-pdev:\t0000:00:02.0\n\
             drm-resident-system:\t4096 KiB\ndrm-resident-gtt:\t2048 KiB\n\
             drm-cycles-rcs:\t100\ndrm-total-cycles-rcs:\t1000\n",
        )
        .unwrap();
        assert_eq!(parsed.client_key, "xe:0000:00:02.0:7");
        assert_eq!(parsed.resident_bytes, 6144 * 1024);
        assert_eq!(parsed.engines, vec![("rcs".into(), 100, 1000)]);
    }

    #[test]
    fn samples_live_cpu_and_memory() {
        let mut sampler = PerformanceSampler::default();
        let first = sampler.sample();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = sampler.sample();

        assert!(first.memory_total_bytes > 0);
        assert!(second.memory_used_bytes <= second.memory_total_bytes);
        assert!(second.cpu_usage_percent.is_some());
    }
}
