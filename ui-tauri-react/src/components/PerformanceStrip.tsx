import { useStore } from "@/lib/store";
import { IconCpu, IconDeviceDesktopAnalytics, IconDeviceSdCard } from "@tabler/icons-react";

function percent(value: number | null | undefined) {
  return value == null ? "--" : `${Math.round(value)}%`;
}

function temperature(value: number | null | undefined) {
  return value == null ? "--" : `${Math.round(value)}°`;
}

function memory(used: number | undefined, total: number | undefined) {
  if (!used || !total) return "--";
  return `${(used / 1024 ** 3).toFixed(1)}/${(total / 1024 ** 3).toFixed(0)}G`;
}

export default function PerformanceStrip() {
  const metrics = useStore().performance;

  return (
    <div className="flex h-10 shrink-0 items-center justify-end gap-4 border-b border-border/60 px-8 font-mono text-[11px] tabular-nums">
      <Metric icon={IconCpu} label="CPU" value={`${percent(metrics?.cpuUsagePercent)} ${temperature(metrics?.cpuTemperatureC)}`} tone="text-teal-500" />
      <Metric icon={IconDeviceDesktopAnalytics} label="GPU" value={`${percent(metrics?.gpuUsagePercent)} ${temperature(metrics?.gpuTemperatureC)}`} tone="text-blue-500" />
      <Metric icon={IconDeviceSdCard} label="RAM" value={memory(metrics?.memoryUsedBytes, metrics?.memoryTotalBytes)} tone="text-amber-500" />
    </div>
  );
}

function Metric({
  icon: Icon,
  label,
  value,
  tone,
}: {
  icon: React.ComponentType<{ className?: string; stroke?: number }>;
  label: string;
  value: string;
  tone: string;
}) {
  return (
    <div className="flex min-w-[112px] items-center gap-1.5 whitespace-nowrap">
      <Icon className={`size-3.5 ${tone}`} stroke={1.75} />
      <span className="text-muted-foreground">{label}</span>
      <span className="font-medium text-foreground">{value}</span>
    </div>
  );
}
