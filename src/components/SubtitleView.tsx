import { useEffect, useRef, useState, type ReactNode } from "react";
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

type SubtitleItem = SubtitleSegmentPayload;

interface SubtitleViewProps {
  onOpenSettings: () => void;
  onToggleFloating: () => void;
  isFloatingOpen: boolean;
}

interface AudioApp {
  pid: number;
  name: string;
  iconDataUrl?: string | null;
}

interface AppConfigResponse {
  asr: { language: string };
  translation: { enabled: boolean; target_language: string };
  capture?: { source?: string; app_pid?: number };
}

function SvgIcon({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.9"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

function HomeIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M3 10.5L12 3l9 7.5" />
      <path d="M5 9.5V21h14V9.5" />
      <path d="M9 21v-6h6v6" />
    </SvgIcon>
  );
}

function BookIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M4 5.5A2.5 2.5 0 0 1 6.5 3H20v17H6.5A2.5 2.5 0 0 0 4 22Z" />
      <path d="M8 7h8" />
      <path d="M8 11h8" />
    </SvgIcon>
  );
}

function ChatIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M7 18l-4 3V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H7Z" />
      <path d="M8 8h8" />
      <path d="M8 12h5" />
    </SvgIcon>
  );
}

function MicIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0" />
      <path d="M12 18v3" />
      <path d="M8 21h8" />
    </SvgIcon>
  );
}

function LayersIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M12 4l8 4-8 4-8-4 8-4Z" />
      <path d="M4 12l8 4 8-4" />
      <path d="M4 16l8 4 8-4" />
    </SvgIcon>
  );
}

function SettingsIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M12 3l1.2 2.6 2.9.4-.9 2.8 2 2-2 2 .9 2.8-2.9.4L12 21l-1.2-2.6-2.9-.4.9-2.8-2-2 2-2-.9-2.8 2.9-.4L12 3Z" />
      <circle cx="12" cy="12" r="3" />
    </SvgIcon>
  );
}

function ArrowIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M6 9l6 6 6-6" />
    </SvgIcon>
  );
}

function CheckIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M5 12l4 4L19 6" />
    </SvgIcon>
  );
}

function RefreshIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M20 11a8 8 0 1 0 2 5.3" />
      <path d="M20 4v7h-7" />
    </SvgIcon>
  );
}

