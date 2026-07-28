import { useEffect, useState } from "react";
import { toast } from "sonner";
import { getBatteryStatus, setChargeLimit } from "@/lib/tauri";
import { refreshSettings, useDispatch } from "@/lib/store";
import type { BatteryStatus } from "@/types/duo";

const defaultBatteryStatus: BatteryStatus = {
  present: false,
  capacityPercent: null,
  energyWh: null,
  dischargePowerW: null,
  state: "Unknown",
  chargeLimitSupported: false,
  configuredChargeLimitPercent: 100,
  activeChargeLimitPercent: null,
};

export function useBatteryStatus() {
  const dispatch = useDispatch();
  const [battery, setBattery] = useState(defaultBatteryStatus);
  const [loading, setLoading] = useState(true);
  const [settingLimit, setSettingLimit] = useState(false);

  useEffect(() => {
    let active = true;

    async function refresh() {
      try {
        const status = await getBatteryStatus();
        if (active) {
          setBattery(status);
        }
      } catch (error) {
        console.error("Failed to read battery status:", error);
      } finally {
        if (active) {
          setLoading(false);
        }
      }
    }

    void refresh();
    const timer = window.setInterval(refresh, 5_000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, []);

  const updateChargeLimit = async (value: string) => {
    const limit = Number(value);
    setSettingLimit(true);
    try {
      const status = await setChargeLimit(limit);
      setBattery(status);
      await refreshSettings(dispatch);
      toast.success(`Charge limit set to ${limit}%`);
    } catch (error) {
      const message =
        typeof error === "string"
          ? error
          : error instanceof Error
            ? error.message
            : "Failed to set the battery charge limit";
      toast.error(message);
    } finally {
      setSettingLimit(false);
    }
  };

  return { battery, loading, settingLimit, updateChargeLimit };
}
