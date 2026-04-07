import type { HardwareEvent } from "@/types/duo";
import { cn } from "@/lib/utils";
import { IconBolt } from "@tabler/icons-react";

interface EventStreamProps {
  events: HardwareEvent[];
}

const severityColor: Record<string, string> = {
  info: "text-foreground/80",
  warning: "text-amber-500",
  error: "text-red-500",
};

const severityDot: Record<string, string> = {
  info: "bg-blue-400",
  warning: "bg-amber-400",
  error: "bg-red-400",
};

const categoryColor: Record<string, string> = {
  USB: "bg-violet-500/15 text-violet-500 border-violet-500/20",
  DISPLAY: "bg-blue-500/15 text-blue-500 border-blue-500/20",
  KEYBOARD: "bg-emerald-500/15 text-emerald-500 border-emerald-500/20",
  NETWORK: "bg-cyan-500/15 text-cyan-500 border-cyan-500/20",
  ROTATION: "bg-orange-500/15 text-orange-500 border-orange-500/20",
  BLUETOOTH: "bg-indigo-500/15 text-indigo-500 border-indigo-500/20",
  SERVICE: "bg-pink-500/15 text-pink-500 border-pink-500/20",
};

function formatTime(timestamp: string) {
  try {
    const date = new Date(timestamp);
    return date.toLocaleTimeString("en-US", { hour12: false });
  } catch {
    return "??:??:??";
  }
}

export default function EventStream({ events }: EventStreamProps) {
  if (events.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-10 text-center">
        <div className="mb-3 flex size-10 items-center justify-center rounded-lg bg-muted">
          <IconBolt className="size-4 text-muted-foreground" stroke={1.5} />
        </div>
        <p className="text-sm text-muted-foreground">No events recorded yet</p>
      </div>
    );
  }

  return (
    <div className="max-h-[400px] space-y-0.5 overflow-y-auto">
      {events.map((event, i) => (
        <div
          key={`${event.timestamp}-${i}`}
          className={cn(
            "flex items-center gap-3 rounded-lg px-2.5 py-2 transition-colors hover:bg-muted/40",
            event.severity === "error" && "bg-red-500/5",
            event.severity === "warning" && "bg-amber-500/5"
          )}
        >
          <span className={cn(
            "inline-block size-1.5 shrink-0 rounded-full",
            severityDot[event.severity] ?? "bg-muted-foreground/30"
          )} />
          <span className="shrink-0 font-mono text-[11px] tabular-nums text-muted-foreground/60">
            {formatTime(event.timestamp)}
          </span>
          <span
            className={cn(
              "shrink-0 rounded-md border px-1.5 py-0.5 font-mono text-[10px] font-medium",
              categoryColor[event.category] ?? "border-border bg-muted text-muted-foreground"
            )}
          >
            {event.category}
          </span>
          <span className={cn("min-w-0 truncate text-[12px]", severityColor[event.severity])}>
            {event.message}
          </span>
        </div>
      ))}
    </div>
  );
}
