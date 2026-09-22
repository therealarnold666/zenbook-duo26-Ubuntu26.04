import { useStore } from "@/lib/store";
import type { PerformanceMetrics } from "@/types/duo";
import { cn } from "@/lib/utils";
import {
  IconCpu,
  IconDeviceDesktopAnalytics,
  IconDeviceSdCard,
  IconTemperature,
} from "@tabler/icons-react";
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

type HistoryPoint = PerformanceMetrics & { time: string };

export default function Performance() {
  const store = useStore();
  const current = store.performance;
  const history: HistoryPoint[] = store.performanceHistory.map((sample) => ({
    ...sample,
    time: new Date(sample.sampledAt).toLocaleTimeString([], {
      minute: "2-digit",
      second: "2-digit",
    }),
  }));

  const memoryPercent = ratio(current?.memoryUsedBytes, current?.memoryTotalBytes);
  const gpuMemoryPercent = ratio(
    current?.gpuMemoryUsedBytes,
    current?.gpuMemoryTotalBytes
  );
  const gpuMemoryTitle = current?.gpuMemoryIsShared ? "GPU shared memory" : "VRAM";

  return (
    <div>
      <div className="mb-6">
        <h1 className="text-xl font-semibold tracking-tight">Performance</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Live processor, graphics and memory telemetry
        </p>
      </div>

      <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        <MetricCard
          label="CPU"
          icon={IconCpu}
          value={formatPercent(current?.cpuUsagePercent)}
          detail={formatTemperature(current?.cpuTemperatureC)}
          accent="teal"
          progress={current?.cpuUsagePercent ?? 0}
        />
        <MetricCard
          label="GPU"
          icon={IconDeviceDesktopAnalytics}
          value={formatPercent(current?.gpuUsagePercent)}
          detail={formatTemperature(current?.gpuTemperatureC)}
          accent="blue"
          progress={current?.gpuUsagePercent ?? 0}
        />
        <MetricCard
          label={gpuMemoryTitle}
          icon={IconDeviceSdCard}
          value={formatBytes(current?.gpuMemoryUsedBytes)}
          detail={`${formatBytes(current?.gpuMemoryTotalBytes)} total`}
          accent="violet"
          progress={gpuMemoryPercent}
        />
        <MetricCard
          label="System memory"
          icon={IconDeviceSdCard}
          value={formatBytes(current?.memoryUsedBytes)}
          detail={`${formatBytes(current?.memoryTotalBytes)} total`}
          accent="amber"
          progress={memoryPercent}
        />
      </div>

      <section className="mt-5 border-t border-border/60 pt-5">
        <div className="mb-3 flex items-center justify-between">
          <div>
            <h2 className="text-[13px] font-semibold">Utilization history</h2>
            <p className="mt-0.5 text-[12px] text-muted-foreground">Last 60 seconds</p>
          </div>
          <div className="flex items-center gap-4 text-[11px] text-muted-foreground">
            <Legend color="bg-teal-500" label="CPU" />
            <Legend color="bg-blue-500" label="GPU" />
            <Legend color="bg-amber-500" label="RAM" />
          </div>
        </div>
        <div className="h-[250px] w-full">
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart data={history} margin={{ top: 12, right: 4, left: -22, bottom: 0 }}>
              <defs>
                <linearGradient id="cpuFill" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="oklch(0.65 0.14 185)" stopOpacity={0.28} />
                  <stop offset="100%" stopColor="oklch(0.65 0.14 185)" stopOpacity={0} />
                </linearGradient>
                <linearGradient id="gpuFill" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="oklch(0.65 0.12 220)" stopOpacity={0.2} />
                  <stop offset="100%" stopColor="oklch(0.65 0.12 220)" stopOpacity={0} />
                </linearGradient>
              </defs>
              <CartesianGrid vertical={false} stroke="var(--border)" strokeOpacity={0.55} />
              <XAxis dataKey="time" minTickGap={42} tick={{ fontSize: 10 }} stroke="var(--muted-foreground)" tickLine={false} axisLine={false} />
              <YAxis domain={[0, 100]} ticks={[0, 25, 50, 75, 100]} tick={{ fontSize: 10 }} stroke="var(--muted-foreground)" tickLine={false} axisLine={false} />
              <Tooltip content={<PerformanceTooltip />} />
              <Area type="monotone" dataKey="cpuUsagePercent" name="CPU" stroke="oklch(0.65 0.14 185)" fill="url(#cpuFill)" strokeWidth={2} connectNulls isAnimationActive={false} />
              <Area type="monotone" dataKey="gpuUsagePercent" name="GPU" stroke="oklch(0.65 0.12 220)" fill="url(#gpuFill)" strokeWidth={2} connectNulls isAnimationActive={false} />
              <Area type="monotone" dataKey={(point: HistoryPoint) => ratio(point.memoryUsedBytes, point.memoryTotalBytes)} name="RAM" stroke="oklch(0.70 0.14 60)" fill="transparent" strokeWidth={1.5} isAnimationActive={false} />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      </section>

      {current?.gpuTemperatureC == null && (
        <div className="mt-4 flex items-center gap-2 border-t border-border/60 pt-4 text-[12px] text-muted-foreground">
          <IconTemperature className="size-4 text-blue-500" stroke={1.5} />
          The current GPU driver does not expose a dedicated temperature sensor.
        </div>
      )}
    </div>
  );
}

