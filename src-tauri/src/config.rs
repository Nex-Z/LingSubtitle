use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub sample_rate: u32,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub system_prompt: String,
    pub target_language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveConfig {
    pub auto_save: bool,
    pub save_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub asr: AsrConfig,
    pub translation: TranslationConfig,
    pub save: SaveConfig,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            base_url: "wss://dashscope.aliyuncs.com/api-ws/v1/realtime".to_string(),
            api_key: String::new(),
            model: "qwen3-asr-flash-realtime".to_string(),
            sample_rate: 16000,
            language: "auto".to_string(),
        }
    }
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "gpt-4o-mini".to_string(),
            system_prompt: "你是一个专业的翻译助手。请将以下文本翻译为目标语言，只输出翻译结果，不要添加任何解释或额外内容。".to_string(),
            target_language: "中文".to_string(),
        }
    }
}

impl Default for SaveConfig {
    fn default() -> Self {
        let default_path = dirs::document_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("LingSubtitle");
        Self {
            auto_save: true,
            save_path: default_path.to_string_lossy().to_string(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            asr: AsrConfig::default(),
            translation: TranslationConfig::default(),
            save: SaveConfig::default(),
        }
    }
}

fn config_path(app: &AppHandle) -> PathBuf {
    let app_dir = app
        .path()
        .app_data_dir()
        .expect("Failed to get app data dir");
    fs::create_dir_all(&app_dir).ok();
    app_dir.join("config.json")
}

pub fn load_config(app: &AppHandle) -> AppConfig {
    let path = config_path(app);
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        let config = AppConfig::default();
        save_config_to_file(app, &config);
        config
    }
}

fn save_config_to_file(app: &AppHandle, config: &AppConfig) {
    let path = config_path(app);
    if let Ok(content) = serde_json::to_string_pretty(config) {
        fs::write(&path, content).ok();
    }
}

#[tauri::command]
pub fn get_config(state: tauri::State<'_, Mutex<AppConfig>>) -> Result<AppConfig, String> {
    let config = state.lock().map_err(|e| e.to_string())?;
    Ok(config.clone())
}

#[tauri::command]
pub fn save_config(
    app: AppHandle,
    state: tauri::State<'_, Mutex<AppConfig>>,
    config: AppConfig,
) -> Result<(), String> {
    save_config_to_file(&app, &config);
    let mut current = state.lock().map_err(|e| e.to_string())?;
    *current = config;
    Ok(())
}
