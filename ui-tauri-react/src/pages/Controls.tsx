import { useState, useEffect } from "react";
import BacklightSlider from "@/components/BacklightSlider";
import OrientationButtons from "@/components/OrientationButtons";
import { restartService, listTouchscreens, setTouchscreenEnabled, loadSettings, saveSettings, applyPerformanceMode } from "@/lib/tauri";
import type { PerformanceMode, TouchscreenDevice } from "@/types/duo";
import { Switch } from "@/components/ui/switch";
import { refreshStatus, useDispatch, useStore } from "@/lib/store";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import {
  IconRefresh,
  IconKeyboard,
  IconRotate,
  IconServer,
  IconCheck,
  IconHandFinger,
  IconGauge,
} from "@tabler/icons-react";

export default function Controls() {
  const dispatch = useDispatch();
  const store = useStore();
  const [restarting, setRestarting] = useState(false);
  const [restarted, setRestarted] = useState(false);
  const [touchscreens, setTouchscreens] = useState<TouchscreenDevice[]>([]);

  useEffect(() => {
    listTouchscreens().then(setTouchscreens).catch(console.error);
  }, []);

  const handleTouchToggle = async (connector: string, enabled: boolean) => {
    try {
      await setTouchscreenEnabled(connector, enabled);
      setTouchscreens((prev) =>
        prev.map((ts) => (ts.connector === connector ? { ...ts, enabled } : ts))
      );
      const settings = await loadSettings();
      const disabled = settings.touchscreenDisabled ?? [];
      settings.touchscreenDisabled = enabled
        ? disabled.filter((c) => c !== connector)
        : [...disabled.filter((c) => c !== connector), connector];
      await saveSettings(settings);
    } catch (e) {
      console.error("Failed to toggle touchscreen:", e);
    }
  };

  const handleAutoRotateToggle = async (autoRotate: boolean) => {
    const settings = { ...store.settings, autoRotate };
    try {
      await saveSettings(settings);
      dispatch({ type: "SET_SETTINGS", payload: settings });
    } catch (err) {
      console.error("Failed to save auto-rotate setting:", err);
    }
  };

  const handleKeyboardPowerSaveToggle = async (keyboardBacklightPowerSave: boolean) => {
    const settings = { ...store.settings, keyboardBacklightPowerSave };
    try {
      await saveSettings(settings);
      dispatch({ type: "SET_SETTINGS", payload: settings });
    } catch (err) {
      console.error("Failed to save keyboard power-save setting:", err);
    }
  };

  const handleAutoQuietOnBatteryToggle = async (autoQuietOnBattery: boolean) => {
    const settings = { ...store.settings, autoQuietOnBattery };
    try {
      await saveSettings(settings);
      dispatch({ type: "SET_SETTINGS", payload: settings });
    } catch (err) {
      console.error("Failed to save battery performance setting:", err);
    }
  };

  const handlePerformanceMode = async (activePerformanceMode: PerformanceMode) => {
    const settings = { ...store.settings, activePerformanceMode };
    try {
      await saveSettings(settings);
      await applyPerformanceMode(activePerformanceMode);
      dispatch({ type: "SET_SETTINGS", payload: settings });
    } catch (err) {
      console.error("Failed to apply performance mode:", err);
    }
  };

  const handlePowerLimitChange = async (field: "pl1Watts" | "pl2Watts" | "pl3Watts", value: string) => {
    const watts = Number.parseInt(value, 10);
    if (!Number.isFinite(watts)) return;
    const mode = store.settings.activePerformanceMode;
    const settings = {
      ...store.settings,
      performanceProfiles: {
        ...store.settings.performanceProfiles,
        [mode]: { ...store.settings.performanceProfiles[mode], [field]: watts },
      },
    };
    try {
      await saveSettings(settings);
      dispatch({ type: "SET_SETTINGS", payload: settings });
    } catch (err) {
      console.error("Failed to save power limits:", err);
    }
  };

  const handleRestart = async () => {
    setRestarting(true);
    setRestarted(false);
    try {
      await restartService();
      setTimeout(async () => {
        await refreshStatus(dispatch);
        setRestarting(false);
        setRestarted(true);
        setTimeout(() => setRestarted(false), 3000);
      }, 2000);
    } catch (err) {
      console.error("Failed to restart service:", err);
      setRestarting(false);
    }
  };

  return (
    <div>
      <div className="mb-6">
        <h1 className="text-xl font-semibold tracking-tight">Controls</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Adjust hardware settings in real time
        </p>
      </div>

      <div className="space-y-5">
        <div className="glass-card animate-stagger-in stagger-1 rounded-xl p-5">
          <div className="mb-4 flex items-start justify-between gap-3">
            <div className="flex items-center gap-2.5">
              <div className="flex size-7 items-center justify-center rounded-lg bg-rose-500/12 text-rose-500 dark:bg-rose-400/10 dark:text-rose-400">
                <IconGauge className="size-3.5" stroke={1.75} />
              </div>
              <div>
                <h3 className="text-[13px] font-semibold text-foreground">Performance Mode</h3>
                <p className="text-[11px] text-muted-foreground">ASUS fan strategy and CPU power limits</p>
              </div>
            </div>
            <label className="flex items-center gap-2 text-[11px] text-muted-foreground">
              Battery saver
              <Switch
                checked={store.settings.autoQuietOnBattery}
                onCheckedChange={handleAutoQuietOnBatteryToggle}
                aria-label="Automatically switch to Quiet mode on battery power"
              />
            </label>
          </div>
          <div className="grid grid-cols-3 gap-2">
            {(["quiet", "balanced", "performance"] as PerformanceMode[]).map((mode) => {
              const limits = store.settings.performanceProfiles[mode];
              const active = store.settings.activePerformanceMode === mode;
              return <button key={mode} onClick={() => handlePerformanceMode(mode)} className={cn("rounded-lg border px-2 py-2 text-left transition-colors", active ? "border-rose-500/50 bg-rose-500/10" : "border-border hover:bg-muted/50")}>
                <span className="block text-xs font-semibold capitalize">{mode}</span>
                <span className="mt-1 block font-mono text-[10px] text-muted-foreground">{limits.pl1Watts}/{limits.pl2Watts}/{limits.pl3Watts} W</span>
              </button>;
            })}
          </div>
          <div className="mt-3 grid grid-cols-3 gap-2">
            {(["pl1Watts", "pl2Watts", "pl3Watts"] as const).map((field, index) => (
              <label key={field} className="text-[10px] text-muted-foreground">
                PL{index + 1} (W)
                <input type="number" min={index === 0 ? 5 : 5} max={index === 0 ? 100 : index === 1 ? 150 : 200} value={store.settings.performanceProfiles[store.settings.activePerformanceMode][field]} onChange={(event) => handlePowerLimitChange(field, event.target.value)} className="mt-1 w-full rounded-md border border-border bg-background px-2 py-1.5 font-mono text-xs text-foreground" />
              </label>
            ))}
          </div>
          <p className="mt-3 text-[10px] text-muted-foreground">PL1 / PL2 / PL3. Edit values, then click the selected mode to apply. MMIO RAPL validates PL1 ≤ PL2 ≤ PL3.</p>
        </div>

        <div className="glass-card animate-stagger-in stagger-1 rounded-xl p-5">
          <div className="mb-5 flex items-start justify-between gap-3">
            <div className="flex items-center gap-2.5">
              <div className="flex size-7 items-center justify-center rounded-lg bg-amber-500/12 text-amber-500 dark:bg-amber-400/10 dark:text-amber-400">
                <IconKeyboard className="size-3.5" stroke={1.75} />
              </div>
              <div>
                <h3 className="text-[13px] font-semibold text-foreground">
                  Keyboard Backlight
                </h3>
                <p className="text-[11px] text-muted-foreground">Adjust brightness level</p>
              </div>
            </div>
            <label className="flex items-center gap-2 text-[11px] text-muted-foreground">
              Power save
              <Switch
                checked={store.settings.keyboardBacklightPowerSave}
                onCheckedChange={handleKeyboardPowerSaveToggle}
                aria-label="Turn off detached keyboard backlight after inactivity"
              />
            </label>
          </div>
          <BacklightSlider />
        </div>

        <div className="glass-card animate-stagger-in stagger-2 rounded-xl p-5">
          <div className="mb-5 flex items-start justify-between gap-3">
            <div className="flex items-center gap-2.5">
              <div className="flex size-7 items-center justify-center rounded-lg bg-blue-500/12 text-blue-500 dark:bg-blue-400/10 dark:text-blue-400">
                <IconRotate className="size-3.5" stroke={1.75} />
              </div>
              <div>
                <h3 className="text-[13px] font-semibold text-foreground">
                  Screen Orientation
                </h3>
                <p className="text-[11px] text-muted-foreground">
                  Current: <span className="font-mono capitalize">{store.status.orientation}</span>
                </p>
              </div>
            </div>
            <label className="flex items-center gap-2 text-[11px] text-muted-foreground">
              Auto rotate
              <Switch
                checked={store.settings.autoRotate}
                onCheckedChange={handleAutoRotateToggle}
                aria-label="Automatically rotate both displays"
              />
            </label>
          </div>
          <OrientationButtons />
        </div>

        <div className="glass-card animate-stagger-in stagger-3 rounded-xl p-5">
          <div className="mb-4 flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <div className={cn(
                "flex size-7 items-center justify-center rounded-lg",
                store.status.serviceActive
                  ? "bg-emerald-500/12 text-emerald-500 dark:bg-emerald-400/10 dark:text-emerald-400"
                  : "bg-destructive/12 text-destructive"
              )}>
                <IconServer className="size-3.5" stroke={1.75} />
              </div>
              <div>
                <h3 className="text-[13px] font-semibold text-foreground">
                  Service Control
                </h3>
                <p className="text-[11px] text-muted-foreground">
                  Rust runtime is{" "}
                  <span className={cn(
                    "font-semibold",
                    store.status.serviceActive ? "text-emerald-500" : "text-destructive"
                  )}>
                    {store.status.serviceActive ? "running" : "stopped"}
                  </span>
                </p>
              </div>
            </div>
            <Button
              variant={restarted ? "outline" : "outline"}
              size="sm"
              onClick={handleRestart}
              disabled={restarting}
              className={cn(
                "gap-2 transition-all",
                restarted && "border-emerald-500/30 text-emerald-500"
              )}
            >
              {restarted ? (
                <IconCheck className="size-3.5" stroke={2} />
              ) : (
                <IconRefresh className={cn("size-3.5", restarting && "animate-spin")} stroke={1.5} />
              )}
              {restarting ? "Restarting..." : restarted ? "Restarted" : "Restart"}
            </Button>
          </div>
        </div>

        {touchscreens.length > 0 && (
          <div className="glass-card animate-stagger-in stagger-4 rounded-xl p-5">
            <div className="mb-5 flex items-center gap-2.5">
              <div className="flex size-7 items-center justify-center rounded-lg bg-purple-500/12 text-purple-500 dark:bg-purple-400/10 dark:text-purple-400">
                <IconHandFinger className="size-3.5" stroke={1.75} />
              </div>
              <div>
                <h3 className="text-[13px] font-semibold text-foreground">
                  Touchscreen
                </h3>
                <p className="text-[11px] text-muted-foreground">
                  Enable or disable touch input per display
                </p>
              </div>
            </div>
            <div className="space-y-3">
              {touchscreens.map((ts) => (
                <div key={ts.connector} className="flex items-center justify-between">
                  <span className="text-[13px]">
                    {ts.connector}
                    <span className="text-muted-foreground ml-2 text-[11px]">
                      {ts.name}
                    </span>
                  </span>
                  <Switch
                    checked={ts.enabled}
                    onCheckedChange={(checked) =>
                      handleTouchToggle(ts.connector, checked)
                    }
                  />
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
