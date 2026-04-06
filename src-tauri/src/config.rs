use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

use crate::gummy::{
    normalize_source_language, normalize_target_language, DEFAULT_BASE_URL, DEFAULT_MODEL,
    DEFAULT_SAMPLE_RATE, DEFAULT_SOURCE_LANGUAGE, DEFAULT_TARGET_LANGUAGE, DEFAULT_VAD_SILENCE_MS,
};
use crate::subtitle::SubtitleManager;

const LEGACY_GUMMY_REALTIME_URL: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/realtime";
const LEGACY_QWEN3_ASR_MODEL: &str = "qwen3-asr-flash-realtime";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AsrConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub sample_rate: u32,
    pub language: String,
    pub vad_silence_ms: u32,
    pub vocabulary_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslationConfig {
    pub enabled: bool,
    pub target_language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SaveConfig {
    pub auto_save: bool,
    pub save_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub source: String,
    pub app_pid: Option<u32>,
    pub app_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub asr: AsrConfig,
    pub translation: TranslationConfig,
    pub save: SaveConfig,
    pub capture: CaptureConfig,
    pub filter_fillers: bool,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: String::new(),
            model: DEFAULT_MODEL.to_string(),
            sample_rate: DEFAULT_SAMPLE_RATE,
            language: DEFAULT_SOURCE_LANGUAGE.to_string(),
            vad_silence_ms: DEFAULT_VAD_SILENCE_MS,
            vocabulary_id: String::new(),
        }
    }
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            target_language: DEFAULT_TARGET_LANGUAGE.to_string(),
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

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            source: "system".to_string(),
            app_pid: None,
            app_name: String::new(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            asr: AsrConfig::default(),
            translation: TranslationConfig::default(),
            save: SaveConfig::default(),
            capture: CaptureConfig::default(),
            filter_fillers: false,
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
    if !path.exists() {
        let config = AppConfig::default();
        save_config_to_file(app, &config);
        return config;
    }

    let content = fs::read_to_string(&path).unwrap_or_default();
    let config = serde_json::from_str::<Value>(&content)
        .ok()
        .map(migrate_config_value)
        .and_then(|value| serde_json::from_value::<AppConfig>(value).ok())
        .unwrap_or_default();

    save_config_to_file(app, &config);
    config
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
    *current = config.clone();

    let subtitle_manager = app.state::<Mutex<SubtitleManager>>();
    if let Ok(mut manager) = subtitle_manager.lock() {
        manager.update_save_path(&config.save.save_path);
    }

    Ok(())
}

fn migrate_config_value(value: Value) -> Value {
    let mut root = match value {
        Value::Object(map) => map,
        _ => return serde_json::to_value(AppConfig::default()).unwrap_or_else(|_| json!({})),
    };

    let defaults = AppConfig::default();
    let asr_value = root.remove("asr").unwrap_or(Value::Null);
    let translation_value = root.remove("translation").unwrap_or(Value::Null);
    let save_value = root.remove("save").unwrap_or(Value::Null);
    let capture_value = root.remove("capture").unwrap_or(Value::Null);

    root.insert("asr".to_string(), migrate_asr_value(asr_value, &defaults.asr));
    root.insert(
        "translation".to_string(),
        migrate_translation_value(translation_value, &defaults.translation),
    );
    root.insert("save".to_string(), merge_with_default(save_value, &defaults.save));
    root.insert(
        "capture".to_string(),
        merge_with_default(capture_value, &defaults.capture),
    );
    root.entry("filter_fillers".to_string())
        .or_insert(Value::Bool(defaults.filter_fillers));

    Value::Object(root)
}

