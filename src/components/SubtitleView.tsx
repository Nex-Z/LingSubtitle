import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./SubtitleView.css";

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
  const listRef = useRef<HTMLDivElement>(null);
  const idCounter = useRef(0);

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

  // Load initial translation state from config
  useEffect(() => {
    invoke<{ translation: { enabled: boolean } }>("get_config")
      .then((cfg) => setTranslationEnabled(cfg.translation.enabled))
      .catch(() => {});
  }, []);

  useEffect(() => {
    // Check initial capture status
    invoke<boolean>("get_capture_status").then(setIsRecording).catch(() => {});

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
        // Translation comes after a final result → mark last entry as final and attach translation
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
        await invoke("start_capture");
        setIsRecording(true);
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
      // Trying to enable — check config completeness first
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
            <button className="nav-item" onClick={onOpenSettings}>
              <span className="nav-icon">⚙️</span>
              配置中心
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
                <div className="hero-icon">✨</div>
                <h1>欢迎使用 灵幕</h1>
                <p>专业的实时语音转写与翻译工具，让沟通无国界。</p>
              </div>

              <div className="quick-actions-grid">
                <div className="action-card" onClick={handleToggleRecord}>
                  <div className="action-icon">🎤</div>
                  <div className="action-info">
                    <h3>开始录制</h3>
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
                  <span>最近动态</span>
                  <button className="btn-text-only">查看全部</button>
                </div>
                <div className="empty-hint-box">
                  暂无最近记录。开始一次录制后，这里将显示您的转写历史。
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
                  <div className="subtitle-split-content">
                    <div className="subtitle-col original">
                      <div className="subtitle-label">原文</div>
                      <div className="subtitle-text">{item.original}</div>
                    </div>
                    {item.translated && (
                      <>
                        <div className="subtitle-col-divider" />
                        <div className="subtitle-col translated">
                          <div className="subtitle-label">译文</div>
                          <div className="subtitle-text">{item.translated}</div>
                        </div>
                      </>
                    )}
                  </div>
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

            <button
              className={`btn-record-main ${isRecording ? "recording" : "idle"}`}
              onClick={handleToggleRecord}
            >
              <div className="record-pulse-ring" />
              <div className="record-inner-icon" />
              <span className="record-main-label">{isRecording ? "停止录制" : "开始录制"}</span>
            </button>

            <div className="dock-divider" />

            <div className="dock-group">
              <div className="dock-toggle-item">
                <span className="dock-icon">🌐</span>
                <span className="dock-label">中英翻译</span>
                <label className="toggle-switch-mini">
                  <input
                    type="checkbox"
                    checked={translationEnabled}
                    onChange={handleToggleTranslation}
                  />
                  <span className="toggle-slider" />
                </label>
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
