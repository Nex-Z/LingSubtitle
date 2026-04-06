import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./SubtitleView.css";
import type {
  AppConfig,
  GummyCapabilities,
  GummyConfigCheckResult,
  GummyConnectivityResult,
  GummyLanguageOption,
} from "../types/gummy";
import type { SubtitleErrorPayload, SubtitleSegmentPayload, TranslationStatus } from "../types/subtitle";

type SubtitleItem = SubtitleSegmentPayload;
type ServiceState = {
  kind: "checking" | "notConfigured" | "ready" | "recording" | "error";
  message: string;
};

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

function SvgIcon({ children, className }: { children: ReactNode; className?: string }) {
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
  return <SvgIcon className={className}><path d="M3 10.5L12 3l9 7.5" /><path d="M5 9.5V21h14V9.5" /><path d="M9 21v-6h6v6" /></SvgIcon>;
}

function BookIcon({ className }: { className?: string }) {
  return <SvgIcon className={className}><path d="M4 5.5A2.5 2.5 0 0 1 6.5 3H20v17H6.5A2.5 2.5 0 0 0 4 22Z" /><path d="M8 7h8" /><path d="M8 11h8" /></SvgIcon>;
}

function ChatIcon({ className }: { className?: string }) {
  return <SvgIcon className={className}><path d="M7 18l-4 3V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H7Z" /><path d="M8 8h8" /><path d="M8 12h5" /></SvgIcon>;
}

function MicIcon({ className }: { className?: string }) {
  return <SvgIcon className={className}><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0" /><path d="M12 18v3" /><path d="M8 21h8" /></SvgIcon>;
}

function LayersIcon({ className }: { className?: string }) {
  return <SvgIcon className={className}><path d="M12 4l8 4-8 4-8-4 8-4Z" /><path d="M4 12l8 4 8-4" /><path d="M4 16l8 4 8-4" /></SvgIcon>;
}

function SettingsIcon({ className }: { className?: string }) {
  return <SvgIcon className={className}><path d="M12 3l1.2 2.6 2.9.4-.9 2.8 2 2-2 2 .9 2.8-2.9.4L12 21l-1.2-2.6-2.9-.4.9-2.8-2-2 2-2-.9-2.8 2.9-.4L12 3Z" /><circle cx="12" cy="12" r="3" /></SvgIcon>;
}

function ArrowIcon({ className }: { className?: string }) {
  return <SvgIcon className={className}><path d="M6 9l6 6 6-6" /></SvgIcon>;
}

function CheckIcon({ className }: { className?: string }) {
  return <SvgIcon className={className}><path d="M5 12l4 4L19 6" /></SvgIcon>;
}

