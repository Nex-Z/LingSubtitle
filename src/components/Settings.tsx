import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./Settings.css";

interface AsrConfig {
  base_url: string;
  api_key: string;
  model: string;
  sample_rate: number;
  language: string;
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

interface AppConfig {
  asr: AsrConfig;
  translation: TranslationConfig;
  save: SaveConfig;
}

interface SettingsProps {
  onBack: () => void;
}

export default function Settings({ onBack }: SettingsProps) {
  const [config, setConfig] = useState<AppConfig>({
    asr: {
      base_url: "wss://dashscope.aliyuncs.com/api-ws/v1/realtime",
      api_key: "",
      model: "qwen3-asr-flash-realtime",
      sample_rate: 16000,
      language: "zh",
    },
    translation: {
      enabled: false,
      base_url: "https://api.openai.com/v1",
      api_key: "",
      model: "gpt-4o-mini",
      system_prompt:
        "你是一个专业的翻译助手。请将以下文本翻译为目标语言，只输出翻译结果，不要添加任何解释或额外内容。",
      target_language: "中文",
    },
    save: {
      auto_save: true,
      save_path: "",
    },
  });
  const [showSuccess, setShowSuccess] = useState(false);
  const [loading, setLoading] = useState(true);
  const [showAsrKey, setShowAsrKey] = useState(false);
  const [showTransKey, setShowTransKey] = useState(false);
  const [activeTab, setActiveTab] = useState<"asr" | "translation" | "save">("asr");

  useEffect(() => {
    loadConfig();
  }, []);

  const loadConfig = async () => {
    try {
      const cfg = await invoke<AppConfig>("get_config");
      setConfig(cfg);
    } catch (err) {
      console.error("Failed to load config:", err);
    } finally {
      setLoading(false);
    }
  };

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
    setConfig((prev) => ({
      ...prev,
      asr: { ...prev.asr, [field]: value },
    }));
  };

  const updateTranslation = (
    field: keyof TranslationConfig,
    value: string | boolean
  ) => {
    setConfig((prev) => ({
      ...prev,
      translation: { ...prev.translation, [field]: value },
    }));
  };

  const updateSave = (field: keyof SaveConfig, value: string | boolean) => {
    setConfig((prev) => ({
      ...prev,
      save: { ...prev.save, [field]: value },
    }));
  };

