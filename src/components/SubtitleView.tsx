import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./SubtitleView.css";
import type {
  SubtitleErrorPayload,
  SubtitleSegmentPayload,
  SubtitleTranslationDeltaPayload,
  SubtitleTranslationFailedPayload,
  SubtitleTranslationPayload,
  SubtitleTranslationStartedPayload,
  TranslationStatus,
} from "../types/subtitle";

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

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    if (open) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [open]);

  const selected = options.find((item) => item.value === value);

  return (
    <div className={`dock-dropdown ${disabled ? "disabled" : ""}`} ref={ref}>
      <button
        className="dock-dropdown-trigger"
        onClick={() => !disabled && setOpen((prev) => !prev)}
        disabled={disabled}
      >
        <span className="dock-dropdown-label">{label}</span>
        <span className="dock-dropdown-value">{selected?.short || value}</span>
        <span className="dock-dropdown-arrow">{open ? "▴" : "▾"}</span>
      </button>
      {open && (
        <div className="dock-dropdown-menu">
          {options.map((option) => (
            <div
              key={option.value}
              className={`dock-dropdown-option ${option.value === value ? "active" : ""}`}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
            >
              <span className="option-label">{option.full}</span>
              {option.value === value && <span className="option-check">✓</span>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

type SubtitleItem = SubtitleSegmentPayload;

interface SubtitleViewProps {
  onOpenSettings: () => void;
  onToggleFloating: () => void;
  isFloatingOpen: boolean;
}

interface AudioApp {
  pid: number;
  name: string;
}

interface AppConfigResponse {
  asr: { language: string };
  translation: { enabled: boolean; target_language: string };
  capture?: { source?: string; app_pid?: number };
}

const asrLanguages = [
  { value: "auto", short: "自动", full: "自动识别" },
  { value: "zh", short: "中文", full: "中文" },
  { value: "en", short: "英语", full: "英语 (English)" },
  { value: "ja", short: "日语", full: "日语 (日本語)" },
  { value: "ko", short: "韩语", full: "韩语 (한국어)" },
  { value: "de", short: "德语", full: "德语 (Deutsch)" },
  { value: "fr", short: "法语", full: "法语 (Français)" },
  { value: "es", short: "西班牙语", full: "西班牙语 (Español)" },
];

const translationLanguages = [
  { value: "中文", short: "中文", full: "中文" },
  { value: "English", short: "英语", full: "英语 (English)" },
  { value: "日本語", short: "日语", full: "日语 (日本語)" },
  { value: "한국어", short: "韩语", full: "韩语 (한국어)" },
  { value: "Deutsch", short: "德语", full: "德语 (Deutsch)" },
  { value: "Français", short: "法语", full: "法语 (Français)" },
  { value: "Español", short: "西班牙语", full: "西班牙语 (Español)" },
];

const captureSources = [
  { value: "system", short: "系统", full: "系统音频" },
  { value: "app", short: "应用", full: "指定应用" },
];

const DRAFT_SWITCH_CHARS = 6;

function upsertSubtitle(items: SubtitleItem[], payload: SubtitleSegmentPayload): SubtitleItem[] {
  const index = items.findIndex((item) => item.segmentId === payload.segmentId);
  if (index === -1) {
    return [...items, payload].sort((left, right) => left.segmentId - right.segmentId);
  }

  const next = [...items];
  next[index] = { ...next[index], ...payload };
  return next;
}

function updateSubtitle(
  items: SubtitleItem[],
  segmentId: number,
  revision: number,
  updater: (item: SubtitleItem) => SubtitleItem
): SubtitleItem[] {
  return items.map((item) =>
    item.segmentId === segmentId && item.revision === revision ? updater(item) : item
  );
}

function translationReadyText(item: SubtitleItem): string {
  return item.translatedText || item.translatedDraftText || "";
}

function canShowDraftAsMain(item: SubtitleItem): boolean {
  return !item.translatedText && (item.translatedDraftText || "").length >= DRAFT_SWITCH_CHARS;
}

function formatError(payload: SubtitleErrorPayload | string): string {
  if (typeof payload === "string") {
    return payload;
  }
  const scopeMap: Record<string, string> = {
    translation: "翻译",
    asr: "识别",
    capture: "录制",
    system: "系统",
  };
  const scope = scopeMap[payload.scope] || "系统";
  const suffix = payload.errorKind ? ` (${payload.errorKind})` : "";
  return `${scope}错误${suffix}：${payload.message}`;
}

function getDisplayMode(
  item: SubtitleItem,
  translationEnabled: boolean
): {
  mainLabel: string;
  mainText: string;
  subLabel: string;
  subText: string;
  showSub: boolean;
} {
  const translated = translationReadyText(item);
  const translationStatus: TranslationStatus = item.translationStatus || "idle";

  if (!translationEnabled) {
    return {
      mainLabel: "原文",
      mainText: item.originalText || "正在识别...",
      subLabel: "",
      subText: "",
      showSub: false,
    };
  }

  if (item.translationError || translationStatus === "failed") {
    return {
      mainLabel: "原文",
      mainText: item.originalText || "翻译失败，已保留原文",
      subLabel: "状态",
      subText: "翻译失败，已保留原文",
      showSub: true,
    };
  }

  if (item.state === "streaming") {
    return {
      mainLabel: "原文",
      mainText: item.originalText || "正在识别...",
      subLabel: "",
      subText: "",
      showSub: false,
    };
  }

  if (item.translatedText || canShowDraftAsMain(item)) {
    return {
      mainLabel: "译文",
      mainText: translated,
      subLabel: "原文",
      subText: item.originalText,
      showSub: true,
    };
  }

  if (translationStatus === "streaming") {
    return {
      mainLabel: "原文",
      mainText: item.originalText,
      subLabel: "译文",
      subText: "翻译生成中...",
      showSub: true,
    };
  }

  return {
    mainLabel: "原文",
    mainText: item.originalText,
    subLabel: "译文",
    subText: "等待翻译...",
    showSub: true,
  };
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

  const audioAppOptions =
    audioApps.length > 0
      ? audioApps.map((app) => ({
          value: String(app.pid),
          short: `${app.name} (${app.pid})`,
          full: `${app.name} (PID ${app.pid})`,
        }))
      : [{ value: "", short: "无", full: "无可记录的音频" }];

  const selectedAppValue = selectedAppPid ? String(selectedAppPid) : "";

  useEffect(() => {
    let interval: number | undefined;
    if (isRecording) {
      setElapsedTime(0);
      interval = window.setInterval(() => setElapsedTime((prev) => prev + 1), 1000);
    }
    return () => {
      if (interval) clearInterval(interval);
    };
  }, [isRecording]);

  useEffect(() => {
    invoke<AppConfigResponse>("get_config")
      .then((cfg) => {
        setTranslationEnabled(cfg.translation.enabled);
        setAsrLanguage(cfg.asr.language || "auto");
        setTargetLanguage(cfg.translation.target_language || "中文");
        setCaptureSource(cfg.capture?.source === "app" ? "app" : "system");
        setSelectedAppPid(cfg.capture?.app_pid ?? null);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    invoke<boolean>("get_capture_status").then(setIsRecording).catch(() => {});

    const unlistenUpsert = listen<SubtitleSegmentPayload>("subtitle-segment-upsert", (event) => {
      setSubtitles((prev) => upsertSubtitle(prev, event.payload));
    });

    const unlistenStarted = listen<SubtitleTranslationStartedPayload>(
      "subtitle-segment-translation-started",
      (event) => {
        setSubtitles((prev) =>
          updateSubtitle(prev, event.payload.segmentId, event.payload.revision, (item) => ({
            ...item,
            translationStatus: "streaming",
            translatedDraftText: item.translatedDraftText ?? "",
            translationError: false,
          }))
        );
      }
    );

    const unlistenDelta = listen<SubtitleTranslationDeltaPayload>(
      "subtitle-segment-translation-delta",
      (event) => {
        setSubtitles((prev) =>
          updateSubtitle(prev, event.payload.segmentId, event.payload.revision, (item) => ({
            ...item,
            translationStatus: "streaming",
            translatedDraftText: event.payload.accumulatedText,
            translationError: false,
          }))
        );
      }
    );

    const unlistenFinished = listen<SubtitleTranslationPayload>(
      "subtitle-segment-translation-finished",
      (event) => {
        setSubtitles((prev) =>
          updateSubtitle(prev, event.payload.segmentId, event.payload.revision, (item) => ({
            ...item,
            translatedText: event.payload.translatedText,
            translatedDraftText: null,
            translationStatus: "completed",
            translationError: false,
          }))
        );
      }
    );

    const unlistenFailed = listen<SubtitleTranslationFailedPayload>(
      "subtitle-segment-translation-failed",
      (event) => {
        setSubtitles((prev) =>
          updateSubtitle(prev, event.payload.segmentId, event.payload.revision, (item) => ({
            ...item,
            translationStatus: "failed",
            translatedDraftText: null,
            translationError: true,
          }))
        );
      }
    );

    const unlistenError = listen<SubtitleErrorPayload>("subtitle-error", (event) => {
      setError(formatError(event.payload));
      window.setTimeout(() => setError(null), 5000);
    });

    return () => {
      unlistenUpsert.then((fn) => fn());
      unlistenStarted.then((fn) => fn());
      unlistenDelta.then((fn) => fn());
      unlistenFinished.then((fn) => fn());
      unlistenFailed.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [subtitles]);

  const refreshAudioApps = async () => {
    try {
      const apps = await invoke<AudioApp[]>("list_audio_apps");
      setAudioApps(apps);
      if (captureSource === "app" && apps.length > 0) {
        const exists = selectedAppPid ? apps.some((item) => item.pid === selectedAppPid) : false;
        if (!exists) {
          const first = apps[0];
          setSelectedAppPid(first.pid);
          await saveCaptureConfig("app", first.pid, first.name);
        }
      }
    } catch (err) {
      console.error("Failed to load audio apps:", err);
    }
  };

  useEffect(() => {
    refreshAudioApps();
  }, []);

  useEffect(() => {
    if (captureSource === "app") {
      refreshAudioApps();
    }
  }, [captureSource]);

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

  const saveCaptureConfig = async (
    source: "system" | "app",
    pid: number | null,
    name: string
  ) => {
    try {
      const cfg = await invoke<Record<string, unknown>>("get_config");
      await invoke("save_config", {
        config: {
          ...cfg,
          capture: {
            ...((cfg.capture as Record<string, unknown>) || {}),
            source,
            app_pid: pid,
            app_name: name,
          },
        },
      });
    } catch (err) {
      console.error("Failed to save capture config:", err);
    }
  };

  const handleAsrLanguageChange = async (language: string) => {
    setAsrLanguage(language);
    try {
      const cfg = await invoke<Record<string, unknown>>("get_config");
      await invoke("save_config", {
        config: {
          ...cfg,
          asr: {
            ...(cfg.asr as Record<string, unknown>),
            language,
          },
        },
      });
    } catch (err) {
      console.error("Failed to save ASR language:", err);
    }
  };

  const handleTargetLanguageChange = async (language: string) => {
    setTargetLanguage(language);
    try {
      const cfg = await invoke<Record<string, unknown>>("get_config");
      await invoke("save_config", {
        config: {
          ...cfg,
          translation: {
            ...(cfg.translation as Record<string, unknown>),
            target_language: language,
          },
        },
      });
    } catch (err) {
      console.error("Failed to save target language:", err);
    }
  };

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

  const handleToggleRecord = async () => {
    try {
      if (isRecording) {
        await invoke("stop_capture");
        setIsRecording(false);
        return;
      }

      if (captureSource === "app" && !selectedAppPid) {
        setError("请选择要录制的应用");
        window.setTimeout(() => setError(null), 5000);
        return;
      }

      await invoke("start_capture");
      setSubtitles([]);
      setIsRecording(true);
    } catch (err) {
      setError(String(err));
      window.setTimeout(() => setError(null), 5000);
    }
  };

  const handleToggleTranslation = async () => {
    const next = !translationEnabled;
    if (next) {
      try {
        const result = await invoke<{ ready: boolean; message: string }>("check_translation_config");
        if (!result.ready) {
          setError(result.message);
          window.setTimeout(() => setError(null), 5000);
          return;
        }
      } catch (err) {
        console.error("Failed to check translation config:", err);
        return;
      }
    }

    setTranslationEnabled(next);
    try {
      await invoke("set_translation_enabled", { enabled: next });
      const cfg = await invoke<Record<string, unknown>>("get_config");
      await invoke("save_config", {
        config: {
          ...cfg,
          translation: {
            ...(cfg.translation as Record<string, unknown>),
            enabled: next,
          },
        },
      });
    } catch (err) {
      console.error("Failed to toggle translation:", err);
    }
  };

  return (
    <div className="subtitle-view-container">
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
            <button
              className="nav-item"
              onClick={() => window.open("https://github.com/Nex-Z/LingSubtitle", "_blank")}
            >
              <span className="nav-icon">📖</span>
              使用指南
            </button>
            <button
              className="nav-item"
              onClick={() => window.open("https://github.com/Nex-Z/LingSubtitle/issues", "_blank")}
            >
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

      <main className="subtitle-main">
        <div className="unified-header">
          <div className="header-left">
            <div className="status-indicator">
              <div className={`status-dot ${isRecording ? "active" : ""}`} />
              <span className="status-text">{isRecording ? formatTime(elapsedTime) : "系统就绪"}</span>
            </div>
          </div>

          <div className="header-drag-area" />

          <div className="header-right">
            <button className="btn-icon-tiny" onClick={onOpenSettings} title="设置">
              ⚙️
            </button>
          </div>
        </div>

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
                    <p>管理您的阿里云 ASR 与翻译密钥</p>
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
                <div className="empty-hint-box">暂无最近记录。开始一次录制后，这里将显示您的转写历史</div>
              </div>
            </div>
          ) : subtitles.length === 0 ? (
            <div className="subtitle-empty">
              <div className="subtitle-empty-icon animate-pulse">🎙️</div>
              <div className="subtitle-empty-text">正在聆听您的声音...</div>
              <div className="subtitle-empty-hint">实时音频捕获已开启</div>
            </div>
          ) : (
            <div className="subtitle-list-inner">
              {subtitles.map((item) => {
                const display = getDisplayMode(item, translationEnabled);
                return (
                  <div
                    key={`${item.segmentId}-${item.revision}`}
                    className={`subtitle-entry ${item.state === "streaming" ? "streaming" : ""}`}
                  >
                    <div className="subtitle-header">
                      <span className="subtitle-time">{item.timestamp}</span>
                      {item.state === "streaming" && <span className="streaming-indicator">进行中</span>}
                    </div>

                    <div className="subtitle-split-content">
                      <div
                        className={`subtitle-col ${
                          display.mainLabel === "译文" ? "translated" : "original"
                        }`}
                      >
                        <div className="subtitle-label">{display.mainLabel}</div>
                        <div className="subtitle-text">{display.mainText}</div>
                      </div>
                      {display.showSub && (
                        <>
                          <div className="subtitle-col-divider" />
                          <div className="subtitle-col original ref-text">
                            <div className="subtitle-label">{display.subLabel}</div>
                            <div className="subtitle-text">{display.subText}</div>
                          </div>
                        </>
                      )}
                    </div>
                  </div>
                );
              })}
              <div className="list-padding-bottom" />
            </div>
          )}
        </div>

        <div className="central-control-dock">
          <div className="dock-container">
            <div className="dock-group">
              <button
                className={`btn-dock-action ${isFloatingOpen ? "active" : ""}`}
                onClick={onToggleFloating}
                title={isFloatingOpen ? "关闭悬浮窗" : "开启悬浮窗"}
              >
                <span className="dock-icon">📺</span>
                <span className="dock-label">悬浮窗</span>
              </button>
            </div>

            <div className="dock-divider" />

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

            <button className={`btn-record-main ${isRecording ? "recording" : "idle"}`} onClick={handleToggleRecord}>
              <div className="record-pulse-ring" />
              <div className="record-inner-icon" />
              <span className="record-main-label">{isRecording ? "停止录制" : "开始录音"}</span>
            </button>

            <div className="dock-divider" />

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

      {error && <div className="error-toast">⚠️ {error}</div>}
    </div>
  );
}
