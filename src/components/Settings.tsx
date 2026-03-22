import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./Settings.css";

interface AsrConfig {
  base_url: string;
  api_key: string;
  model: string;
  sample_rate: number;
  language: string;
  vad_silence_ms: number;
}

interface TranslationConfig {
  enabled: boolean;
  base_url: string;
  api_key: string;
  model: string;
  system_prompt: string;
  target_language: string;
}

interface SaveConfig {
  auto_save: boolean;
  save_path: string;
}

interface CaptureConfig {
  source: string;
  app_pid?: number | null;
  app_name?: string;
}

interface AppConfig {
  asr: AsrConfig;
  translation: TranslationConfig;
  save: SaveConfig;
  capture: CaptureConfig;
  filter_fillers: boolean;
}

interface SettingsProps {
  onBack: () => void;
}

type SettingsTab = "asr" | "translation" | "save";

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

function SparkIcon({ className }: { className?: string }) {
  return (
    <SvgIcon className={className}>
      <path d="M12 3l1.6 4.4L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6L12 3Z" />
      <path d="M19 14l.9 2.1L22 17l-2.1.9L19 20l-.9-2.1L16 17l2.1-.9L19 14Z" />
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
  asr: { label: "语音识别", desc: "录制与识别参数" },
  translation: { label: "翻译服务", desc: "模型与提示词" },
  save: { label: "保存设置", desc: "自动导出与路径" },
};

export default function Settings({ onBack }: SettingsProps) {
  const [config, setConfig] = useState<AppConfig>({
    asr: {
      base_url: "wss://dashscope.aliyuncs.com/api-ws/v1/realtime",
      api_key: "",
      model: "qwen3-asr-flash-realtime",
      sample_rate: 16000,
      language: "zh",
      vad_silence_ms: 300,
    },
    translation: {
      enabled: false,
      base_url: "https://api.openai.com/v1",
      api_key: "",
      model: "gpt-4o-mini",
      system_prompt:
        "你是一个专业的翻译助手。请将下列文本翻译为目标语言，只输出翻译结果，不要添加解释或额外内容。",
      target_language: "中文",
    },
    save: { auto_save: true, save_path: "" },
    capture: { source: "system", app_pid: null, app_name: "" },
    filter_fillers: false,
  });
  const [showSuccess, setShowSuccess] = useState(false);
  const [loading, setLoading] = useState(true);
  const [showAsrKey, setShowAsrKey] = useState(false);
  const [showTransKey, setShowTransKey] = useState(false);
  const [activeTab, setActiveTab] = useState<SettingsTab>("asr");

  useEffect(() => {
    invoke<AppConfig>("get_config")
      .then((cfg) => setConfig(cfg))
      .catch((err) => console.error("Failed to load config:", err))
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

  const updateAsr = (field: keyof AsrConfig, value: string | number) => {
    setConfig((prev) => ({ ...prev, asr: { ...prev.asr, [field]: value } }));
  };

  const updateTranslation = (field: keyof TranslationConfig, value: string | boolean) => {
    setConfig((prev) => ({ ...prev, translation: { ...prev.translation, [field]: value } }));
  };

  const updateSave = (field: keyof SaveConfig, value: string | boolean) => {
    setConfig((prev) => ({ ...prev, save: { ...prev.save, [field]: value } }));
  };

  const getVadPreset = (ms: number) => {
    if (ms <= 180) return "fast";
    if (ms <= 350) return "balanced";
    if (ms <= 650) return "stable";
    return "custom";
  };

  const handleVadPresetChange = (preset: string) => {
    if (preset === "fast") updateAsr("vad_silence_ms", 150);
    else if (preset === "balanced") updateAsr("vad_silence_ms", 300);
    else if (preset === "stable") updateAsr("vad_silence_ms", 500);
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
            <div className="settings-subtitle">正在加载配置...</div>
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
            <div className="settings-subtitle">只保留必要配置，不再显示重复概览。</div>
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
              {tab === "translation" && <SparkIcon className="sidebar-icon" />}
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
                  <div className="panel-title">语音识别</div>
                  <div className="panel-desc">优化录制实时性、识别模型和 API 连接。</div>
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
                    placeholder="wss://dashscope.aliyuncs.com/api-ws/v1/realtime"
                  />
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
                    placeholder="qwen3-asr-flash-realtime"
                  />
                </div>

                <div className="form-field">
                  <label className="form-label">VAD 预设</label>
                  <select
                    className="input-field"
                    value={getVadPreset(config.asr.vad_silence_ms)}
                    onChange={(e) => handleVadPresetChange(e.target.value)}
                  >
                    <option value="fast">更实时 (150ms)</option>
                    <option value="balanced">推荐 (300ms)</option>
                    <option value="stable">更稳定 (500ms)</option>
                    <option value="custom">自定义</option>
                  </select>
                </div>

                <div className="form-field">
                  <label className="form-label">静音阈值</label>
                  <input
                    className="input-field"
                    type="number"
                    min={100}
                    max={2000}
                    step={10}
                    value={config.asr.vad_silence_ms}
                    onChange={(e) =>
                      updateAsr("vad_silence_ms", Math.min(2000, Math.max(100, Number(e.target.value))))
                    }
                  />
                </div>
              </div>
            </div>
          )}

          {activeTab === "translation" && (
            <div className="settings-panel">
              <div className="panel-header">
                <div>
                  <div className="panel-title">翻译服务</div>
                  <div className="panel-desc">配置模型、API 地址和翻译提示词。</div>
                </div>
                <label className="toggle-switch">
                  <input
                    type="checkbox"
                    checked={config.translation.enabled}
                    onChange={(e) => updateTranslation("enabled", e.target.checked)}
                  />
                  <span className="toggle-slider" />
                </label>
              </div>

              <div className="settings-form-grid">
                <div className="form-field">
                  <label className="form-label">API 地址</label>
                  <input
                    className="input-field"
                    value={config.translation.base_url}
                    onChange={(e) => updateTranslation("base_url", e.target.value)}
                    placeholder="https://api.openai.com/v1"
                  />
                </div>

                <div className="form-field">
                  <label className="form-label">目标语言</label>
                  <input
                    className="input-field"
                    value={config.translation.target_language}
                    onChange={(e) => updateTranslation("target_language", e.target.value)}
                    placeholder="中文"
                  />
                </div>

                <div className="form-field">
                  <label className="form-label">API Key</label>
                  <div className="input-wrapper">
                    <input
                      className="input-field"
                      type={showTransKey ? "text" : "password"}
                      value={config.translation.api_key}
                      onChange={(e) => updateTranslation("api_key", e.target.value)}
                      placeholder="sk-..."
                    />
                    <button
                      type="button"
                      className="input-icon-btn"
                      onClick={() => setShowTransKey((prev) => !prev)}
                      title={showTransKey ? "隐藏" : "显示"}
                    >
                      {showTransKey ? <EyeOffIcon className="field-icon" /> : <EyeIcon className="field-icon" />}
                    </button>
                  </div>
                </div>

                <div className="form-field">
                  <label className="form-label">模型名称</label>
                  <input
                    className="input-field"
                    value={config.translation.model}
                    onChange={(e) => updateTranslation("model", e.target.value)}
                    placeholder="gpt-4o-mini"
                  />
                </div>

                <div className="form-field span-2">
                  <label className="form-label">系统提示词</label>
                  <textarea
                    className="input-field textarea-field"
                    value={config.translation.system_prompt}
                    onChange={(e) => updateTranslation("system_prompt", e.target.value)}
                    rows={6}
                    placeholder="你是一个专业的翻译助手..."
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
                  <div className="panel-desc">控制字幕记录是否自动导出到本地目录。</div>
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