function RefreshIcon({ className }: { className?: string }) {
  return <SvgIcon className={className}><path d="M20 11a8 8 0 1 0 2 5.3" /><path d="M20 4v7h-7" /></SvgIcon>;
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
      if (ref.current && !ref.current.contains(event.target as Node)) setOpen(false);
    };
    if (open) document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [open]);
  const selected = options.find((item) => item.value === value) || options[0];
  return (
    <div className={`dock-dropdown ${disabled ? "disabled" : ""}`} ref={ref}>
      <button type="button" className="dock-dropdown-trigger" onClick={() => !disabled && setOpen((prev) => !prev)} disabled={disabled}>
        {selected?.iconDataUrl ? <img className="dock-option-icon trigger" src={selected.iconDataUrl} alt="" /> : null}
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
                <span className="dock-option-fallback" aria-hidden="true">{option.short.slice(0, 1)}</span>
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

const captureSources = [
  { value: "system", short: "系统", full: "系统音频" },
  { value: "app", short: "应用", full: "指定应用" },
];

const DRAFT_SWITCH_CHARS = 6;

function upsertSubtitle(items: SubtitleItem[], payload: SubtitleSegmentPayload): SubtitleItem[] {
  if (items.length > 0 && items[0].sessionId !== payload.sessionId) return [payload];
  const index = items.findIndex((item) => item.segmentId === payload.segmentId);
  if (index === -1) return [...items, payload].sort((left, right) => left.segmentId - right.segmentId);
  const next = [...items];
  next[index] = { ...next[index], ...payload };
  return next;
}

function translationReadyText(item: SubtitleItem): string {
  return item.translatedText || item.translatedDraftText || "";
}

function canShowDraftAsMain(item: SubtitleItem): boolean {
  return !item.translatedText && (item.translatedDraftText || "").length >= DRAFT_SWITCH_CHARS;
}

function hasUsableTranslation(item: SubtitleItem): boolean {
  return Boolean(item.translatedText) || canShowDraftAsMain(item);
}

function formatError(payload: SubtitleErrorPayload | string): string {
  if (typeof payload === "string") return payload;
  const scopeMap: Record<string, string> = { translation: "翻译", asr: "识别", capture: "录制", system: "系统" };
  const scope = scopeMap[payload.scope] || "系统";
  const suffix = payload.errorKind ? ` (${payload.errorKind})` : "";
  return `${scope}错误${suffix}: ${payload.message}`;
}

function getDisplayMode(item: SubtitleItem, translationEnabled: boolean) {
  const translated = translationReadyText(item);
  const translationStatus: TranslationStatus = item.translationStatus || "idle";
  if (!translationEnabled) {
    return { mainLabel: "原文", mainText: item.originalText || "正在识别...", subLabel: "", subText: "", showSub: false };
  }
  if (item.translationError || translationStatus === "failed") {
    return { mainLabel: "原文", mainText: item.originalText || "译文缺失，已保留原文", subLabel: "状态", subText: "译文缺失，已保留原文", showSub: true };
  }
  if (hasUsableTranslation(item)) {
    return { mainLabel: "译文", mainText: translated, subLabel: "原文", subText: item.originalText, showSub: true };
  }
  if (item.state === "streaming") {
    return { mainLabel: "原文", mainText: item.originalText || "正在识别...", subLabel: "", subText: "", showSub: false };
  }
  if (translationStatus === "streaming") {
    return { mainLabel: "原文", mainText: item.originalText, subLabel: "译文", subText: "翻译生成中...", showSub: true };
  }
  return { mainLabel: "原文", mainText: item.originalText, subLabel: "译文", subText: "等待译文...", showSub: true };
}

function formatTime(seconds: number) {
  const hrs = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  return [hrs > 0 ? hrs.toString().padStart(2, "0") : null, mins.toString().padStart(2, "0"), secs.toString().padStart(2, "0")].filter(Boolean).join(":");
}

function shortLanguageLabel(item: GummyLanguageOption): string {
  return item.label.length > 4 ? item.label.slice(0, 4) : item.label;
}

function serviceStateText(state: ServiceState): string {
  switch (state.kind) {
    case "checking": return "Gummy 检测中";
    case "notConfigured": return "Gummy 未配置";
    case "ready": return "Gummy 已就绪";
    case "recording": return "Gummy 录制中";
    case "error": return "Gummy 任务失败";
    default: return "Gummy 状态未知";
  }
}

export default function SubtitleView({ onOpenSettings, onToggleFloating, isFloatingOpen }: SubtitleViewProps) {
  const [isRecording, setIsRecording] = useState(false);
  const [subtitles, setSubtitles] = useState<SubtitleItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [translationEnabled, setTranslationEnabled] = useState(false);
  const [elapsedTime, setElapsedTime] = useState(0);
  const [asrLanguage, setAsrLanguage] = useState("auto");
  const [targetLanguage, setTargetLanguage] = useState("en");
  const [captureSource, setCaptureSource] = useState<"system" | "app">("system");
  const [audioApps, setAudioApps] = useState<AudioApp[]>([]);
  const [selectedAppPid, setSelectedAppPid] = useState<number | null>(null);
  const [capabilities, setCapabilities] = useState<GummyCapabilities | null>(null);
  const [serviceState, setServiceState] = useState<ServiceState>({ kind: "checking", message: "正在检测 Gummy 服务状态..." });
  const listRef = useRef<HTMLDivElement>(null);

  const sourceLanguageOptions = useMemo(() => {
    if (!capabilities) return [{ value: "auto", short: "自动", full: "自动识别" }];
    return capabilities.sourceLanguages.map((item) => ({ value: item.code, short: shortLanguageLabel(item), full: item.label }));
  }, [capabilities]);

  const targetLanguageOptions = useMemo(() => {
    if (!capabilities) return [];
    return (capabilities.targetLanguagesBySource[asrLanguage] || []).map((item) => ({ value: item.code, short: shortLanguageLabel(item), full: item.label }));
  }, [capabilities, asrLanguage]);

  const audioAppOptions = audioApps.length > 0
    ? audioApps.map((app) => ({ value: String(app.pid), short: app.name, full: `${app.name} (PID ${app.pid})`, iconDataUrl: app.iconDataUrl }))
    : [{ value: "", short: "无应用", full: "暂无可录制的应用" }];

  const selectedAppValue = selectedAppPid ? String(selectedAppPid) : "";
  const latestSubtitle = subtitles[subtitles.length - 1];
  const isWelcomeState = subtitles.length === 0;
  const headerStatusText = isRecording ? `录制中 ${formatTime(elapsedTime)}` : "准备就绪";
  const translationBlockedByAuto = translationEnabled && asrLanguage === "auto";
  const targetLanguageUnsupported = translationEnabled && asrLanguage !== "auto" && !targetLanguageOptions.some((item) => item.value === targetLanguage);

  const persistConfig = async (updater: (config: AppConfig) => AppConfig) => {
    const cfg = await invoke<AppConfig>("get_config");
    const next = updater(cfg);
    await invoke("save_config", { config: next });
    return next;
  };

  const refreshServiceStatus = async (recording = isRecording) => {
    if (recording) {
      setServiceState({ kind: "recording", message: "Gummy 任务正在运行。" });
      return;
    }
    setServiceState({ kind: "checking", message: "正在检测 Gummy 服务状态..." });
    try {
      const configCheck = await invoke<GummyConfigCheckResult>("check_gummy_config");
      if (!configCheck.ready) {
        setServiceState({ kind: "notConfigured", message: configCheck.message });
        return;
      }
      const connectivity = await invoke<GummyConnectivityResult>("check_gummy_connectivity");
      setServiceState(connectivity.ok
        ? { kind: "ready", message: connectivity.message }
        : { kind: "error", message: connectivity.message });
    } catch (err) {
      setServiceState({ kind: "error", message: String(err) });
    }
  };

  const refreshAudioApps = async () => {
    try {
      const apps = await invoke<AudioApp[]>("list_audio_apps");
      setAudioApps(apps);
      if (captureSource !== "app") {
        return;
      }
      const exists = selectedAppPid ? apps.some((item) => item.pid === selectedAppPid) : false;
      if (apps.length === 0) {
        setSelectedAppPid(null);
        await persistConfig((cfg) => ({ ...cfg, capture: { ...cfg.capture, source: "app", app_pid: null, app_name: "" } }));
        return;
      }
      if (!exists) {
        const first = apps[0];
        setSelectedAppPid(first.pid);
        await persistConfig((cfg) => ({ ...cfg, capture: { ...cfg.capture, source: "app", app_pid: first.pid, app_name: first.name } }));
      }
    } catch (err) {
      console.error("Failed to load audio apps:", err);
    }
  };

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
    Promise.all([invoke<AppConfig>("get_config"), invoke<GummyCapabilities>("get_gummy_capabilities"), invoke<boolean>("get_capture_status")])
      .then(([cfg, gummyCapabilities, captureStatus]) => {
        setCapabilities(gummyCapabilities);
        setTranslationEnabled(cfg.translation.enabled);
        setAsrLanguage(cfg.asr.language || "auto");
        setTargetLanguage(cfg.translation.target_language || gummyCapabilities.defaults.target_language);
        setCaptureSource(cfg.capture?.source === "app" ? "app" : "system");
        setSelectedAppPid(cfg.capture?.app_pid ?? null);
        setIsRecording(captureStatus);
      })
      .catch((err) => {
        console.error("Failed to initialize subtitle view:", err);
        setServiceState({ kind: "error", message: String(err) });
      });
  }, []);

  useEffect(() => {
    if (!capabilities) return;
    refreshServiceStatus();
    refreshAudioApps();
  }, [capabilities]);

  useEffect(() => {
    if (captureSource === "app") {
      refreshAudioApps();
    }
  }, [captureSource]);

  useEffect(() => {
    const unlistenUpsert = listen<SubtitleSegmentPayload>("subtitle-segment-upsert", (event) => {
      setSubtitles((prev) => upsertSubtitle(prev, event.payload));
    });
    const unlistenError = listen<SubtitleErrorPayload>("subtitle-error", async (event) => {
      const nextError = formatError(event.payload);
      setError(nextError);
      window.setTimeout(() => setError(null), 5000);
      if (event.payload.scope === "asr" || event.payload.scope === "capture") {
        setServiceState({ kind: "error", message: event.payload.message });
        try {
          const captureStatus = await invoke<boolean>("get_capture_status");
          setIsRecording(captureStatus);
        } catch {
          setIsRecording(false);
        }
      }
    });
    return () => {
      unlistenUpsert.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (listRef.current) listRef.current.scrollTop = listRef.current.scrollHeight;
  }, [subtitles]);

  useEffect(() => {
    if (isRecording) setServiceState({ kind: "recording", message: "Gummy 任务正在运行。" });
  }, [isRecording]);

  useEffect(() => {
    if (!translationEnabled || !capabilities || asrLanguage === "auto") return;
    const supportedTargets = capabilities.targetLanguagesBySource[asrLanguage] || [];
    if (supportedTargets.length === 0) return;
    if (!supportedTargets.some((item) => item.code === targetLanguage)) {
      const nextTarget = supportedTargets[0].code;
      setTargetLanguage(nextTarget);
      persistConfig((cfg) => ({ ...cfg, translation: { ...cfg.translation, target_language: nextTarget } }))
        .catch((err) => console.error("Failed to sync target language:", err));
    }
  }, [asrLanguage, capabilities, targetLanguage, translationEnabled]);

  const handleAsrLanguageChange = async (language: string) => {
    setAsrLanguage(language);
    try {
      const next = await persistConfig((cfg) => ({ ...cfg, asr: { ...cfg.asr, language } }));
      setTranslationEnabled(next.translation.enabled);
      if (!isRecording) await refreshServiceStatus(false);
    } catch (err) {
      console.error("Failed to save ASR language:", err);
    }
  };

  const handleTargetLanguageChange = async (language: string) => {
    setTargetLanguage(language);
    try {
      await persistConfig((cfg) => ({ ...cfg, translation: { ...cfg.translation, target_language: language } }));
      if (!isRecording) await refreshServiceStatus(false);
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
    try {
      await persistConfig((cfg) => ({ ...cfg, capture: { ...cfg.capture, source: nextSource, app_pid: nextPid, app_name: nextName } }));
    } catch (err) {
      console.error("Failed to save capture source:", err);
    }
  };

  const handleCaptureAppChange = async (pidStr: string) => {
    const pid = pidStr ? Number(pidStr) : null;
    setSelectedAppPid(pid);
    const app = audioApps.find((item) => item.pid === pid);
    try {
      await persistConfig((cfg) => ({ ...cfg, capture: { ...cfg.capture, app_pid: pid, app_name: app?.name || "" } }));
    } catch (err) {
      console.error("Failed to save capture app:", err);
    }
  };

  const handleToggleRecord = async () => {
    try {
      if (isRecording) {
        await invoke("stop_capture");
        setIsRecording(false);
        await refreshServiceStatus(false);
        return;
      }
      if (captureSource === "app" && !selectedAppPid) {
        setError("请选择要录制的应用");
        window.setTimeout(() => setError(null), 5000);
        return;
      }
      if (translationBlockedByAuto) {
        setError("翻译开启时请先选择明确的识别语言，不能使用自动识别。");
        window.setTimeout(() => setError(null), 5000);
        return;
      }
      if (targetLanguageUnsupported) {
        setError("当前语言组合不在 Gummy 支持范围内，请调整源语言或目标语言。");
        window.setTimeout(() => setError(null), 5000);
        return;
      }
      const configCheck = await invoke<GummyConfigCheckResult>("check_gummy_config");
      if (!configCheck.ready) {
        setError(configCheck.message);
        window.setTimeout(() => setError(null), 5000);
        return;
      }
      await invoke("start_capture");
      setSubtitles([]);
      setIsRecording(true);
      setServiceState({ kind: "recording", message: "Gummy 任务正在运行。" });
    } catch (err) {
      const nextError = String(err);
      setError(nextError);
      setServiceState({ kind: "error", message: nextError });
      window.setTimeout(() => setError(null), 5000);
    }
  };

  const handleToggleTranslation = async () => {
    const next = !translationEnabled;
    setTranslationEnabled(next);
    try {
      await invoke("set_translation_enabled", { enabled: next });
      const nextConfig = await persistConfig((cfg) => ({ ...cfg, translation: { ...cfg.translation, enabled: next } }));
      if (next && nextConfig.asr.language === "auto") {
        setError("翻译已开启，请先把识别语言改成明确语种后再开始录制。");
        window.setTimeout(() => setError(null), 5000);
      }
      if (!isRecording) await refreshServiceStatus(false);
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
            <button type="button" className="nav-item" onClick={() => window.open("https://github.com/Nex-Z/LingSubtitle", "_blank")}>
              <BookIcon className="nav-icon" />
              使用指南
            </button>
            <button type="button" className="nav-item" onClick={() => window.open("https://github.com/Nex-Z/LingSubtitle/issues", "_blank")}>
              <ChatIcon className="nav-icon" />
              反馈建议
            </button>
          </div>
        </nav>
        <div className="sidebar-footer">
          <div className="usage-card">
            <div className="usage-title">服务状态</div>
            <div className="usage-status">{serviceStateText(serviceState)}</div>
            <div className="subtitle-stream-desc">{serviceState.message}</div>
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

        <div className={`subtitle-content-area ${isWelcomeState ? "welcome-mode" : "stream-mode"}`} ref={listRef}>
          <div className="content-backdrop" />
          {subtitles.length === 0 && !isRecording ? (
            <div className="welcome-panel">
              <div className="welcome-hero">
                <h1>Gummy 实时识别与翻译，现在走一条更短的双语链路</h1>
                <p>开始录制后，这里会持续显示最新字幕。语言组合与翻译可用性都按 Gummy 官方能力矩阵实时约束。</p>
              </div>
              <div className="quick-actions-grid">
                <button type="button" className="action-card primary" onClick={handleToggleRecord}>
                  <div className="action-icon"><MicIcon className="action-icon-svg" /></div>
                  <div className="action-info"><h3>开始录制</h3><p>立即捕获当前音频并开启 Gummy 实时双语任务。</p></div>
                </button>
                <button type="button" className="action-card" onClick={onOpenSettings}>
                  <div className="action-icon"><SettingsIcon className="action-icon-svg" /></div>
                  <div className="action-info"><h3>配置服务</h3><p>管理 Gummy 连接、VAD、翻译与保存设置。</p></div>
                </button>
                <button type="button" className="action-card" onClick={onToggleFloating}>
                  <div className="action-icon"><LayersIcon className="action-icon-svg" /></div>
                  <div className="action-info"><h3>{isFloatingOpen ? "关闭悬浮窗" : "打开悬浮窗"}</h3><p>在其他窗口上方显示实时字幕条。</p></div>
                </button>
              </div>
            </div>
          ) : subtitles.length === 0 ? (
            <div className="subtitle-empty">
              <div className="subtitle-empty-icon animate-pulse"><MicIcon className="empty-state-icon" /></div>
              <div className="subtitle-empty-text">正在监听当前音频...</div>
              <div className="subtitle-empty-hint">首条字幕出现后会自动滚动到最新内容。</div>
            </div>
          ) : (
            <div className="subtitle-list-shell">
              <div className="subtitle-stream-overview">
                <div>
                  <div className="subtitle-stream-title">实时字幕流</div>
                  <div className="subtitle-stream-desc">每一条字幕都直接绑定到 Gummy 的 sentence_id，不再本地拼句。</div>
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
                    <div key={`${item.segmentId}-${item.revision}`} className={`subtitle-entry ${item.state === "streaming" ? "streaming" : ""}`}>
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
              <button type="button" className={`btn-dock-action ${isFloatingOpen ? "active" : ""}`} onClick={onToggleFloating} title={isFloatingOpen ? "关闭悬浮窗" : "开启悬浮窗"}>
                <LayersIcon className="dock-icon" />
                <span className="dock-label">悬浮窗</span>
              </button>
            </div>

            <div className="dock-divider" />

            <div className="dock-group">
              <DockDropdown label="音频来源" options={captureSources} value={captureSource} onChange={handleCaptureSourceChange} disabled={isRecording} />
            </div>

            {captureSource === "app" && (
              <>
                <div className="dock-divider" />
                <div className="dock-group">
                  <DockDropdown label="应用" options={audioAppOptions} value={selectedAppValue} onChange={handleCaptureAppChange} disabled={isRecording || audioApps.length === 0} />
                  <button type="button" className="dock-refresh-btn" onClick={refreshAudioApps} title="刷新应用列表" disabled={isRecording}>
                    <RefreshIcon className="dock-refresh-icon" />
                  </button>
                </div>
              </>
            )}

            <div className="dock-divider" />

            <div className="dock-group">
              <DockDropdown label="识别语言" options={sourceLanguageOptions} value={asrLanguage} onChange={handleAsrLanguageChange} disabled={isRecording} />
            </div>

            <div className="dock-divider" />

            <button type="button" className={`btn-record-main ${isRecording ? "recording" : "idle"}`} onClick={handleToggleRecord}>
              <div className="record-pulse-ring" />
              <div className="record-inner-icon" />
              <span className="record-main-label">{isRecording ? "停止录制" : "开始录制"}</span>
            </button>

            <div className="dock-divider" />

            <div className="dock-group">
              <DockDropdown
                label="翻译目标"
                options={targetLanguageOptions.length > 0 ? targetLanguageOptions : [{ value: targetLanguage, short: "待选", full: "请先选择明确的识别语言" }]}
                value={targetLanguage}
                onChange={handleTargetLanguageChange}
                disabled={!translationEnabled || asrLanguage === "auto" || targetLanguageOptions.length === 0}
              />
            </div>

            <div className="dock-divider" />

            <div className="dock-group">
              <div className="dock-toggle-item">
                <span className="dock-label">翻译</span>
                <div className="segmented-toggle">
                  <button type="button" className={`segmented-btn ${!translationEnabled ? "active" : ""}`} onClick={() => translationEnabled && handleToggleTranslation()}>
                    关
                  </button>
                  <button type="button" className={`segmented-btn ${translationEnabled ? "active" : ""}`} onClick={() => !translationEnabled && handleToggleTranslation()}>
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
