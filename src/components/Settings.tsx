import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./Settings.css";
import type { AppConfig, GummyCapabilities } from "../types/gummy";

interface SettingsProps {
  onBack: () => void;
}

type SettingsTab = "asr" | "save";

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
      aria-hidden="true"
      className={className}
    >
      {children}
    </svg>
  );
}

function ArrowLeftIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M15 18l-6-6 6-6" />
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

function FolderIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" />
    </SvgIcon>
  );
}

function EyeIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6Z" />
      <circle cx="12" cy="12" r="3" />
    </SvgIcon>
  );
}

function EyeOffIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M3 3l18 18" />
      <path d="M10.7 6.2A11.7 11.7 0 0 1 12 6c6.5 0 10 6 10 6a17.6 17.6 0 0 1-3.4 4.2" />
      <path d="M6.2 6.3A17.5 17.5 0 0 0 2 12s3.5 6 10 6c1.2 0 2.4-.2 3.4-.5" />
      <path d="M9.9 9.9a3 3 0 0 0 4.2 4.2" />
    </SvgIcon>
  );
}

const tabMeta: Record<SettingsTab, { label: string; desc: string }> = {
  asr: { label: "Gummy 识别", desc: "连接、VAD 与热词" },
  save: { label: "保存设置", desc: "自动导出与路径" },
};

const EMPTY_CONFIG: AppConfig = {
  asr: {
    base_url: "",
    api_key: "",
    model: "",
    sample_rate: 16000,
    language: "auto",
    vad_silence_ms: 800,
    vocabulary_id: "",
  },
  translation: {
    enabled: false,
    target_language: "en",
  },
  save: { auto_save: true, save_path: "" },
  capture: { source: "system", app_pid: null, app_name: "" },
  filter_fillers: false,
};

