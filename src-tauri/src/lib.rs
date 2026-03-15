mod asr;
mod audio;
mod config;
mod subtitle;
mod translation;

use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, watch};

use audio::{AudioApp, AudioCapture};
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
    let capture_pid = if config.capture.source == "app" {
        config.capture.app_pid
    } else {
        None
    };

    let sample_rate = {
        let mut audio_guard = state.audio.lock().map_err(|e| e.to_string())?;
        match capture_pid {
            Some(pid) => match audio_guard.start(audio_tx.clone(), Some(pid)) {
                Ok(rate) => rate,
                Err(e) => {
                    let _ = app.emit(
                        "subtitle-error",
                        format!("应用录制失败，已回退系统声音: {}", e),
                    );
                    audio_guard.start(audio_tx, None)?
                }
            },
            None => audio_guard.start(audio_tx, None)?,
        }
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
        let mut last_intermediate_text: String = String::new();
        let mut stable_suffix_count: u32 = 0;
        let mut last_change_at: Option<tokio::time::Instant> = None;
        let mut last_translate_request_at: Option<tokio::time::Instant> = None;
        let mut last_final_at: Option<tokio::time::Instant> = None;
        let mut last_intermediate_sent_at: Option<tokio::time::Instant> = None;

        const FALLBACK_CHAR_THRESHOLD: usize = 15;
        const FALLBACK_TIME_SECS: u64 = 2;
        const STABLE_SUFFIX_MIN: usize = 8;
        const STABLE_COUNT_MIN: u32 = 3;
        const STABLE_WAIT_MILLIS: u64 = 800;
        const TRANSLATE_MIN_INTERVAL_MILLIS: u64 = 700;

        let latest_req_id = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (translate_tx, mut translate_rx) = watch::channel::<Option<(u64, String)>>(None);

        let app_translate_worker = app_result.clone();
        let latest_req_id_worker = latest_req_id.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                if translate_rx.changed().await.is_err() {
                    break;
                }
                let payload = translate_rx.borrow().clone();
                let Some((req_id, text)) = payload else { continue; };

                let translate_config = app_translate_worker
                    .state::<Mutex<AppConfig>>()
                    .lock()
                    .map(|c| c.translation.clone())
                    .unwrap_or_else(|_| config::TranslationConfig::default());

                match translation::translate(&translate_config, &text).await {
                    Ok(t) => {
                        if req_id
                            == latest_req_id_worker.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            let _ = app_translate_worker.emit("subtitle-translated", &t);
                        }
                    }
                    Err(e) => {
                        let _ = app_translate_worker.emit(
                            "subtitle-error",
                            format!("翻译错误: {}", e),
                        );
                    }
                }
            }
        });

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

        fn common_suffix_len(a: &str, b: &str) -> usize {
            let mut count = 0usize;
            let mut a_iter = a.chars().rev();
            let mut b_iter = b.chars().rev();
            loop {
                match (a_iter.next(), b_iter.next()) {
                    (Some(x), Some(y)) if x == y => count += 1,
                    _ => break,
                }
            }
            count
        }

        fn can_trigger(
            now: tokio::time::Instant,
            last: &Option<tokio::time::Instant>,
        ) -> bool {
            match last {
                Some(t) => {
                    now.duration_since(*t)
                        >= std::time::Duration::from_millis(TRANSLATE_MIN_INTERVAL_MILLIS)
                }
                None => true,
            }
        }

        fn is_filler_only(text: &str) -> bool {
            fn is_punct_or_space(c: char) -> bool {
                matches!(
                    c,
                    ' ' | '\t' | '\n' | '\r'
                        | '.' | ',' | '!' | '?' | ':' | ';'
                        | '。' | '，' | '！' | '？' | '：' | '；'
                        | '…' | '—' | '-' | '–' | '·' | '、' | '_' | '～'
                )
            }

            let normalized: String = text.chars().filter(|c| !is_punct_or_space(*c)).collect();
            if normalized.is_empty() {
                return false;
            }

            let lower = normalized.to_lowercase();
            let en_fillers = ["um", "uh", "er", "ah", "oh", "hmm", "hm"];
            if en_fillers.iter().any(|f| f == &lower) {
                return true;
            }

            let zh_fillers = [
                "嗯", "啊", "呃", "哦", "噢", "唔", "额", "诶", "欸", "哎", "呀", "嘛", "呗",
                "嘿", "哈", "嗯哼",
            ];
            if zh_fillers.iter().any(|f| f == &normalized) {
                return true;
            }

            if normalized.chars().count() <= 3 {
                let mut chars = normalized.chars();
                if let Some(first) = chars.next() {
                    if chars.all(|c| c == first) {
                        return zh_fillers.iter().any(|f| f.chars().next() == Some(first));
                    }
                }
            }

            false
        }

        loop {
            // Use a short tick interval to check fallback conditions
            let fallback_check = tokio::time::sleep(tokio::time::Duration::from_millis(500));

            tokio::select! {
                result = result_rx.recv() => {
                    match result {
                        Some(asr_result) => {
                            let filter_fillers = app_result
                                .state::<Mutex<AppConfig>>()
                                .lock()
                                .map(|c| c.filter_fillers)
                                .unwrap_or(false);
                            if filter_fillers && is_filler_only(&asr_result.text) {
                                continue;
                            }
                            // Emit original text (intermediate or final)
                            let _ = app_result.emit("subtitle-original", &asr_result.text);

                            if asr_result.is_final {
                                // Clear fallback state
                                pending_intermediate = None;
                                intermediate_start = None;
                                last_intermediate_text.clear();
                                stable_suffix_count = 0;
                                last_change_at = None;
                                last_final_at = Some(tokio::time::Instant::now());

                                // Check runtime translation_enabled flag
                                let do_translate = app_result
                                    .state::<AppState>()
                                    .translation_enabled
                                    .lock()
                                    .map(|v| *v)
                                    .unwrap_or(false);

                                if do_translate && asr_result.text != last_translated_text {
                                    last_translated_text = asr_result.text.clone();
                                    let text = asr_result.text.clone();
                                    let req_id = latest_req_id
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                        + 1;
                                    let _ = translate_tx.send(Some((req_id, text)));
                                    last_translate_request_at = Some(tokio::time::Instant::now());
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
                                let now = tokio::time::Instant::now();
                                if pending_intermediate.is_none() {
                                    intermediate_start = Some(now);
                                }
                                pending_intermediate = Some(asr_result.text.clone());

                                // Track stability of suffix to trigger earlier translation
                                if !last_intermediate_text.is_empty() {
                                    let suffix_len =
                                        common_suffix_len(&last_intermediate_text, &asr_result.text);
                                    if suffix_len >= STABLE_SUFFIX_MIN {
                                        stable_suffix_count += 1;
                                    } else {
                                        stable_suffix_count = 0;
                                        last_change_at = Some(now);
                                    }
                                } else {
                                    last_change_at = Some(now);
                                }
                                last_intermediate_text = asr_result.text.clone();

                                // If stable for a short time, trigger translation early
                                let do_translate = app_result
                                    .state::<AppState>()
                                    .translation_enabled
                                    .lock()
                                    .map(|v| *v)
                                    .unwrap_or(false);
                                if do_translate
                                    && stable_suffix_count >= STABLE_COUNT_MIN
                                    && last_intermediate_text != last_translated_text
                                {
                                    let stable_ok = last_change_at
                                        .map(|t| {
                                            now.duration_since(t)
                                                >= std::time::Duration::from_millis(STABLE_WAIT_MILLIS)
                                        })
                                        .unwrap_or(false);
                                    let final_ok = last_final_at
                                        .map(|t| {
                                            now.duration_since(t)
                                                >= std::time::Duration::from_millis(1200)
                                        })
                                        .unwrap_or(true);
                                    let interval_ok = last_intermediate_sent_at
                                        .map(|t| {
                                            now.duration_since(t)
                                                >= std::time::Duration::from_millis(1500)
                                        })
                                        .unwrap_or(true);
                                    if stable_ok
                                        && final_ok
                                        && interval_ok
                                        && can_trigger(now, &last_translate_request_at)
                                    {
                                        let text = last_intermediate_text.clone();
                                        last_translated_text = text.clone();
                                        let req_id = latest_req_id
                                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                            + 1;
                                        let _ = translate_tx.send(Some((req_id, text)));
                                        last_intermediate_sent_at = Some(now);
                                        last_translate_request_at = Some(now);
                                    }
                                }
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
                            let req_id = latest_req_id
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                + 1;
                            let _ = translate_tx.send(Some((req_id, text)));
                            last_translate_request_at = Some(tokio::time::Instant::now());
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
fn list_audio_apps() -> Result<Vec<AudioApp>, String> {
    audio::list_audio_apps()
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
            list_audio_apps,
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