function DockDropdown({
  label,
  options,
  value,
  onChange,
  disabled,
}: {
  label: string;
  options: { value: string; short: string; full: string; iconDataUrl?: string | null }[];
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
    if (open) document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [open]);

  const selected = options.find((item) => item.value === value);

  return (
    <div className={`dock-dropdown ${disabled ? "disabled" : ""}`} ref={ref}>
      <button
        type="button"
        className="dock-dropdown-trigger"
        onClick={() => !disabled && setOpen((prev) => !prev)}
        disabled={disabled}
      >
        {selected?.iconDataUrl ? (
          <img className="dock-option-icon trigger" src={selected.iconDataUrl} alt="" />
        ) : null}
        <span className="dock-dropdown-label">{label}</span>
        <span className="dock-dropdown-value">{selected?.short || value}</span>
        <ArrowIcon className={`dock-dropdown-arrow ${open ? "open" : ""}`} />
      </button>
      {open && (
        <div className="dock-dropdown-menu">
          {options.map((option) => (
            <button
              type="button"
              key={option.value}
              className={`dock-dropdown-option ${option.value === value ? "active" : ""}`}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
            >
              {option.iconDataUrl ? (
                <img className="dock-option-icon" src={option.iconDataUrl} alt="" />
              ) : (
                <span className="dock-option-fallback" aria-hidden="true">
                  {option.short.slice(0, 1)}
                </span>
              )}
              <span className="option-label">{option.full}</span>
              {option.value === value && <CheckIcon className="option-check" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

const asrLanguages = [
  { value: "auto", short: "自动", full: "自动识别" },
  { value: "zh", short: "中文", full: "中文" },
  { value: "en", short: "英语", full: "英语 (English)" },
  { value: "ja", short: "日语", full: "日语 (日本语)" },
  { value: "ko", short: "韩语", full: "韩语 (한국어)" },
  { value: "de", short: "德语", full: "德语 (Deutsch)" },
  { value: "fr", short: "法语", full: "法语 (Francais)" },
  { value: "es", short: "西语", full: "西班牙语 (Espanol)" },
];

const translationLanguages = [
  { value: "中文", short: "中文", full: "中文" },
  { value: "English", short: "英语", full: "英语 (English)" },
  { value: "日本語", short: "日语", full: "日语 (日本语)" },
  { value: "한국어", short: "韩语", full: "韩语 (한국어)" },
  { value: "Deutsch", short: "德语", full: "德语 (Deutsch)" },
  { value: "Francais", short: "法语", full: "法语 (Francais)" },
  { value: "Espanol", short: "西语", full: "西班牙语 (Espanol)" },
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
  if (typeof payload === "string") return payload;
  const scopeMap: Record<string, string> = {
    translation: "翻译",
    asr: "识别",
    capture: "录制",
    system: "系统",
  };
  const scope = scopeMap[payload.scope] || "系统";
  const suffix = payload.errorKind ? ` (${payload.errorKind})` : "";
  return `${scope}错误${suffix}: ${payload.message}`;
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
    return { mainLabel: "原文", mainText: item.originalText || "正在识别...", subLabel: "", subText: "", showSub: false };
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
    return { mainLabel: "原文", mainText: item.originalText || "正在识别...", subLabel: "", subText: "", showSub: false };
  }
  if (item.translatedText || canShowDraftAsMain(item)) {
    return { mainLabel: "译文", mainText: translated, subLabel: "原文", subText: item.originalText, showSub: true };
  }
  if (translationStatus === "streaming") {
    return { mainLabel: "原文", mainText: item.originalText, subLabel: "译文", subText: "翻译生成中...", showSub: true };
  }
  return { mainLabel: "原文", mainText: item.originalText, subLabel: "译文", subText: "等待翻译...", showSub: true };
}

function formatTime(seconds: number) {
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
          short: app.name,
          full: `${app.name} (PID ${app.pid})`,
          iconDataUrl: app.iconDataUrl,
        }))
      : [{ value: "", short: "无应用", full: "暂无可录制的应用" }];

  const selectedAppValue = selectedAppPid ? String(selectedAppPid) : "";
  const latestSubtitle = subtitles[subtitles.length - 1];
  const isWelcomeState = subtitles.length === 0;
  const headerStatusText = isRecording ? `录制中 ${formatTime(elapsedTime)}` : "准备就绪";

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
    if (listRef.current) listRef.current.scrollTop = listRef.current.scrollHeight;
  }, [subtitles]);

  const saveCaptureConfig = async (source: "system" | "app", pid: number | null, name: string) => {
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
    if (captureSource === "app") refreshAudioApps();
  }, [captureSource]);

  const handleAsrLanguageChange = async (language: string) => {
    setAsrLanguage(language);
    try {
      const cfg = await invoke<Record<string, unknown>>("get_config");
      await invoke("save_config", {
        config: {
          ...cfg,
          asr: { ...(cfg.asr as Record<string, unknown>), language },
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
          translation: { ...(cfg.translation as Record<string, unknown>), target_language: language },
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
          translation: { ...(cfg.translation as Record<string, unknown>), enabled: next },
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
            <div className="nav-label">工作台</div>
            <button type="button" className="nav-item active">
              <HomeIcon className="nav-icon" />
              实时转写
            </button>
          </div>

          <div className="nav-group">
            <div className="nav-label">帮助</div>
            <button
              type="button"
              className="nav-item"
              onClick={() => window.open("https://github.com/Nex-Z/LingSubtitle", "_blank")}
            >
              <BookIcon className="nav-icon" />
              使用指南
            </button>
            <button
              type="button"
              className="nav-item"
              onClick={() => window.open("https://github.com/Nex-Z/LingSubtitle/issues", "_blank")}
            >
              <ChatIcon className="nav-icon" />
              反馈建议
            </button>
          </div>
        </nav>

        <div className="sidebar-footer">
          <div className="usage-card">
            <div className="usage-title">服务状态</div>
            <div className="usage-status">DashScope ASR 已连接</div>
          </div>
        </div>
      </aside>

      <main className="subtitle-main">
        <div className="unified-header">
          <div className="header-left">
            <div className="status-indicator">
              <div className={`status-dot ${isRecording ? "active" : ""}`} />
              <span className="status-text">{headerStatusText}</span>
            </div>
          </div>

          <div className="header-drag-area" />

          <div className="header-right">
            <button type="button" className="btn-icon-tiny" onClick={onOpenSettings} title="设置">
              <SettingsIcon className="btn-icon-svg" />
            </button>
          </div>
        </div>

        <div
          className={`subtitle-content-area ${isWelcomeState ? "welcome-mode" : "stream-mode"}`}
          ref={listRef}
        >
          <div className="content-backdrop" />

          {subtitles.length === 0 && !isRecording ? (
            <div className="welcome-panel">
              <div className="welcome-hero">
                <h1>实时字幕、翻译和悬浮窗，放在一个更轻的工作流里</h1>
                <p>开始录制后，这里会持续显示最新字幕。更多参数放到底部控制台和设置页，不再重复堆叠。</p>
              </div>

              <div className="quick-actions-grid">
                <button type="button" className="action-card primary" onClick={handleToggleRecord}>
                  <div className="action-icon">
                    <MicIcon className="action-icon-svg" />
                  </div>
                  <div className="action-info">
                    <h3>开始录制</h3>
                    <p>立即捕获当前音频并开启实时字幕流。</p>
                  </div>
                </button>
                <button type="button" className="action-card" onClick={onOpenSettings}>
                  <div className="action-icon">
                    <SettingsIcon className="action-icon-svg" />
                  </div>
                  <div className="action-info">
                    <h3>配置服务</h3>
                    <p>管理识别、翻译与保存设置。</p>
                  </div>
                </button>
                <button type="button" className="action-card" onClick={onToggleFloating}>
                  <div className="action-icon">
                    <LayersIcon className="action-icon-svg" />
                  </div>
                  <div className="action-info">
                    <h3>{isFloatingOpen ? "关闭悬浮窗" : "打开悬浮窗"}</h3>
                    <p>在其他窗口上方显示实时字幕条。</p>
                  </div>
                </button>
              </div>
            </div>
          ) : subtitles.length === 0 ? (
            <div className="subtitle-empty">
              <div className="subtitle-empty-icon animate-pulse">
                <MicIcon className="empty-state-icon" />
              </div>
              <div className="subtitle-empty-text">正在监听当前音频...</div>
              <div className="subtitle-empty-hint">首条字幕出现后会自动滚动到最新内容。</div>
            </div>
          ) : (
            <div className="subtitle-list-shell">
              <div className="subtitle-stream-overview">
                <div>
                  <div className="subtitle-stream-title">实时字幕流</div>
                  <div className="subtitle-stream-desc">录制过程中会自动追踪最新片段。</div>
                </div>
                {latestSubtitle && (
                  <div className="subtitle-stream-meta">
                    <span className="meta-chip">最新时间 {latestSubtitle.timestamp}</span>
                    <span className="meta-chip">{translationEnabled ? "双语显示" : "原文显示"}</span>
                  </div>
                )}
              </div>

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
                        <div className={`subtitle-col ${display.mainLabel === "译文" ? "translated" : "original"}`}>
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
            </div>
          )}
        </div>

        <div className="central-control-dock">
          <div className="dock-container">
            <div className="dock-group">
              <button
                type="button"
                className={`btn-dock-action ${isFloatingOpen ? "active" : ""}`}
                onClick={onToggleFloating}
                title={isFloatingOpen ? "关闭悬浮窗" : "开启悬浮窗"}
              >
                <LayersIcon className="dock-icon" />
                <span className="dock-label">悬浮窗</span>
              </button>
            </div>

            <div className="dock-divider" />

            <div className="dock-group">
              <DockDropdown
                label="音频来源"
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
                    type="button"
                    className="dock-refresh-btn"
                    onClick={refreshAudioApps}
                    title="刷新应用列表"
                    disabled={isRecording}
                  >
                    <RefreshIcon className="dock-refresh-icon" />
                  </button>
                </div>
              </>
            )}

            <div className="dock-divider" />

            <div className="dock-group">
              <DockDropdown
                label="识别语言"
                options={asrLanguages}
                value={asrLanguage}
                onChange={handleAsrLanguageChange}
                disabled={isRecording}
              />
            </div>

            <div className="dock-divider" />

            <button
              type="button"
              className={`btn-record-main ${isRecording ? "recording" : "idle"}`}
              onClick={handleToggleRecord}
            >
              <div className="record-pulse-ring" />
              <div className="record-inner-icon" />
              <span className="record-main-label">{isRecording ? "停止录制" : "开始录制"}</span>
            </button>

            <div className="dock-divider" />

            <div className="dock-group">
              <DockDropdown
                label="翻译目标"
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
                    type="button"
                    className={`segmented-btn ${!translationEnabled ? "active" : ""}`}
                    onClick={() => translationEnabled && handleToggleTranslation()}
                  >
                    关
                  </button>
                  <button
                    type="button"
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

      {error && <div className="error-toast">提示: {error}</div>}
    </div>
  );
}