  if (loading) {
    return (
      <div className="settings-page">
        <div className="settings-header">
          <button className="btn-back" onClick={onBack}>
            <span>←</span>
          </button>
          <span className="settings-title">设置</span>
        </div>
        <div className="settings-content">
          <div className="subtitle-empty" style={{ margin: "auto" }}>
            <div className="subtitle-empty-text">加载中...</div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="settings-page">
      {/* Header */}
      <div className="settings-header">
        <button className="btn-back" onClick={onBack} title="返回">
          <span>←</span>
        </button>
        <span className="settings-title">设置</span>
      </div>

      {/* Content */}
      <div className="settings-content">
        {/* Sidebar */}
        <div className="settings-sidebar">
          <div
            className={`sidebar-item ${activeTab === "asr" ? "active" : ""}`}
            onClick={() => setActiveTab("asr")}
          >
            <span className="sidebar-icon">🎤</span>
            <span>语音识别</span>
          </div>
          <div
            className={`sidebar-item ${
              activeTab === "translation" ? "active" : ""
            }`}
            onClick={() => setActiveTab("translation")}
          >
            <span className="sidebar-icon">🌐</span>
            <span>翻译设置</span>
          </div>
          <div
            className={`sidebar-item ${activeTab === "save" ? "active" : ""}`}
            onClick={() => setActiveTab("save")}
          >
            <span className="sidebar-icon">💾</span>
            <span>保存设置</span>
          </div>
        </div>

        {/* Detail Area */}
        <div className="settings-detail">
          {activeTab === "asr" && (
            <div className="settings-section">
              <div className="settings-section-header">
                <span className="settings-section-title">
                  🎤 语音识别（ASR）
                </span>
              </div>
              <div className="settings-section-body">
                <div className="form-field">
                  <label className="form-label">WebSocket 地址</label>
                  <input
                    className="input-field"
                    value={config.asr.base_url}
                    onChange={(e) => updateAsr("base_url", e.target.value)}
                    placeholder="wss://dashscope.aliyuncs.com/api-ws/v1/realtime"
                  />
                  <span className="form-hint">
                    阿里云百炼 Realtime API 地址（不含 model 参数，系统自动拼接）
                  </span>
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
                      className="input-icon-btn"
                      onClick={() => setShowAsrKey(!showAsrKey)}
                      title={showAsrKey ? "隐藏" : "查看"}
                    >
                      {showAsrKey ? "👁️‍🗨️" : "👁️"}
                    </button>
                  </div>
                  <span className="form-hint">
                    百炼 API Key，获取地址：https://bailian.console.aliyun.com
                  </span>
                </div>
                <div className="form-field">
                  <label className="form-label">模型名称</label>
                  <input
                    className="input-field"
                    value={config.asr.model}
                    onChange={(e) => updateAsr("model", e.target.value)}
                    placeholder="qwen3-asr-flash-realtime"
                  />
                  <span className="form-hint">
                    推荐：qwen3-asr-flash-realtime（稳定版）、qwen3-asr-flash-realtime-2026-02-10（最新快照版）
                  </span>
                </div>
              </div>
            </div>
          )}

          {activeTab === "translation" && (
            <div className="settings-section">
              <div className="settings-section-header">
                <span className="settings-section-title">🌐 翻译服务</span>
                <label className="toggle-switch">
                  <input
                    type="checkbox"
                    checked={config.translation.enabled}
                    onChange={(e) =>
                      updateTranslation("enabled", e.target.checked)
                    }
                  />
                  <span className="toggle-slider" />
                </label>
              </div>
              {config.translation.enabled && (
                <div className="settings-section-body">
                  <div className="form-field">
                    <label className="form-label">API 地址 (Base URL)</label>
                    <input
                      className="input-field"
                      value={config.translation.base_url}
                      onChange={(e) =>
                        updateTranslation("base_url", e.target.value)
                      }
                      placeholder="https://api.openai.com/v1"
                    />
                  </div>
                  <div className="form-field">
                    <label className="form-label">API Key</label>
                    <div className="input-wrapper">
                      <input
                        className="input-field"
                        type={showTransKey ? "text" : "password"}
                        value={config.translation.api_key}
                        onChange={(e) =>
                          updateTranslation("api_key", e.target.value)
                        }
                        placeholder="sk-..."
                      />
                      <button
                        className="input-icon-btn"
                        onClick={() => setShowTransKey(!showTransKey)}
                        title={showTransKey ? "隐藏" : "查看"}
                      >
                        {showTransKey ? "👁️‍🗨️" : "👁️"}
                      </button>
                    </div>
                  </div>
                  <div className="form-field">
                    <label className="form-label">模型名称</label>
                    <input
                      className="input-field"
                      value={config.translation.model}
                      onChange={(e) =>
                        updateTranslation("model", e.target.value)
                      }
                      placeholder="gpt-4o-mini"
                    />
                  </div>
                  <div className="form-field">
                    <label className="form-label">
                      翻译提示词 (System Prompt)
                    </label>
                    <textarea
                      className="input-field"
                      value={config.translation.system_prompt}
                      onChange={(e) =>
                        updateTranslation("system_prompt", e.target.value)
                      }
                      rows={5}
                      placeholder="你是一个专业的翻译助手..."
                    />
                  </div>
                </div>
              )}
              {!config.translation.enabled && (
                <div className="settings-section-body">
                  <div className="subtitle-empty">
                    <div className="subtitle-empty-text">翻译服务已禁用</div>
                  </div>
                </div>
              )}
            </div>
          )}

          {activeTab === "save" && (
            <div className="settings-section">
              <div className="settings-section-header">
                <span className="settings-section-title">💾 自动保存</span>
                <label className="toggle-switch">
                  <input
                    type="checkbox"
                    checked={config.save.auto_save}
                    onChange={(e) => updateSave("auto_save", e.target.checked)}
                  />
                  <span className="toggle-slider" />
                </label>
              </div>
              {config.save.auto_save && (
                <div className="settings-section-body">
                  <div className="form-field">
                    <label className="form-label">保存路径</label>
                    <input
                      className="input-field"
                      value={config.save.save_path}
                      onChange={(e) => updateSave("save_path", e.target.value)}
                      placeholder="C:\Users\...\Documents\LingSubtitle"
                    />
                    <span className="form-hint">
                      字幕文件将保存为：字幕_YYYY-MM-DD_HH-mm-ss.txt
                    </span>
                  </div>
                </div>
              )}
              {!config.save.auto_save && (
                <div className="settings-section-body">
                  <div className="subtitle-empty">
                    <div className="subtitle-empty-text">自动保存已禁用</div>
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Footer */}
      <div className="settings-footer">
        <button className="btn btn-secondary" onClick={onBack}>
          取消
        </button>
        <button className="btn btn-primary" onClick={handleSave}>
          保存设置
        </button>
      </div>

      {/* Success Toast */}
      {showSuccess && (
        <div className="save-success">
          <span>✅</span>
          <span>设置已保存</span>
        </div>
      )}
    </div>
  );
}
