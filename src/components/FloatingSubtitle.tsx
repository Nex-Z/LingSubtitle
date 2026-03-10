import { useState, useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./FloatingSubtitle.css";

interface FloatingSettings {
  fontSize: number;
  bgOpacity: number;
  showOriginal: boolean;
}

const DEFAULT_SETTINGS: FloatingSettings = {
  fontSize: 18,
  bgOpacity: 0.85,
  showOriginal: true,
};

export default function FloatingSubtitle() {
  const [original, setOriginal] = useState("");
  const [translated, setTranslated] = useState("");
  const [hasTranslation, setHasTranslation] = useState(false); // tracks if translation mode is active
  const [settings, setSettings] = useState<FloatingSettings>(() => {
    try {
      const saved = localStorage.getItem("floating-subtitle-settings");
      return saved ? { ...DEFAULT_SETTINGS, ...JSON.parse(saved) } : DEFAULT_SETTINGS;
    } catch {
      return DEFAULT_SETTINGS;
    }
  });
  const [showSettings, setShowSettings] = useState(false);
  const settingsRef = useRef<HTMLDivElement>(null);

  // Persist settings
  useEffect(() => {
    localStorage.setItem("floating-subtitle-settings", JSON.stringify(settings));
  }, [settings]);

  // Click outside to close settings
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (settingsRef.current && !settingsRef.current.contains(e.target as Node)) {
        setShowSettings(false);
      }
    };
    if (showSettings) document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [showSettings]);

  useEffect(() => {
    let unlistenOriginal: UnlistenFn | null = null;
    let unlistenTranslated: UnlistenFn | null = null;

    const setupListeners = async () => {
      try {
        unlistenOriginal = await listen<string>("subtitle-original", (event) => {
          setOriginal(event.payload);
          setTranslated(""); // clear stale translation for new sentence
        });

        unlistenTranslated = await listen<string>("subtitle-translated", (event) => {
          setTranslated(event.payload);
          setHasTranslation(true); // we know translation mode is active
        });
      } catch (err) {
        console.error("Failed to attach listeners:", err);
      }
    };

    setupListeners();

    return () => {
      if (unlistenOriginal) unlistenOriginal();
      if (unlistenTranslated) unlistenTranslated();
    };
  }, []);

  const handleDrag = () => {
    try { getCurrentWindow().startDragging(); } catch {}
  };

  const handleClose = () => {
    try { getCurrentWindow().close(); } catch {}
  };

  const updateSetting = <K extends keyof FloatingSettings>(key: K, value: FloatingSettings[K]) => {
    setSettings((prev) => ({ ...prev, [key]: value }));
  };

  // Determine display mode
  const isDualLine = hasTranslation && settings.showOriginal;
  const mainText = translated || original;
  const subText = translated ? original : "";

  return (
    <div
      className="floating-container"
      style={{ background: `rgba(20, 20, 22, ${settings.bgOpacity})` }}
    >
      <div className="floating-drag-area" onMouseDown={handleDrag} title="拖动" />

      {/* Hover toolbar */}
      <div className="floating-toolbar">
        <button className="floating-btn" onClick={() => setShowSettings(!showSettings)} title="设置">⚙</button>
        <button className="floating-btn close" onClick={handleClose} title="关闭">✕</button>
      </div>

      {/* Settings Panel */}
      {showSettings && (
        <div className="floating-settings-panel" ref={settingsRef}>
          <div className="fs-row">
            <span className="fs-label">字号</span>
            <div className="fs-size-btns">
              {[14, 16, 18, 22, 26].map((s) => (
                <button
                  key={s}
                  className={`fs-size-btn ${settings.fontSize === s ? "active" : ""}`}
                  onClick={() => updateSetting("fontSize", s)}
                >{s}</button>
              ))}
            </div>
          </div>
          <div className="fs-row">
            <span className="fs-label">透明度</span>
            <input
              type="range" className="fs-slider"
              min="0.3" max="1" step="0.05"
              value={settings.bgOpacity}
              onChange={(e) => updateSetting("bgOpacity", parseFloat(e.target.value))}
            />
          </div>
          <div className="fs-row">
            <span className="fs-label">显示原文</span>
            <label className="fs-toggle">
              <input type="checkbox" checked={settings.showOriginal}
                onChange={(e) => updateSetting("showOriginal", e.target.checked)} />
              <span className="fs-toggle-slider" />
            </label>
          </div>
        </div>
      )}

      {/* Fixed-layout subtitle area */}
      <div className={`floating-subtitle-area ${isDualLine ? "dual" : "single"}`}>
        {original || translated ? (
          <>
            {/* Line 1: Main text (translation if available, otherwise original) */}
            <div
              className={`subtitle-line main ${translated ? "has-translation" : ""}`}
              style={{ fontSize: `${settings.fontSize}px` }}
            >
              {mainText}
            </div>

            {/* Line 2: Original text (only in dual mode, when translated exists) */}
            <div
              className={`subtitle-line sub ${subText ? "visible" : ""}`}
              style={{ fontSize: `${Math.max(12, settings.fontSize - 4)}px` }}
            >
              {subText || "\u00A0"}
            </div>
          </>
        ) : (
          <div className="floating-empty">等待字幕...</div>
        )}
      </div>
    </div>
  );
}