export default function Settings({ onBack }: SettingsProps) {
  const [config, setConfig] = useState<AppConfig>(EMPTY_CONFIG);
  const [capabilities, setCapabilities] = useState<GummyCapabilities | null>(null);
  const [showSuccess, setShowSuccess] = useState(false);
  const [loading, setLoading] = useState(true);
  const [showAsrKey, setShowAsrKey] = useState(false);
  const [activeTab, setActiveTab] = useState<SettingsTab>("asr");

  useEffect(() => {
    Promise.all([
      invoke<AppConfig>("get_config"),
      invoke<GummyCapabilities>("get_gummy_capabilities"),
    ])
      .then(([cfg, gummyCapabilities]) => {
        setConfig(cfg);
        setCapabilities(gummyCapabilities);
      })
      .catch((err) => console.error("Failed to load settings:", err))
      .finally(() => setLoading(false));
  }, []);

  const handleSave = async () => {
    try {
      await invoke("save_config", { config });
      setShowSuccess(true);
      setTimeout(() => setShowSuccess(false), 3000);
    } catch (err) {
      console.error("Failed to save config:", err);
    }
  };

  const updateAsr = (field: keyof AppConfig["asr"], value: string | number) => {
    setConfig((prev) => ({ ...prev, asr: { ...prev.asr, [field]: value } }));
  };

  const updateSave = (field: keyof AppConfig["save"], value: string | boolean) => {
    setConfig((prev) => ({ ...prev, save: { ...prev.save, [field]: value } }));
  };

  const getVadPreset = (ms: number) => {
    if (ms <= 500) return "fast";
    if (ms <= 900) return "balanced";
    if (ms <= 1500) return "stable";
    return "custom";
  };

  const handleVadPresetChange = (preset: string) => {
    if (preset === "fast") updateAsr("vad_silence_ms", 500);
    else if (preset === "balanced") updateAsr("vad_silence_ms", 800);
    else if (preset === "stable") updateAsr("vad_silence_ms", 1200);
  };

  if (loading) {
    return (
      <div className="settings-page">
        <div className="settings-header">
          <button type="button" className="btn-back" onClick={onBack} title="返回">
            <ArrowLeftIcon className="header-icon" />
          </button>
          <div>
            <div className="settings-title">设置中心</div>
            <div className="settings-subtitle">正在加载 Gummy 配置...</div>
          </div>
        </div>
        <div className="settings-loading">正在读取本地配置，请稍候。</div>
      </div>
    );
  }

  return (
    <div className="settings-page">
      <div className="settings-header">
        <div className="settings-header-left">
          <button type="button" className="btn-back" onClick={onBack} title="返回">
            <ArrowLeftIcon className="header-icon" />
          </button>
          <div>
            <div className="settings-title">设置中心</div>
            <div className="settings-subtitle">当前在线引擎已切换为 Gummy 实时识别翻译。</div>
          </div>
        </div>
      </div>

      <div className="settings-content">
        <aside className="settings-sidebar">
          {(Object.keys(tabMeta) as SettingsTab[]).map((tab) => (
            <button
              key={tab}
              type="button"
              className={`sidebar-item ${activeTab === tab ? "active" : ""}`}
              onClick={() => setActiveTab(tab)}
            >
              {tab === "asr" && <MicIcon className="sidebar-icon" />}
              {tab === "save" && <FolderIcon className="sidebar-icon" />}
              <span className="sidebar-copy">
                <span className="sidebar-title">{tabMeta[tab].label}</span>
                <span className="sidebar-desc">{tabMeta[tab].desc}</span>
              </span>
            </button>
          ))}
        </aside>

        <section className="settings-detail">
          {activeTab === "asr" && (
            <div className="settings-panel">
              <div className="panel-header">
                <div>
                  <div className="panel-title">Gummy 识别链路</div>
                  <div className="panel-desc">这里配置实时语音任务本身。翻译开关和目标语言已经收敛到首页快捷栏。</div>
                </div>
              </div>

              <div className="settings-form-grid">
                <div className="form-field toggle-field">
                  <div className="field-copy">
                    <label className="form-label">过滤语气词</label>
                    <span className="form-hint">仅过滤单独出现的语气词，不影响正常句子。</span>
                  </div>
                  <label className="toggle-switch">
                    <input
                      type="checkbox"
                      checked={config.filter_fillers}
                      onChange={(e) =>
                        setConfig((prev) => ({ ...prev, filter_fillers: e.target.checked }))
                      }
                    />
                    <span className="toggle-slider" />
                  </label>
                </div>

                <div className="form-field span-2">
                  <label className="form-label">WebSocket 地址</label>
                  <input
                    className="input-field"
                    value={config.asr.base_url}
                    onChange={(e) => updateAsr("base_url", e.target.value)}
                    placeholder={capabilities?.defaults.base_url || "wss://dashscope.aliyuncs.com/api-ws/v1/inference"}
                  />
                  <span className="form-hint">建议保持阿里云官方 Gummy WebSocket 地址。</span>
                </div>

                <div className="form-field">
                  <label className="form-label">API Key</label>
                  <div className="input-wrapper">
                    <input
                      className="input-field"
                      type={showAsrKey ? "text" : "password"}
                      value={config.asr.api_key}
                      onChange={(e) => updateAsr("api_key", e.target.value)}
                      placeholder="sk-..."
                    />
                    <button
                      type="button"
                      className="input-icon-btn"
                      onClick={() => setShowAsrKey((prev) => !prev)}
                      title={showAsrKey ? "隐藏" : "显示"}
                    >
                      {showAsrKey ? <EyeOffIcon className="field-icon" /> : <EyeIcon className="field-icon" />}
                    </button>
                  </div>
                </div>

                <div className="form-field">
                  <label className="form-label">模型名称</label>
                  <input
                    className="input-field"
                    value={config.asr.model}
                    onChange={(e) => updateAsr("model", e.target.value)}
                    placeholder={capabilities?.defaults.model || "gummy-realtime-v1"}
                  />
                </div>

                <div className="form-field">
                  <label className="form-label">采样率</label>
                  <input
                    className="input-field"
                    type="number"
                    value={config.asr.sample_rate}
                    disabled
                    readOnly
                  />
                  <span className="form-hint">当前桌面音频链路固定输出 16000Hz PCM，无需修改。</span>
                </div>

                <div className="form-field">
                  <label className="form-label">热词表 ID</label>
                  <input
                    className="input-field"
                    value={config.asr.vocabulary_id}
                    onChange={(e) => updateAsr("vocabulary_id", e.target.value)}
                    placeholder="可选，留空则不启用"
                  />
                </div>

                <div className="form-field">
                  <label className="form-label">VAD 预设</label>
                  <select
                    className="input-field"
                    value={getVadPreset(config.asr.vad_silence_ms)}
                    onChange={(e) => handleVadPresetChange(e.target.value)}
                  >
                    <option value="fast">更实时 (500ms)</option>
                    <option value="balanced">推荐 (800ms)</option>
                    <option value="stable">更稳定 (1200ms)</option>
                    <option value="custom">自定义</option>
                  </select>
                </div>

                <div className="form-field">
                  <label className="form-label">句尾静音阈值</label>
                  <input
                    className="input-field"
                    type="number"
                    min={200}
                    max={6000}
                    step={100}
                    value={config.asr.vad_silence_ms}
                    onChange={(e) =>
                      updateAsr(
                        "vad_silence_ms",
                        Math.min(6000, Math.max(200, Number(e.target.value) || 800))
                      )
                    }
                  />
                </div>
              </div>
            </div>
          )}

          {activeTab === "save" && (
            <div className="settings-panel">
              <div className="panel-header">
                <div>
                  <div className="panel-title">自动保存</div>
                  <div className="panel-desc">最终句会按会话持续写入本地文本文件。</div>
                </div>
                <label className="toggle-switch">
                  <input
                    type="checkbox"
                    checked={config.save.auto_save}
                    onChange={(e) => updateSave("auto_save", e.target.checked)}
                  />
                  <span className="toggle-slider" />
                </label>
              </div>

              <div className="settings-form-grid">
                <div className="form-field span-2">
                  <label className="form-label">保存路径</label>
                  <input
                    className="input-field"
                    value={config.save.save_path}
                    onChange={(e) => updateSave("save_path", e.target.value)}
                    placeholder="C:\\Users\\...\\Documents\\LingSubtitle"
                    disabled={!config.save.auto_save}
                  />
                  <span className="form-hint">字幕文件会按时间命名，例如 字幕_YYYY-MM-DD_HH-mm-ss.txt</span>
                </div>
              </div>
            </div>
          )}
        </section>
      </div>

      <div className="settings-footer">
        <button className="btn btn-secondary" onClick={onBack}>
          取消
        </button>
        <button className="btn btn-primary" onClick={handleSave}>
          保存设置
        </button>
      </div>

      {showSuccess && <div className="save-success">设置已保存</div>}
    </div>
  );
}
