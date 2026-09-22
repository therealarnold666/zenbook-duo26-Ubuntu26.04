import { useState } from "react";
import { useStore, useStoreInit } from "@/lib/store";
import Sidebar from "@/components/Sidebar";
import Status from "@/pages/Status";
import Controls from "@/pages/Controls";
import Settings from "@/pages/Settings";
import Logs from "@/pages/Logs";
import DisplayLayout from "@/pages/DisplayLayout";
import Profiles from "@/pages/Profiles";
import EventMonitor from "@/pages/EventMonitor";
import Diagnostics from "@/pages/Diagnostics";
import Setup from "@/pages/Setup";
import Performance from "@/pages/Performance";
import PerformanceStrip from "@/components/PerformanceStrip";

export type Page =
  | "status"
  | "controls"
  | "settings"
  | "logs"
  | "display"
  | "profiles"
  | "events"
  | "diagnostics"
  | "performance";

const pageComponents: Record<Page, React.ComponentType> = {
  status: Status,
  controls: Controls,
  settings: Settings,
  logs: Logs,
  display: DisplayLayout,
  profiles: Profiles,
  events: EventMonitor,
  diagnostics: Diagnostics,
  performance: Performance,
};

export default function App() {
  const [currentPage, setCurrentPage] = useState<Page>("status");

  useStoreInit();
  const store = useStore();

  if (!store.loading && !store.settings.setupCompleted) {
    return <Setup />;
  }

  const PageComponent = pageComponents[currentPage];

  return (
    <div className="flex h-screen overflow-hidden bg-background">
      <Sidebar currentPage={currentPage} onNavigate={setCurrentPage} />
      <div className="flex flex-1 flex-col overflow-hidden">
        {/* Subtle top border accent */}
        <div className="h-px w-full bg-gradient-to-r from-transparent via-primary/20 to-transparent" />
        <PerformanceStrip />
        <main className="flex-1 overflow-y-auto px-8 py-7" key={currentPage}>
          <div className="animate-page-enter mx-auto max-w-4xl">
            <PageComponent />
          </div>
        </main>
      </div>
    </div>
  );
}
