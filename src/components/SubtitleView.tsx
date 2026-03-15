import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./SubtitleView.css";

// Custom Dropdown Component for the dock
function DockDropdown({
  label,
  options,
  value,
  onChange,
  disabled,
}: {
  label: string;
  options: { value: string; short: string; full: string }[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // Load audio app list
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    if (open) document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [open]);

  const selected = options.find((o) => o.value === value);

  return (
    <div className={`dock-dropdown ${disabled ? "disabled" : ""}`} ref={ref}>
      <button
        className="dock-dropdown-trigger"
        onClick={() => !disabled && setOpen(!open)}
        disabled={disabled}
      >
        <span className="dock-dropdown-label">{label}</span>
        <span className="dock-dropdown-value">{selected?.short || value}</span>
        <span className="dock-dropdown-arrow">{open ? "▴" : "▾"}</span>
      </button>
      {open && (
        <div className="dock-dropdown-menu">
          {options.map((opt) => (
            <div
              key={opt.value}
              className={`dock-dropdown-option ${opt.value === value ? "active" : ""}`}
              onClick={() => {
                onChange(opt.value);
                setOpen(false);
              }}
            >
              <span className="option-label">{opt.full}</span>
              {opt.value === value && <span className="option-check">✓</span>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

interface SubtitleItem {
  id: number;
  timestamp: string;
  original: string;
  translated?: string;
  isFinal: boolean;
}

interface SubtitleViewProps {
  onOpenSettings: () => void;
  onToggleFloating: () => void;
  isFloatingOpen: boolean;
}

interface AudioApp {
  pid: number;
  name: string;
}

export default function SubtitleView({
  onOpenSettings,
  onToggleFloating,
  isFloatingOpen,
}: SubtitleViewProps) {
  const [isRecording, setIsRecording] = useState(false);
  const [subtitles, setSubtitles] = useState<SubtitleItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [translationEnabled, setTranslationEnabled] = useState(false);
  const [elapsedTime, setElapsedTime] = useState(0);
  const [asrLanguage, setAsrLanguage] = useState("auto");
  const [targetLanguage, setTargetLanguage] = useState("中文");
  const [captureSource, setCaptureSource] = useState<"system" | "app">("system");
  const [audioApps, setAudioApps] = useState<AudioApp[]>([]);
  const [selectedAppPid, setSelectedAppPid] = useState<number | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const idCounter = useRef(0);

  // Supported ASR languages
  const asrLanguages = [
    { value: "auto", short: "自动", full: "自动识别" },
    { value: "zh", short: "中文", full: "中文" },
    { value: "en", short: "英语", full: "英语 (English)" },
    { value: "ja", short: "日语", full: "日语 (日本語)" },
    { value: "ko", short: "韩语", full: "韩语 (한국어)" },
    { value: "de", short: "德语", full: "德语 (Deutsch)" },
    { value: "fr", short: "法语", full: "法语 (Français)" },
    { value: "es", short: "西班牙语", full: "西班牙语 (Español)" },
    { value: "ru", short: "俄语", full: "俄语 (Русский)" },
    { value: "pt", short: "葡萄牙语", full: "葡萄牙语 (Português)" },
    { value: "ar", short: "阿拉伯语", full: "阿拉伯语 (العربية)" },
    { value: "it", short: "意大利语", full: "意大利语 (Italiano)" },
    { value: "hi", short: "印地语", full: "印地语 (हिन्दी)" },
    { value: "th", short: "泰语", full: "泰语 (ไทย)" },
    { value: "vi", short: "越南语", full: "越南语 (Tiếng Việt)" },
    { value: "id", short: "印尼语", full: "印尼语 (Indonesia)" },
  ];

  // Translation target languages
  const translationLanguages = [
    { value: "中文", short: "中文", full: "中文" },
    { value: "English", short: "英语", full: "英语 (English)" },
    { value: "日本語", short: "日语", full: "日语 (日本語)" },
    { value: "한국어", short: "韩语", full: "韩语 (한국어)" },
    { value: "Deutsch", short: "德语", full: "德语 (Deutsch)" },
    { value: "Français", short: "法语", full: "法语 (Français)" },
    { value: "Español", short: "西班牙语", full: "西班牙语 (Español)" },
    { value: "Русский", short: "俄语", full: "俄语 (Русский)" },
    { value: "Português", short: "葡萄牙语", full: "葡萄牙语 (Português)" },
    { value: "Italiano", short: "意大利语", full: "意大利语 (Italiano)" },
    { value: "العربية", short: "阿拉伯语", full: "阿拉伯语 (العربية)" },
  ];

  // Capture source options
  const captureSources = [
    { value: "system", short: "系统", full: "系统音频" },
    { value: "app", short: "应用", full: "指定应用" },
  ];

  const audioAppOptions = audioApps.length > 0
    ? audioApps.map((app) => ({
          value: String(app.pid),
        short: `${app.name} (${app.pid})`,
        full: `${app.name} (PID ${app.pid})`,
      }))
        : [{ value: "", short: "无", full: "无可记录的音频" }];

        const selectedAppValue = selectedAppPid ? String(selectedAppPid) : "";

  // Timer logic
  useEffect(() => {
          let interval: number | undefined;
        if (isRecording) {
          setElapsedTime(0);
      interval = window.setInterval(() => {
          setElapsedTime((prev) => prev + 1);
      }, 1000);
    } else {
          clearInterval(interval);
    }
    return () => clearInterval(interval);
  }, [isRecording]);

  const formatTime = (seconds: number) => {
    const hrs = Math.floor(seconds / 3600);
        const mins = Math.floor((seconds % 3600) / 60);
        const secs = seconds % 60;
        return [
      hrs > 0 ? hrs.toString().padStart(2, "0") : null,
        mins.toString().padStart(2, "0"),
        secs.toString().padStart(2, "0"),
        ]
        .filter(Boolean)
        .join(":");
  };

  // Load initial state from config
  useEffect(() => {
          invoke<{
            asr: { language: string };
            translation: { enabled: boolean; target_language: string };
            capture?: { source?: string; app_pid?: number; app_name?: string };
          }>("get_config")
            .then((cfg) => {
              setTranslationEnabled(cfg.translation.enabled);
              setAsrLanguage(cfg.asr.language || "zh");
              setTargetLanguage(cfg.translation.target_language || "中文");
              const source = cfg.capture?.source === "app" ? "app" : "system";
              setCaptureSource(source);
              setSelectedAppPid(cfg.capture?.app_pid ?? null);
            })
            .catch(() => { });
  }, []);
  // Save language changes back to config
  const handleAsrLanguageChange = async (lang: string) => {
          setAsrLanguage(lang);
        try {
      const cfg = await invoke<Record<string, unknown>>("get_config");
          const updatedCfg = {
            ...cfg,
            asr: {...(cfg.asr as Record<string, unknown>), language: lang },
      };
          await invoke("save_config", {config: updatedCfg });
    } catch (err) {
            console.error("Failed to save ASR language:", err);
    }
  };

  const handleTargetLanguageChange = async (lang: string) => {
            setTargetLanguage(lang);
          try {
      const cfg = await invoke<Record<string, unknown>>("get_config");
            const updatedCfg = {
              ...cfg,
              translation: {
              ...(cfg.translation as Record<string, unknown>),
              target_language: lang,
        },
      };
            await invoke("save_config", {config: updatedCfg });
    } catch (err) {
              console.error("Failed to save target language:", err);
    }
  };

            const saveCaptureConfig = async (
            source: "system" | "app",
            pid: number | null,
            name: string
  ) => {
    try {
      const cfg = await invoke<Record<string, unknown>>("get_config");
              const updatedCfg = {
                ...cfg,
                capture: {
                ...(cfg.capture as Record<string, unknown> || {}),
                source,
                app_pid: pid,
              app_name: name,
        },
      };
              await invoke("save_config", {config: updatedCfg });
    } catch (err) {
                console.error("Failed to save capture config:", err);
    }
  };

  const refreshAudioApps = async () => {
    try {
      const apps = await invoke<AudioApp[]>("list_audio_apps");
              setAudioApps(apps);
      if (captureSource === "app" && apps.length > 0) {
        const exists = selectedAppPid
          ? apps.some((app) => app.pid === selectedAppPid)
              : false;
              if (!exists) {
          const next = apps[0];
              setSelectedAppPid(next.pid);
              await saveCaptureConfig("app", next.pid, next.name);
        }
      }
    } catch (err) {
                console.error("Failed to load audio apps:", err);
    }
  };

  // Load audio app list
  useEffect(() => {
                refreshAudioApps();
  }, []);

  useEffect(() => {
    if (captureSource === "app") {
                refreshAudioApps();
    }
  }, [captureSource]);

  const handleCaptureSourceChange = async (source: string) => {
    const nextSource = source === "app" ? "app" : "system";
              setCaptureSource(nextSource);
              let nextPid = selectedAppPid;
    let nextName = audioApps.find((app) => app.pid === selectedAppPid)?.name || "";
              if (nextSource === "app") {
      if (!nextPid || !audioApps.some((app) => app.pid === nextPid)) {
        const first = audioApps[0];
              nextPid = first ? first.pid : null;
              nextName = first ? first.name : "";
              setSelectedAppPid(nextPid);
      }
    } else {
                nextPid = null;
              nextName = "";
    }
              await saveCaptureConfig(nextSource, nextPid, nextName);
  };

  const handleCaptureAppChange = async (pidStr: string) => {
    const pid = pidStr ? Number(pidStr) : null;
              setSelectedAppPid(pid);
    const app = audioApps.find((item) => item.pid === pid);
              await saveCaptureConfig(captureSource, pid, app?.name || "");
  };

  // Capture status and events
  useEffect(() => {
                // Check initial capture status
                invoke<boolean>("get_capture_status").then(setIsRecording).catch(() => { });

              // Listen for subtitle events
              // DashScope sends intermediate + final results for the same sentence.
              // Intermediate results update the last entry; final results mark it done.
              const unlistenOriginal = listen<string>("subtitle-original", (event) => {
                setSubtitles((prev) => {
                  const last = prev.length > 0 ? prev[prev.length - 1] : null;

                  // If last entry is not final (still intermediate), update it
                  if (last && !last.isFinal) {
                    const updated = [...prev];
                    updated[updated.length - 1] = {
                      ...last,
                      original: event.payload,
                    };
                    return updated;
                  }

                  // Otherwise, create a new entry
                  const now = new Date();
                  const timestamp = now.toLocaleTimeString("zh-CN", { hour12: false });
                  return [
                    ...prev,
                    {
                      id: idCounter.current++,
                      timestamp,
                      original: event.payload,
                      isFinal: false,
                    },
                  ];
                });
    });

                const unlistenTranslated = listen<string>(
                  "subtitle-translated",
      (event) => {
                    // Translation comes after a final result, mark last entry as final and attach translation
                    setSubtitles((prev) => {
                      if (prev.length === 0) return prev;
                      const updated = [...prev];
                      updated[updated.length - 1] = {
                        ...updated[updated.length - 1],
                        translated: event.payload,
                        isFinal: true,
                      };
                      return updated;
                    });
      }
                  );

                  const unlistenError = listen<string>("subtitle-error", (event) => {
                    setError(event.payload);
      setTimeout(() => setError(null), 5000);
    });

    return () => {
                      unlistenOriginal.then((fn) => fn());
      unlistenTranslated.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, []);

  // Also: when ASR sends a final result without translation, mark it final
  // We handle this by checking: if backend doesn't have translation enabled,
  // the entry stays as !isFinal from ASR perspective.
  // But for display, intermediate vs final doesn't matter much - we keep streaming text.

  // Auto-scroll to bottom
  // Load audio app list
  useEffect(() => {
    if (listRef.current) {
                      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [subtitles]);

  const handleToggleRecord = async () => {
    try {
      if (isRecording) {
                      await invoke("stop_capture");
                    setIsRecording(false);
        // Mark last entry as final
        setSubtitles((prev) => {
          if (prev.length === 0) return prev;
                    const updated = [...prev];
                    updated[updated.length - 1] = {
                      ...updated[updated.length - 1],
                      isFinal: true,
          };
                    return updated;
        });
      } else {
        if (captureSource === "app" && !selectedAppPid) {
                      setError("请选择要录制的应用");
          setTimeout(() => setError(null), 5000);
                    return;
        }
                    await invoke("start_capture");        setIsRecording(true);
                    setSubtitles([]);
      }
    } catch (err) {
                      setError(String(err));
      setTimeout(() => setError(null), 5000);
    }
  };

  const handleToggleTranslation = async () => {
    const newVal = !translationEnabled;
                    if (newVal) {
      // Trying to enable, check config completeness first
      try {
        const result = await invoke<{ ready: boolean; message: string }>(
                    "check_translation_config"
                    );
                    if (!result.ready) {
                      setError(result.message);
          setTimeout(() => setError(null), 5000);
                    return; // Don't enable
        }
      } catch (err) {
                      console.error("Failed to check translation config:", err);
                    return;
      }
    }
                    setTranslationEnabled(newVal);
                    try {
                      await invoke("set_translation_enabled", { enabled: newVal });
                    // Persist to config so the next recording respects the toggle
                    const cfg = await invoke<Record<string, unknown>>("get_config");
                      const updatedCfg = {
                        ...cfg,
                        translation: {
                        ...(cfg.translation as Record<string, unknown>),
                        enabled: newVal,
        },
      };
                      await invoke("save_config", {config: updatedCfg });
    } catch (err) {
                        console.error("Failed to toggle translation:", err);
    }
  };

                      return (
                      <div className="subtitle-view-container">
                        {/* Sidebar */}
                        <aside className="subtitle-sidebar">
                          <div className="sidebar-header">
                            <div className="app-brand-large">灵幕</div>
                            <div className="app-version">v0.1.0</div>
                          </div>

                          <nav className="sidebar-nav">
                            <div className="nav-group">
                              <div className="nav-label">主要</div>
                              <button className="nav-item active">
                                <span className="nav-icon">🎙️</span>
                                  实时转写
                              </button>
                            </div>

                            <div className="nav-group">
                              <div className="nav-label">支持</div>
                              <button className="nav-item" onClick={() => window.open('https://github.com/Nex-Z/LingSubtitle', '_blank')}>
                                <span className="nav-icon">📖</span>
                                使用指南
                              </button>
                              <button className="nav-item" onClick={() => window.open('https://github.com/Nex-Z/LingSubtitle/issues', '_blank')}>
                                <span className="nav-icon">💬</span>
                                反馈建议
                              </button>
                            </div>
                          </nav>

                          <div className="sidebar-footer">
                            <div className="usage-card">
                              <div className="usage-title">DashScope ASR</div>
                              <div className="usage-status">已连接</div>
                              </div>
                            </div>
                        </aside>

                        {/* Main Content Area */}
                        <main className="subtitle-main">
                          {/* Unified Header */}
                          <div className="unified-header">
                            <div className="header-left">
                              <div className="status-indicator">
                                <div className={`status-dot ${isRecording ? "active" : ""}`} />
                                <span className="status-text">
                                  {isRecording ? formatTime(elapsedTime) : "系统就绪"}
                                </span>
                              </div>
                            </div>

                            {/* Drag Area */}
                            <div className="header-drag-area" />

                            <div className="header-right">
                              <button
                                className="btn-icon-tiny"
                                onClick={onOpenSettings}
                                title="设置"
                              >
                                ⚙️
                              </button>
                            </div>
                          </div>

                          {/* Subtitle List / Welcome Panel */}
                          <div className="subtitle-content-area" ref={listRef}>
                            {subtitles.length === 0 && !isRecording ? (
                              <div className="welcome-panel">
                                <div className="welcome-hero">
                                  <div className="hero-icon">🎙️</div>
                                    <h1>欢迎使用 灵幕</h1>
                                    <p>专业的实时语音转写与翻译工具，让沟通无国界</p>
                                  </div>

                                  <div className="quick-actions-grid">
                                    <div className="action-card" onClick={handleToggleRecord}>
                                      <div className="action-icon">🎤</div>
                                      <div className="action-info">
                                        <h3>开始录音</h3>
                                          <p>立即捕获系统音频并开启实时转写</p>
                                          </div>
                                      </div>
                                      <div className="action-card" onClick={onOpenSettings}>
                                        <div className="action-icon">🔧</div>
                                        <div className="action-info">
                                          <h3>配置服务</h3>
                                          <p>管理您的 阿里云 ASR 与 翻译 密钥</p>
                                        </div>
                                      </div>
                                      <div className="action-card" onClick={onToggleFloating}>
                                        <div className="action-icon">🖥️</div>
                                          <div className="action-info">
                                            <h3>悬浮窗口</h3>
                                            <p>在其他窗口之上显示实时字幕条</p>
                                          </div>
                                        </div>
                                      </div>

                                      <div className="recent-activity-placeholder">
                                        <div className="section-header">
                                          <span>最近活动</span>
                                            <button className="btn-text-only">查看全部</button>
                                        </div>
                                        <div className="empty-hint-box">
                                          暂无最近记录。开始一次录制后，这里将显示您的转写历史
                                        </div>
                                      </div>
                                    </div>
                                    ) : subtitles.length === 0 && isRecording ? (
                                    <div className="subtitle-empty">
                                      <div className="subtitle-empty-icon animate-pulse">🎙️</div>
                                        <div className="subtitle-empty-text">正在聆听您的声音...</div>
                                        <div className="subtitle-empty-hint">实时音频捕获已开启</div>
                                        </div>
                                        ) : (
                                        <div className="subtitle-list-inner">
                                          {subtitles.map((item) => (
                                            <div
                                              key={item.id}
                                              className={`subtitle-entry ${!item.isFinal ? "streaming" : ""}`}
                                            >
                                              <div className="subtitle-header">
                                                <span className="subtitle-time">{item.timestamp}</span>
                                                {!item.isFinal && <span className="streaming-indicator">进行中</span>}
                                                </div>
                  {translationEnabled ? (
                                                  /* Translation ON: translated text is primary */
                                                  <div className="subtitle-split-content">
                                                    <div className="subtitle-col translated">
                                                      <div className="subtitle-label">译文</div>
                                                      <div className="subtitle-text">
                                                        {item.translated || (item.isFinal ? "翻译中..." : item.original)}
                                                      </div>
                                                    </div>
                                                    {item.original && (
                                                      <>
                                                        <div className="subtitle-col-divider" />
                                                        <div className="subtitle-col original ref-text">
                                                          <div className="subtitle-label">原文</div>
                                                          <div className="subtitle-text">{item.original}</div>
                                                        </div>
                                                      </>
                                                    )}
                                                  </div>
                                                ) : (
                                                  /* Translation OFF: original only */
                                                  <div className="subtitle-split-content">
                                                    <div className="subtitle-col original">
                                                      <div className="subtitle-label">原文</div>
                                                      <div className="subtitle-text">{item.original}</div>
                                                    </div>
                                                  </div>
                                                )}
                                              </div>
              ))}
                                              <div className="list-padding-bottom" />
                                            </div>
                                          )}
                                        </div>

                                        {/* Central Floating Control Bar */}
                                        <div className="central-control-dock">
                                          <div className="dock-container">
                                            <div className="dock-group">
                                              <button
                                                className={`btn-dock-action ${isFloatingOpen ? 'active' : ''}`}
                                                onClick={onToggleFloating}
                                                title={isFloatingOpen ? "关闭悬浮窗" : "开启悬浮窗"}
              >
                                              <span className="dock-icon">📺</span>
                                              <span className="dock-label">悬浮窗</span>
                                              </button>
                                            </div>

                                            <div className="dock-divider" />

                                            {/* Capture Source Selector */}
                                            <div className="dock-group">
                                              <DockDropdown
                                                label="音频源"
                                                options={captureSources}
                                                value={captureSource}
                                                onChange={handleCaptureSourceChange}
                                                disabled={isRecording}
                                              />
                                            </div>

                                            {captureSource === "app" && (
                                              <>
                                                <div className="dock-divider" />
                                                <div className="dock-group">
                                                  <DockDropdown
                                                    label="应用"
                                                    options={audioAppOptions}
                                                    value={selectedAppValue}
                                                    onChange={handleCaptureAppChange}
                                                    disabled={isRecording || audioApps.length === 0}
                                                  />
                                                  <button
                                                    className="dock-refresh-btn"
                                                    onClick={refreshAudioApps}
                                                    title="刷新应用列表"
                                                    disabled={isRecording}
                                                  >
                                                    R
                                                  </button>
                                                </div>
                                              </>
                                            )}

                                            <div className="dock-divider" />

                                            {/* ASR Language Selector */}
                                            <div className="dock-group">
                                              <DockDropdown
                                                label="🎤 识别"
                                                options={asrLanguages}
                                                value={asrLanguage}
                                                onChange={handleAsrLanguageChange}
                                                disabled={isRecording}
                                              />
                                            </div>

                                            <div className="dock-divider" />

                                            <button
                                              className={`btn-record-main ${isRecording ? "recording" : "idle"}`}
                                              onClick={handleToggleRecord}
                                            >
                                              <div className="record-pulse-ring" />
                                              <div className="record-inner-icon" />
                                              <span className="record-main-label">{isRecording ? "停止录制" : "开始录音"}</span>
            </button>

                                            <div className="dock-divider" />

                                            {/* Translation Target Language Selector */}
                                            <div className="dock-group">
                                              <DockDropdown
                                                label="🌐 译为"
                                                options={translationLanguages}
                                                value={targetLanguage}
                                                onChange={handleTargetLanguageChange}
                                                disabled={!translationEnabled}
                                              />
                                            </div>

                                            <div className="dock-divider" />

                                            <div className="dock-group">
                                              <div className="dock-toggle-item">
                                                <span className="dock-label">翻译</span>
                                                <div className="segmented-toggle">
                                                  <button
                                                    className={`segmented-btn ${!translationEnabled ? "active" : ""}`}
                                                    onClick={() => translationEnabled && handleToggleTranslation()}
                                                  >
                                                    关
                                                  </button>
                                                  <button
                                                    className={`segmented-btn ${translationEnabled ? "active" : ""}`}
                                                    onClick={() => !translationEnabled && handleToggleTranslation()}
                                                  >
                                                    开
                                                  </button>
                                                </div>
                                              </div>
                                            </div>
                                          </div>
                                        </div>
                                      </main>

                                      {/* Error Toast */}
                                      {error && <div className="error-toast">⚠️ {error}</div>}
                                    </div>

                                    );
}