const accents = {
  teal: { icon: "bg-teal-500/10 text-teal-500", bar: "bg-teal-500" },
  blue: { icon: "bg-blue-500/10 text-blue-500", bar: "bg-blue-500" },
  violet: { icon: "bg-violet-500/10 text-violet-500", bar: "bg-violet-500" },
  amber: { icon: "bg-amber-500/10 text-amber-500", bar: "bg-amber-500" },
};

function MetricCard({ label, icon: Icon, value, detail, accent, progress }: {
  label: string;
  icon: React.ComponentType<{ className?: string; stroke?: number }>;
  value: string;
  detail: string;
  accent: keyof typeof accents;
  progress: number;
}) {
  const tone = accents[accent];
  return (
    <div className="glass-card rounded-lg p-4">
      <div className="flex items-center gap-2">
        <div className={cn("flex size-7 items-center justify-center rounded-md", tone.icon)}>
          <Icon className="size-4" stroke={1.75} />
        </div>
        <span className="text-[11px] font-semibold uppercase text-muted-foreground">{label}</span>
      </div>
      <div className="mt-4 font-mono text-xl font-medium tabular-nums">{value}</div>
      <div className="mt-1 h-4 text-[11px] text-muted-foreground">{detail}</div>
      <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-muted">
        <div className={cn("h-full rounded-full transition-[width] duration-500", tone.bar)} style={{ width: `${Math.max(0, Math.min(100, progress))}%` }} />
      </div>
    </div>
  );
}

function PerformanceTooltip({ active, payload, label }: { active?: boolean; payload?: Array<{ name?: string; value?: number | null; color?: string }>; label?: string }) {
  if (!active || !payload?.length) return null;
  return (
    <div className="rounded-md border border-border bg-popover px-3 py-2 shadow-lg">
      <div className="mb-1 font-mono text-[10px] text-muted-foreground">{label}</div>
      {payload.map((item) => (
        <div key={item.name} className="flex min-w-24 justify-between gap-4 font-mono text-[11px]">
          <span style={{ color: item.color }}>{item.name}</span>
          <span>{item.value == null ? "--" : `${Math.round(item.value)}%`}</span>
        </div>
      ))}
    </div>
  );
}

function Legend({ color, label }: { color: string; label: string }) {
  return <span className="flex items-center gap-1.5"><span className={cn("size-2 rounded-full", color)} />{label}</span>;
}

function ratio(used?: number, total?: number) {
  return used != null && total ? (used / total) * 100 : 0;
}

function formatPercent(value?: number | null) {
  return value == null ? "--" : `${Math.round(value)}%`;
}

function formatTemperature(value?: number | null) {
  return value == null ? "Temperature unavailable" : `${Math.round(value)} °C`;
}

function formatBytes(value?: number) {
  if (!value) return "--";
  return `${(value / 1024 ** 3).toFixed(1)} GB`;
}
