import { useEffect, useState } from "react";
import { useStore, useDispatch, refreshSettings } from "@/lib/store";
import {
  loadTrayPerformanceSettings,
  saveSettings,
  saveTrayPerformanceSettings,
} from "@/lib/tauri";
import type {
  DuoSettings,
  ThemePreference,
  TrayPerformanceMetric,
  TrayPerformanceSettings,
} from "@/types/duo";
import { useTheme } from "next-themes";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { useUsbMediaRemap } from "@/hooks/use-usb-media-remap";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  IconChartHistogram,
  IconChevronDown,
  IconChevronUp,
  IconDeviceFloppy,
  IconKeyboard,
  IconPlayerPause,
  IconPlayerPlay,
} from "@tabler/icons-react";

const trayMetricLabels: Record<TrayPerformanceMetric, string> = {
  cpuUsage: "CPU usage",
  cpuTemperature: "CPU temperature",
  gpuUsage: "GPU usage",
  gpuTemperature: "GPU temperature",
  memoryUsage: "Memory usage",
  gpuMemoryUsage: "Graphics memory usage",
};

const defaultTrayPerformanceSettings: TrayPerformanceSettings = {
  enabled: true,
  items: [
    { metric: "cpuUsage", enabled: true },
    { metric: "cpuTemperature", enabled: true },
    { metric: "gpuUsage", enabled: true },
    { metric: "gpuTemperature", enabled: true },
    { metric: "memoryUsage", enabled: true },
    { metric: "gpuMemoryUsage", enabled: true },
  ],
};

