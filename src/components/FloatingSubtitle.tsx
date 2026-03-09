import { useState, useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./FloatingSubtitle.css";

export default function FloatingSubtitle() {
  const [original, setOriginal] = useState("");
  const [translated, setTranslated] = useState("");

  useEffect(() => {
    console.log("FloatingSubtitle mounted");
    
    let unlistenOriginal: UnlistenFn | null = null;
    let unlistenTranslated: UnlistenFn | null = null;

    const setupListeners = async () => {
      try {
        unlistenOriginal = await listen<string>("subtitle-original", (event) => {
          setOriginal(event.payload);
          setTranslated(""); // Clear previous translation
        });

        unlistenTranslated = await listen<string>("subtitle-translated", (event) => {
          setTranslated(event.payload);
        });
        
        console.log("Listeners attached successfully");
      } catch (err) {
        console.error("Failed to attach listeners:", err);
      }
    };

    setupListeners();

    return () => {
      console.log("FloatingSubtitle unmounting, cleaning up listeners");
      if (unlistenOriginal) unlistenOriginal();
      if (unlistenTranslated) unlistenTranslated();
    };
  }, []);

  const handleDrag = () => {
    try {
      getCurrentWindow().startDragging();
    } catch (err) {
      console.error("Failed to start dragging:", err);
    }
  };

  const handleClose = () => {
    try {
      getCurrentWindow().close();
    } catch (err) {
      console.error("Failed to close window:", err);
    }
  };

  return (
    <div className="floating-container">
      <div className="floating-drag-area" onMouseDown={handleDrag} title="拖动" />
      <button className="floating-close" onClick={handleClose} title="关闭">
        ✕
      </button>
      <div className="floating-content">
        {original ? (
          <>
            <div className="floating-text-original">{original}</div>
            {translated && (
              <div className="floating-text-translated">{translated}</div>
            )}
          </>
        ) : (
          <div className="floating-empty">等待字幕...</div>
        )}
      </div>
    </div>
  );
}
