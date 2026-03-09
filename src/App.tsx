import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import SubtitleView from "./components/SubtitleView";
import Settings from "./components/Settings";

type Page = "home" | "settings";

function App() {
  const [page, setPage] = useState<Page>("home");
  const [isFloatingOpen, setIsFloatingOpen] = useState(false);

  const handleToggleFloating = async () => {
    try {
      if (isFloatingOpen) {
        await invoke("close_floating_window");
        setIsFloatingOpen(false);
      } else {
        await invoke("open_floating_window");
        setIsFloatingOpen(true);
      }
    } catch (err) {
      console.error("Failed to toggle floating window:", err);
    }
  };

  return (
    <div className="app-layout">
      {/* Content Area (Header now integrated in components) */}
      <div className="app-content">
        {page === "home" ? (
          <SubtitleView
            onOpenSettings={() => setPage("settings")}
            onToggleFloating={handleToggleFloating}
            isFloatingOpen={isFloatingOpen}
          />
        ) : (
          <Settings onBack={() => setPage("home")} />
        )}
      </div>
    </div>
  );
}

export default App;