export default function Settings() {
  const store = useStore();
  const dispatch = useDispatch();
  const { setTheme } = useTheme();
  const [saving, setSaving] = useState(false);
  const [localSettings, setLocalSettings] = useState<DuoSettings>({
    ...store.settings,
  });
  const [traySettings, setTraySettings] = useState<TrayPerformanceSettings>(
    defaultTrayPerformanceSettings
  );

  useEffect(() => {
    let cancelled = false;
    loadTrayPerformanceSettings()
      .then((settings) => {
        if (!cancelled) setTraySettings(settings);
      })
      .catch((error) => console.error("Failed to load tray performance settings:", error));
    return () => {
      cancelled = true;
    };
  }, []);
  const {
    isUsb,
    remapBusy,
    remapDesired,
    remapStatus,
    statusLabel,
    setEnabled,
    togglePause,
  } = useUsbMediaRemap({
    settings: localSettings,
    onSettingsSaved: setLocalSettings,
  });

  const updateLocal = (key: keyof DuoSettings, value: unknown) => {
    setLocalSettings((prev) => ({ ...prev, [key]: value }));
  };

  const setTrayMetricEnabled = (metric: TrayPerformanceMetric, enabled: boolean) => {
    setTraySettings((prev) => ({
      ...prev,
      items: prev.items.map((item) =>
        item.metric === metric ? { ...item, enabled } : item
      ),
    }));
  };

  const moveTrayMetric = (index: number, offset: -1 | 1) => {
    setTraySettings((prev) => {
      const destination = index + offset;
      if (destination < 0 || destination >= prev.items.length) return prev;

      const items = [...prev.items];
      [items[index], items[destination]] = [items[destination], items[index]];
      return { ...prev, items };
    });
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await Promise.all([
        saveSettings(localSettings),
        saveTrayPerformanceSettings(traySettings),
      ]);
      await refreshSettings(dispatch);

      const themeMap: Record<ThemePreference, string> = {
        system: "system",
        dark: "dark",
        light: "light",
      };
      setTheme(themeMap[localSettings.theme]);

      toast.success("Settings saved");
    } catch (err) {
      console.error("Failed to save settings:", err);
      toast.error("Failed to save settings");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div>
      <div className="mb-6">
        <h1 className="text-xl font-semibold tracking-tight">Settings</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Configure default behavior and preferences
        </p>
      </div>

      <div className="glass-card animate-stagger-in stagger-1 rounded-xl p-5">
        <div className="mb-5 flex items-center justify-between gap-4">
          <div className="flex items-center gap-2.5">
            <div className="flex size-7 items-center justify-center rounded-lg bg-primary/10 text-primary">
              <IconDeviceFloppy className="size-3.5" stroke={1.75} />
            </div>
            <h3 className="text-[13px] font-semibold text-foreground">
              Defaults
            </h3>
          </div>
          <Button onClick={handleSave} disabled={saving} size="sm" className="gap-2">
            <IconDeviceFloppy className="size-3.5" stroke={1.5} />
            {saving ? "Saving..." : "Save"}
          </Button>
        </div>

        <div className="space-y-5">
          <SettingRow label="Default Backlight Level" description="Applied when the keyboard connects">
            <Select
              value={String(localSettings.defaultBacklight)}
              onValueChange={(v) => updateLocal("defaultBacklight", parseInt(v))}
            >
              <SelectTrigger className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="0">0 - Off</SelectItem>
                <SelectItem value="1">1 - Low</SelectItem>
                <SelectItem value="2">2 - Medium</SelectItem>
                <SelectItem value="3">3 - High</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>

          <div className="h-px bg-border/50" />

          <SettingRow label="Default Display Scale" description="Scale factor for newly connected displays">
            <Select
              value={String(localSettings.defaultScale)}
              onValueChange={(v) => updateLocal("defaultScale", parseFloat(v))}
            >
              <SelectTrigger className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="1">1.0x (100%)</SelectItem>
                <SelectItem value="1.25">1.25x (125%)</SelectItem>
                <SelectItem value="1.5">1.5x (150%)</SelectItem>
                <SelectItem value="1.66">1.66x (166%)</SelectItem>
                <SelectItem value="1.67">1.67x (167%)</SelectItem>
                <SelectItem value="2">2.0x (200%)</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>

          <div className="h-px bg-border/50" />

          <SettingRow label="Theme" description="Application appearance">
            <Select
              value={localSettings.theme}
              onValueChange={(v) => updateLocal("theme", v as ThemePreference)}
            >
              <SelectTrigger className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="system">System</SelectItem>
                <SelectItem value="dark">Dark</SelectItem>
                <SelectItem value="light">Light</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>
        </div>
      </div>

      <div className="mt-5 glass-card animate-stagger-in stagger-2 rounded-xl p-5">
        <div className="mb-5 flex items-center justify-between gap-4">
          <div className="flex items-center gap-2.5">
            <div className="flex size-7 items-center justify-center rounded-lg bg-cyan-500/10 text-cyan-600 dark:text-cyan-400">
              <IconChartHistogram className="size-3.5" stroke={1.75} />
            </div>
            <div>
              <h3 className="text-[13px] font-semibold text-foreground">Top Panel Performance</h3>
              <p className="mt-0.5 text-[12px] text-muted-foreground">
                Choose which live metrics appear beside the tray icon and in what order
              </p>
            </div>
          </div>
          <Button onClick={handleSave} disabled={saving} size="sm" className="gap-2">
            <IconDeviceFloppy className="size-3.5" stroke={1.5} />
            {saving ? "Saving..." : "Save"}
          </Button>
        </div>

        <SettingRow
          label="Show performance metrics"
          description="Keep the tray icon while hiding or showing its live text"
        >
          <Switch
            checked={traySettings.enabled}
            onCheckedChange={(enabled) => setTraySettings((prev) => ({ ...prev, enabled }))}
          />
        </SettingRow>

        <div className="my-4 h-px bg-border/50" />

        <div className="space-y-1">
          {traySettings.items.map((item, index) => (
            <div
              key={item.metric}
              className="flex min-h-10 items-center gap-3 border-b border-border/40 px-1 py-1.5 last:border-b-0"
            >
              <span className="w-5 text-center font-mono text-[11px] tabular-nums text-muted-foreground/60">
                {index + 1}
              </span>
              <Switch
                checked={item.enabled}
                onCheckedChange={(enabled) => setTrayMetricEnabled(item.metric, enabled)}
                disabled={!traySettings.enabled}
                aria-label={`Show ${trayMetricLabels[item.metric]}`}
              />
              <span className="flex-1 text-[13px] font-medium text-foreground">
                {trayMetricLabels[item.metric]}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                onClick={() => moveTrayMetric(index, -1)}
                disabled={index === 0}
                title="Move up"
                aria-label={`Move ${trayMetricLabels[item.metric]} up`}
              >
                <IconChevronUp className="size-4" />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                onClick={() => moveTrayMetric(index, 1)}
                disabled={index === traySettings.items.length - 1}
                title="Move down"
                aria-label={`Move ${trayMetricLabels[item.metric]} down`}
              >
                <IconChevronDown className="size-4" />
              </Button>
            </div>
          ))}
        </div>
      </div>

      <div className="mt-5 glass-card animate-stagger-in stagger-2 rounded-xl p-5">
        <div className="mb-5 flex items-center gap-2.5">
          <div className="flex size-7 items-center justify-center rounded-lg bg-teal-500/10 text-teal-500 dark:bg-teal-400/10 dark:text-teal-400">
            <IconKeyboard className="size-3.5" stroke={1.75} />
          </div>
          <h3 className="text-[13px] font-semibold text-foreground">
            Keyboard
          </h3>
        </div>

        <div className="space-y-4">
          <SettingRow
            label="USB Media Remap"
            description="Maps F1-F3/F5-F6 to media and brightness keys while docked"
            labelExtra={
              store.status.connectionType === "bluetooth" ? (
                <Badge
                  className="border-amber-500/20 bg-amber-500/10 text-amber-700 dark:border-amber-400/25 dark:bg-amber-400/10 dark:text-amber-200"
                >
                  Unavailable on bluetooth
                </Badge>
              ) : null
            }
          >
            <div className="flex items-center gap-3">
              <Switch
                checked={remapDesired ?? remapStatus.running}
                onCheckedChange={setEnabled}
                disabled={!isUsb || remapBusy || remapDesired !== null}
              />
              <span className="text-[12px] text-muted-foreground">
                {statusLabel}
              </span>
              {remapStatus.running && remapDesired === null && (
                <Button
                  size="sm"
                  variant={remapStatus.paused ? "default" : "outline"}
                  className="gap-1.5"
                  onClick={togglePause}
                >
                  {remapStatus.paused ? (
                    <><IconPlayerPlay className="size-3.5" stroke={1.5} /> Resume</>
                  ) : (
                    <><IconPlayerPause className="size-3.5" stroke={1.5} /> Pause</>
                  )}
                </Button>
              )}
              {remapStatus.running && remapStatus.paused && (
                <Badge
                  className="border-amber-500/20 bg-amber-500/10 text-amber-700 dark:border-amber-400/25 dark:bg-amber-400/10 dark:text-amber-200"
                >
                  Paused
                </Badge>
              )}
              {(remapBusy || remapDesired !== null) && <Spinner className="text-muted-foreground" />}
            </div>
          </SettingRow>

          <p className="rounded-lg bg-muted/50 px-3 py-2 text-[12px] text-muted-foreground">
            Requires admin approval and restarts input handling while enabled.
            You can pause/resume from the button above or from the system tray.
            For a keyboard shortcut, add a compositor keybinding that runs{" "}
            <code className="rounded bg-muted px-1 py-0.5 text-[11px]">
              zenbook-duo-control --toggle-remap-pause
            </code>.
          </p>
        </div>
      </div>

    </div>
  );
}

function SettingRow({
  label,
  description,
  labelExtra,
  children,
}: {
  label: string;
  description?: string;
  labelExtra?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div>
        <div className="flex items-center gap-2">
          <Label className="text-[13px] font-medium">{label}</Label>
          {labelExtra}
        </div>
        {description && (
          <p className="mt-0.5 text-[12px] text-muted-foreground">{description}</p>
        )}
      </div>
      {children}
    </div>
  );
}