fn migrate_asr_value(value: Value, defaults: &AsrConfig) -> Value {
    let mut asr = value.as_object().cloned().unwrap_or_default();

    let language = asr
        .get("language")
        .and_then(Value::as_str)
        .and_then(normalize_source_language)
        .unwrap_or_else(|| defaults.language.clone());

    let base_url = asr
        .get("base_url")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| normalize_asr_base_url(value, &defaults.base_url))
        .unwrap_or_else(|| defaults.base_url.clone());

    let model = asr
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| normalize_asr_model(value, &defaults.model))
        .unwrap_or_else(|| defaults.model.clone());

    let sample_rate = asr
        .get("sample_rate")
        .and_then(Value::as_u64)
        .map(|value| value as u32)
        .filter(|value| *value == DEFAULT_SAMPLE_RATE)
        .unwrap_or(DEFAULT_SAMPLE_RATE);

    let vad_silence_ms = asr
        .get("vad_silence_ms")
        .and_then(Value::as_u64)
        .map(|value| value as u32)
        .map(|value| value.clamp(200, 6_000))
        .unwrap_or_else(|| defaults.vad_silence_ms);

    let vocabulary_id = asr
        .remove("vocabulary_id")
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_default();

    json!({
        "base_url": base_url,
        "api_key": asr
            .get("api_key")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "model": model,
        "sample_rate": sample_rate,
        "language": language,
        "vad_silence_ms": vad_silence_ms,
        "vocabulary_id": vocabulary_id,
    })
}

fn migrate_translation_value(value: Value, defaults: &TranslationConfig) -> Value {
    let translation = value.as_object().cloned().unwrap_or_default();
    let target_language = translation
        .get("target_language")
        .and_then(Value::as_str)
        .and_then(normalize_target_language)
        .unwrap_or_else(|| defaults.target_language.clone());

    json!({
        "enabled": translation
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(defaults.enabled),
        "target_language": target_language,
    })
}

fn normalize_asr_base_url(value: &str, default_value: &str) -> String {
    let normalized = value.trim().trim_end_matches('/');
    if normalized.eq_ignore_ascii_case(LEGACY_GUMMY_REALTIME_URL) {
        return default_value.to_string();
    }
    normalized.to_string()
}

fn normalize_asr_model(value: &str, default_value: &str) -> String {
    let normalized = value.trim();
    if normalized.eq_ignore_ascii_case(LEGACY_QWEN3_ASR_MODEL) {
        return default_value.to_string();
    }
    normalized.to_string()
}

fn merge_with_default<T>(value: Value, defaults: &T) -> Value
where
    T: Serialize,
{
    let default_value = serde_json::to_value(defaults).unwrap_or(Value::Null);
    merge_value(default_value, value)
}

fn merge_value(defaults: Value, overrides: Value) -> Value {
    match (defaults, overrides) {
        (Value::Object(mut default_map), Value::Object(override_map)) => {
            for (key, default_value) in default_map.clone() {
                if let Some(override_value) = override_map.get(&key) {
                    default_map.insert(key, merge_value(default_value, override_value.clone()));
                }
            }
            Value::Object(default_map)
        }
        (default_value, Value::Null) => default_value,
        (_, override_value) => override_value,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::migrate_config_value;

    #[test]
    fn migrates_legacy_translation_fields_to_gummy_shape() {
        let migrated = migrate_config_value(json!({
            "asr": {
                "base_url": "wss://dashscope.aliyuncs.com/api-ws/v1/realtime",
                "model": "qwen3-asr-flash-realtime",
                "language": "中文"
            },
            "translation": {
                "enabled": true,
                "base_url": "https://dashscope.aliyuncs.com/compatible-mode/v1",
                "api_key": "sk-old",
                "model": "qwen-plus",
                "system_prompt": "legacy",
                "target_language": "English"
            }
        }));

        assert_eq!(migrated["asr"]["language"], "zh");
        assert_eq!(
            migrated["asr"]["base_url"],
            "wss://dashscope.aliyuncs.com/api-ws/v1/inference"
        );
        assert_eq!(migrated["asr"]["model"], "gummy-realtime-v1");
        assert_eq!(migrated["translation"]["enabled"], true);
        assert_eq!(migrated["translation"]["target_language"], "en");
        assert!(migrated["translation"].get("base_url").is_none());
        assert!(migrated["translation"].get("model").is_none());
    }

    #[test]
    fn clamps_legacy_vad_silence_to_supported_range() {
        let migrated = migrate_config_value(json!({
            "asr": {
                "vad_silence_ms": 8000
            }
        }));

        assert_eq!(migrated["asr"]["vad_silence_ms"], 6000);
    }

    #[test]
    fn preserves_existing_capture_settings() {
        let migrated = migrate_config_value(json!({
            "capture": {
                "source": "app",
                "app_pid": 9527,
                "app_name": "player.exe"
            }
        }));

        assert_eq!(migrated["capture"]["source"], "app");
        assert_eq!(migrated["capture"]["app_pid"], 9527);
        assert_eq!(migrated["capture"]["app_name"], "player.exe");
    }
}
