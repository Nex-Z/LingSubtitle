mod asr;
mod audio;
mod config;
mod subtitle;
mod translation;

use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, watch};

use audio::AudioCapture;
use config::{AppConfig, load_config};

struct AppState {
    audio: Mutex<AudioCapture>,
    stop_tx: Mutex<Option<watch::Sender<bool>>>,
    translation_enabled: Mutex<bool>,
}

#[tauri::command]
async fn start_capture(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    config_state: tauri::State<'_, Mutex<AppConfig>>,
) -> Result<(), String> {
    let config = config_state.lock().map_err(|e| e.to_string())?.clone();

    // Create stop signal
    let (stop_tx, stop_rx) = watch::channel(false);
    {
        let mut guard = state.stop_tx.lock().map_err(|e| e.to_string())?;
        *guard = Some(stop_tx);
    }

    // Create audio channel
    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Start audio capture
    let sample_rate = {
        let mut audio_guard = state.audio.lock().map_err(|e| e.to_string())?;
        audio_guard.start(audio_tx)?
    };

    // Update ASR config with actual sample rate
    let mut asr_config = config.asr.clone();
    asr_config.sample_rate = sample_rate;

    let save_config = config.save.clone();

    // Get initial translation_enabled state from config
    {
        let mut te = state
            .translation_enabled
            .lock()
            .map_err(|e| e.to_string())?;
        *te = config.translation.enabled;
    }

    // Create ASR result channel
    let (result_tx, mut result_rx) = mpsc::unbounded_channel();

    // Spawn ASR WebSocket session
    let asr_stop_rx = stop_rx.clone();
    let app_asr_err = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = asr::run_asr_session(asr_config, audio_rx, result_tx, asr_stop_rx).await {
            eprintln!("ASR session error: {}", e);
            let _ = app_asr_err.emit("subtitle-error", format!("ASR 错误: {}", e));
        }
    });

    // Spawn result processing (translation + UI + save)
    let app_result = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut subtitle_mgr = subtitle::SubtitleManager::new(&save_config.save_path);
        subtitle_mgr.start_new_session();

        // Fallback translation state
        let mut pending_intermediate: Option<String> = None;
        let mut intermediate_start: Option<tokio::time::Instant> = None;
        let mut last_translated_text: String = String::new();

        const FALLBACK_CHAR_THRESHOLD: usize = 30;
        const FALLBACK_TIME_SECS: u64 = 5;

        // Helper closure-like: check if fallback translation should fire
        fn should_fallback_translate(
            text: &Option<String>,
            start: &Option<tokio::time::Instant>,
            last_translated: &str,
        ) -> bool {
            if let (Some(t), Some(s)) = (text, start) {
                if t == last_translated {
                    return false; // Already translated this exact text
                }
                t.chars().count() >= FALLBACK_CHAR_THRESHOLD
                    || s.elapsed() >= std::time::Duration::from_secs(FALLBACK_TIME_SECS)
            } else {
                false
            }
        }

        loop {
            // Use a short tick interval to check fallback conditions
            let fallback_check = tokio::time::sleep(tokio::time::Duration::from_millis(500));

            tokio::select! {
                result = result_rx.recv() => {
                    match result {
                        Some(asr_result) => {
                            // Emit original text (intermediate or final)
                            let _ = app_result.emit("subtitle-original", &asr_result.text);

                            if asr_result.is_final {
                                // Clear fallback state
                                pending_intermediate = None;
                                intermediate_start = None;

                                // Check runtime translation_enabled flag
                                let do_translate = app_result
                                    .state::<AppState>()
                                    .translation_enabled
                                    .lock()
                                    .map(|v| *v)
                                    .unwrap_or(false);

                                if do_translate && asr_result.text != last_translated_text {
                                    last_translated_text = asr_result.text.clone();
                                    let app_for_translate = app_result.clone();
                                    let text = asr_result.text.clone();
                                    // Read live config for each translation
                                    let translate_config = app_for_translate
                                        .state::<Mutex<AppConfig>>()
                                        .lock()
                                        .map(|c| c.translation.clone())
                                        .unwrap_or_else(|_| config::TranslationConfig::default());
                                    tauri::async_runtime::spawn(async move {
                                        match translation::translate(&translate_config, &text).await {
                                            Ok(t) => {
                                                let _ = app_for_translate.emit("subtitle-translated", &t);
                                            }
                                            Err(e) => {
                                                let _ = app_for_translate.emit(
                                                    "subtitle-error",
                                                    format!("翻译错误: {}", e),
                                                );
                                            }
                                        }
                                    });
                                }

                                // Auto-save
                                let entry = subtitle::SubtitleEntry {
                                    timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                                    original: asr_result.text.clone(),
                                    translated: None,
                                };
                                if save_config.auto_save {
                                    subtitle_mgr.save_entry(&entry).ok();
                                }
                            } else {
                                // Intermediate result: track for fallback
                                if pending_intermediate.is_none() {
                                    intermediate_start = Some(tokio::time::Instant::now());
                                }
                                pending_intermediate = Some(asr_result.text.clone());
                            }
                        }
                        None => break, // Channel closed
                    }
                }
                _ = fallback_check => {
                    // Check if we should trigger fallback translation
                    if should_fallback_translate(&pending_intermediate, &intermediate_start, &last_translated_text) {
                        let do_translate = app_result
                            .state::<AppState>()
                            .translation_enabled
                            .lock()
                            .map(|v| *v)
                            .unwrap_or(false);

                        if do_translate {
                            let text = pending_intermediate.clone().unwrap();
                            last_translated_text = text.clone();
                            // Reset timer so next fallback waits again
                            intermediate_start = Some(tokio::time::Instant::now());

                            let app_for_translate = app_result.clone();
                            // Read live config for each translation
                            let translate_config = app_for_translate
                                .state::<Mutex<AppConfig>>()
                                .lock()
                                .map(|c| c.translation.clone())
                                .unwrap_or_else(|_| config::TranslationConfig::default());
                            tauri::async_runtime::spawn(async move {
                                match translation::translate(&translate_config, &text).await {
                                    Ok(t) => {
                                        let _ = app_for_translate.emit("subtitle-translated", &t);
                                    }
                                    Err(e) => {
                                        let _ = app_for_translate.emit(
                                            "subtitle-error",
                                            format!("翻译错误: {}", e),
                                        );
                                    }
                                }
                            });
                        }
                    }
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn stop_capture(state: tauri::State<'_, AppState>) -> Result<(), String> {
    // Send stop signal
    {
        let guard = state.stop_tx.lock().map_err(|e| e.to_string())?;
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(true);
        }
    }

    // Stop audio capture
    {
        let mut audio = state.audio.lock().map_err(|e| e.to_string())?;
        audio.stop();
    }

    Ok(())
}

#[tauri::command]
fn get_capture_status(state: tauri::State<'_, AppState>) -> bool {
    state
        .audio
        .lock()
        .map(|a| a.is_running())
        .unwrap_or(false)
}

#[tauri::command]
fn set_translation_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let mut te = state
        .translation_enabled
        .lock()
        .map_err(|e| e.to_string())?;
    *te = enabled;
    Ok(())
}

#[tauri::command]
fn check_translation_config(
    config_state: tauri::State<'_, Mutex<AppConfig>>,
) -> Result<serde_json::Value, String> {
    let config = config_state.lock().map_err(|e| e.to_string())?;
    let t = &config.translation;

    let mut missing = Vec::new();
    if t.api_key.trim().is_empty() {
        missing.push("API Key");
    }
    if t.base_url.trim().is_empty() {
        missing.push("API 地址");
    }
    if t.model.trim().is_empty() {
        missing.push("模型名称");
    }

    if missing.is_empty() {
        Ok(serde_json::json!({ "ready": true, "message": "" }))
    } else {
        Ok(serde_json::json!({
            "ready": false,
            "message": format!("请先在设置中配置翻译服务：{}", missing.join("、"))
        }))
    }
}

#[tauri::command]
async fn open_floating_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::WebviewWindowBuilder;

    if app.get_webview_window("subtitle").is_some() {
        return Ok(());
    }

    // Get monitor dimensions to calculate center-bottom position
    let monitor = app
        .primary_monitor()
        .ok()
        .flatten()
        .ok_or("无法获取主显示器尺寸")?;
    let scale_factor = monitor.scale_factor();
    let monitor_size = monitor.size();
    
    // Convert physical pixels to logical pixels for calculation
    let screen_w = monitor_size.width as f64 / scale_factor;
    let screen_h = monitor_size.height as f64 / scale_factor;
    
    let win_w = 600.0;
    let win_h = 100.0;
    
    // Position: horizontal center, vertical 2/3 down (lower 1/3 area)
    let x = (screen_w - win_w) / 2.0;
    let y = (screen_h * 2.0 / 3.0) - (win_h / 2.0);

    let _window = WebviewWindowBuilder::new(&app, "subtitle", tauri::WebviewUrl::App("floating.html".into()))
        .title("字幕")
        .inner_size(win_w, win_h)
        .position(x, y)
        .always_on_top(true)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .skip_taskbar(true)
        .build()
        .map_err(|e| format!("Failed to create floating window: {}", e))?;

    Ok(())
}

#[tauri::command]
async fn close_floating_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("subtitle") {
        window
            .close()
            .map_err(|e| format!("Failed to close floating window: {}", e))?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            audio: Mutex::new(AudioCapture::new()),
            stop_tx: Mutex::new(None),
            translation_enabled: Mutex::new(false),
        })
        .setup(|app| {
            let config = load_config(&app.handle());
            app.manage(Mutex::new(config));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_capture,
            stop_capture,
            get_capture_status,
            set_translation_enabled,
            check_translation_config,
            config::get_config,
            config::save_config,
            open_floating_window,
            close_floating_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
