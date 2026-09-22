use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceMetrics {
    pub sampled_at: i64,
    pub cpu_temperature_c: Option<f64>,
    pub cpu_usage_percent: Option<f64>,
    pub gpu_temperature_c: Option<f64>,
    pub gpu_usage_percent: Option<f64>,
    pub gpu_memory_used_bytes: u64,
    pub gpu_memory_total_bytes: u64,
    pub gpu_memory_is_shared: bool,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum TrayPerformanceMetric {
    CpuUsage,
    CpuTemperature,
    GpuUsage,
    GpuTemperature,
    MemoryUsage,
    GpuMemoryUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrayPerformanceItem {
    pub metric: TrayPerformanceMetric,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrayPerformanceSettings {
    pub enabled: bool,
    pub items: Vec<TrayPerformanceItem>,
}

impl Default for TrayPerformanceSettings {
    fn default() -> Self {
        use TrayPerformanceMetric::*;

        Self {
            enabled: true,
            items: [
                CpuUsage,
                CpuTemperature,
                GpuUsage,
                GpuTemperature,
                MemoryUsage,
                GpuMemoryUsage,
            ]
            .into_iter()
            .map(|metric| TrayPerformanceItem {
                metric,
                enabled: true,
            })
            .collect(),
        }
    }
}
