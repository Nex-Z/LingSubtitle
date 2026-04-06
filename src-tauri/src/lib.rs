mod asr;
mod audio;
mod config;
mod gummy;
mod subtitle;

use std::collections::BTreeMap;
use std::sync::Mutex;

use asr::{AsrSegmentUpdate, GummyConnectivityResult, GummyErrorInfo};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, watch};

use crate::audio::AudioCapture;
use crate::config::{load_config, AppConfig};
use crate::gummy::GummyCapabilities;
use crate::subtitle::{SubtitleEntry, SubtitleManager};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum SegmentState {
    Streaming,
    Final,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum TranslationStatus {
    Idle,
    Streaming,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleSegmentPayload {
    segment_id: u64,
    session_id: u64,
    timestamp: String,
    original_text: String,
    translated_text: Option<String>,
    translated_draft_text: Option<String>,
    state: SegmentState,
    is_final: bool,
    revision: u32,
    translation_error: bool,
    translation_status: TranslationStatus,
    translation_started_at: Option<i64>,
    translation_finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleErrorPayload {
    scope: String,
    message: String,
    segment_id: Option<u64>,
    error_kind: Option<String>,
    provider: Option<String>,
    resolved_url: Option<String>,
}

#[derive(Debug, Clone)]
struct SegmentRecord {
    segment_id: u64,
    session_id: u64,
    timestamp: String,
    original_text: String,
    translated_text: Option<String>,
    translated_draft_text: Option<String>,
    state: SegmentState,
    revision: u32,
    translation_error: bool,
    translation_status: TranslationStatus,
    translation_started_at: Option<i64>,
    translation_finished_at: Option<i64>,
    saved_final: bool,
}

impl SegmentRecord {
    fn to_payload(&self) -> SubtitleSegmentPayload {
        SubtitleSegmentPayload {
            segment_id: self.segment_id,
            session_id: self.session_id,
            timestamp: self.timestamp.clone(),
            original_text: self.original_text.clone(),
            translated_text: self.translated_text.clone(),
            translated_draft_text: self.translated_draft_text.clone(),
            state: self.state,
            is_final: self.state == SegmentState::Final,
            revision: self.revision,
            translation_error: self.translation_error,
            translation_status: self.translation_status,
            translation_started_at: self.translation_started_at,
            translation_finished_at: self.translation_finished_at,
        }
    }
}

#[derive(Debug)]
struct SubtitleSession {
    session_id: u64,
    segments: BTreeMap<u64, SegmentRecord>,
}

impl SubtitleSession {
    fn new(session_id: u64) -> Self {
        Self {
            session_id,
            segments: BTreeMap::new(),
        }
    }
}

struct SubtitleState {
    current: SubtitleSession,
}

impl SubtitleState {
    fn new() -> Self {
        Self {
            current: SubtitleSession::new(1),
        }
    }
}

#[derive(Default)]
struct CaptureRuntime {
    audio_capture: Option<AudioCapture>,
    stop_tx: Option<watch::Sender<bool>>,
    active_session_id: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GummyConfigCheckResult {
    ready: bool,
    message: String,
}

#[derive(Debug, Clone)]
struct SessionConfigSnapshot {
    translation_enabled: bool,
    auto_save: bool,
    asr_base_url: String,
}

fn now_unix_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn subtitle_timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn emit_segment_upsert(app: &AppHandle, segment: &SegmentRecord) {
    let _ = app.emit("subtitle-segment-upsert", segment.to_payload());
}

fn emit_legacy_original(app: &AppHandle, text: &str) {
    let _ = app.emit("subtitle-original", text.to_string());
}

fn emit_legacy_translated(app: &AppHandle, text: &str) {
    let _ = app.emit("subtitle-translated", text.to_string());
}

fn emit_subtitle_error(
    app: &AppHandle,
    scope: &str,
    message: impl Into<String>,
    segment_id: Option<u64>,
    error_kind: Option<String>,
    provider: Option<String>,
    resolved_url: Option<String>,
) {
    let _ = app.emit(
        "subtitle-error",
        SubtitleErrorPayload {
            scope: scope.to_string(),
            message: message.into(),
            segment_id,
            error_kind,
            provider,
            resolved_url,
        },
    );
}

fn stop_capture_runtime(app: &AppHandle, recording_session_id: u64) {
    let state = app.state::<Mutex<CaptureRuntime>>();
    let mut capture_state = match state.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if capture_state.active_session_id != Some(recording_session_id) {
        return;
    }

    if let Some(stop_tx) = capture_state.stop_tx.take() {
        let _ = stop_tx.send(true);
    }
    if let Some(audio_capture) = capture_state.audio_capture.as_mut() {
        audio_capture.stop();
    }
    capture_state.audio_capture = None;
    capture_state.active_session_id = None;
}

fn is_active_capture_session(app: &AppHandle, recording_session_id: u64) -> bool {
    let state = app.state::<Mutex<CaptureRuntime>>();
    let is_active = match state.lock() {
        Ok(capture_state) => capture_state.active_session_id == Some(recording_session_id),
        Err(_) => false,
    };
    is_active
}

fn process_asr_result(
    app: &AppHandle,
    session_config: &SessionConfigSnapshot,
    subtitle_state: &mut SubtitleState,
    subtitle_manager: &Mutex<SubtitleManager>,
    recording_session_id: u64,
    update: AsrSegmentUpdate,
) {
    if subtitle_state.current.session_id != recording_session_id {
        return;
    }

    if update.original_text.trim().is_empty()
        && update
            .translated_text
            .as_ref()
            .map(|text| text.trim().is_empty())
            .unwrap_or(true)
    {
        return;
    }

    let session = &mut subtitle_state.current;
    let segment_id = update.sentence_id;
    let now = now_unix_ms();
    let mut save_entry = None;
    let mut translation_failure = None;

    {
        let segment = session.segments.entry(segment_id).or_insert_with(|| SegmentRecord {
            segment_id,
            session_id: session.session_id,
            timestamp: subtitle_timestamp(),
            original_text: String::new(),
            translated_text: None,
            translated_draft_text: None,
            state: SegmentState::Streaming,
            revision: 0,
            translation_error: false,
            translation_status: TranslationStatus::Idle,
            translation_started_at: None,
            translation_finished_at: None,
            saved_final: false,
        });

        segment.original_text = update.original_text.trim().to_string();
        segment.state = if update.is_final {
            SegmentState::Final
        } else {
            SegmentState::Streaming
        };
        segment.revision = segment.revision.saturating_add(1);

        if session_config.translation_enabled {
            match update
                .translated_text
                .as_ref()
                .map(|text| text.trim())
                .filter(|text| !text.is_empty())
            {
                Some(translated_text) if update.is_final => {
                    if segment.translation_started_at.is_none() {
                        segment.translation_started_at = Some(now);
                    }
                    segment.translated_text = Some(translated_text.to_string());
                    segment.translated_draft_text = None;
                    segment.translation_status = TranslationStatus::Completed;
                    segment.translation_error = false;
                    segment.translation_finished_at = Some(now);
                }
                Some(translated_text) => {
                    if segment.translation_started_at.is_none() {
                        segment.translation_started_at = Some(now);
                    }
                    segment.translated_text = None;
                    segment.translated_draft_text = Some(translated_text.to_string());
                    segment.translation_status = TranslationStatus::Streaming;
                    segment.translation_error = false;
                    segment.translation_finished_at = None;
                }
                None if update.is_final => {
                    segment.translated_text = None;
                    segment.translated_draft_text = None;
                    segment.translation_status = TranslationStatus::Failed;
                    segment.translation_error = true;
                    segment.translation_finished_at = Some(now);
                    translation_failure = Some(
                        "Gummy 已返回最终句，但没有给出对应译文，请检查语言对或服务端任务状态。"
                            .to_string(),
                    );
                }
                None => {
                    segment.translated_text = None;
                    segment.translated_draft_text = None;
                    segment.translation_status = TranslationStatus::Idle;
                    segment.translation_error = false;
                    segment.translation_finished_at = None;
                }
            }
        } else {
            segment.translated_text = None;
            segment.translated_draft_text = None;
            segment.translation_status = TranslationStatus::Idle;
            segment.translation_error = false;
            segment.translation_started_at = None;
            segment.translation_finished_at = None;
        }

        emit_segment_upsert(app, segment);

        if update.is_final {
            emit_legacy_original(app, &segment.original_text);
            if let Some(translated_text) = segment.translated_text.as_deref() {
                emit_legacy_translated(app, translated_text);
            }

            if session_config.auto_save && !segment.saved_final {
                save_entry = Some(SubtitleEntry {
                    timestamp: segment.timestamp.clone(),
                    original: segment.original_text.clone(),
                    translated: segment.translated_text.clone(),
                });
                segment.saved_final = true;
            }
        }
    }

    if let Some(message) = translation_failure {
        emit_subtitle_error(
            app,
            "translation",
            message,
            Some(segment_id),
            Some("missing_final_translation".to_string()),
            Some("dashscopeGummy".to_string()),
            Some(session_config.asr_base_url.clone()),
        );
    }

    if let Some(entry) = save_entry {
        if let Ok(manager) = subtitle_manager.lock() {
            let _ = manager.save_entry(&entry);
        }
    }
}

#[tauri::command]
fn start_capture(
    app: AppHandle,
    capture_state: tauri::State<'_, Mutex<CaptureRuntime>>,
) -> Result<(), String> {
    let config_state = app.state::<Mutex<AppConfig>>();
    let config = config_state.lock().map_err(|e| e.to_string())?.clone();
    asr::validate_runtime_config(&config.asr, &config.translation)
        .map_err(|error| error.message.clone())?;

    let recording_session_id = {
        let subtitle_state = app.state::<Mutex<SubtitleState>>();
        let mut subtitle_state = subtitle_state.lock().map_err(|e| e.to_string())?;
        let next_session_id = subtitle_state.current.session_id + 1;
        subtitle_state.current = SubtitleSession::new(next_session_id);
        next_session_id
    };

    {
        let subtitle_manager = app.state::<Mutex<SubtitleManager>>();
        let mut manager = subtitle_manager.lock().map_err(|e| e.to_string())?;
        *manager = SubtitleManager::new(&config.save.save_path);
        manager.start_new_session();
    }

    let mut audio_capture = AudioCapture::new();
    let (audio_tx, audio_rx) = mpsc::unbounded_channel();
    let (result_tx, mut result_rx) = mpsc::unbounded_channel();
    let (stop_tx, stop_rx) = watch::channel(false);
    let session_config = SessionConfigSnapshot {
        translation_enabled: config.translation.enabled,
        auto_save: config.save.auto_save,
        asr_base_url: config.asr.base_url.clone(),
    };

    let process_id = if config.capture.source == "app" {
        config.capture.app_pid
    } else {
        None
    };

    audio_capture.start(audio_tx, process_id)?;

    {
        let mut capture_state = capture_state.lock().map_err(|e| e.to_string())?;
        if let Some(existing_stop_tx) = capture_state.stop_tx.take() {
            let _ = existing_stop_tx.send(true);
        }
        if let Some(existing_capture) = capture_state.audio_capture.as_mut() {
            existing_capture.stop();
        }
        capture_state.audio_capture = Some(audio_capture);
        capture_state.stop_tx = Some(stop_tx);
        capture_state.active_session_id = Some(recording_session_id);
    }

    let app_handle = app.clone();
    let asr_config = config.asr.clone();
    let translation_config = config.translation.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = asr::run_asr_session(
            asr_config,
            translation_config,
            audio_rx,
            result_tx,
            stop_rx,
        )
        .await
        {
            if !is_active_capture_session(&app_handle, recording_session_id) {
                return;
            }
            emit_asr_failure(&app_handle, err);
            stop_capture_runtime(&app_handle, recording_session_id);
        }
    });

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(asr_result) = result_rx.recv().await {
            let subtitle_state = app_handle.state::<Mutex<SubtitleState>>();
            let mut subtitle_state = match subtitle_state.lock() {
                Ok(state) => state,
                Err(err) => {
                    eprintln!("Failed to lock subtitle state during ASR processing: {}", err);
                    continue;
                }
            };
            let subtitle_manager = app_handle.state::<Mutex<SubtitleManager>>();
            process_asr_result(
                &app_handle,
                &session_config,
                &mut subtitle_state,
                &subtitle_manager,
                recording_session_id,
                asr_result,
            );
        }
    });
    Ok(())
}

#[tauri::command]
fn stop_capture(
    capture_state: tauri::State<'_, Mutex<CaptureRuntime>>,
) -> Result<(), String> {
    let mut capture_state = capture_state.lock().map_err(|e| e.to_string())?;
    if let Some(stop_tx) = capture_state.stop_tx.take() {
        let _ = stop_tx.send(true);
    }
    if let Some(audio_capture) = capture_state.audio_capture.as_mut() {
        audio_capture.stop();
    }
    capture_state.audio_capture = None;
    capture_state.active_session_id = None;
    Ok(())
}

#[tauri::command]
fn get_capture_status(
    capture_state: tauri::State<'_, Mutex<CaptureRuntime>>,
) -> Result<bool, String> {
    let capture_state = capture_state.lock().map_err(|e| e.to_string())?;
    Ok(capture_state
        .audio_capture
        .as_ref()
        .map(|capture| capture.is_running())
        .unwrap_or(false))
}

#[tauri::command]
fn set_translation_enabled(
    config_state: tauri::State<'_, Mutex<AppConfig>>,
    enabled: bool,
) -> Result<(), String> {
    let mut config = config_state.lock().map_err(|e| e.to_string())?;
    config.translation.enabled = enabled;
    Ok(())
}

#[tauri::command]
fn get_gummy_capabilities() -> Result<GummyCapabilities, String> {
    Ok(gummy::capabilities())
}

#[tauri::command]
fn check_gummy_config(
    config_state: tauri::State<'_, Mutex<AppConfig>>,
) -> Result<GummyConfigCheckResult, String> {
    let config = config_state.lock().map_err(|e| e.to_string())?.clone();
    match asr::validate_runtime_config(&config.asr, &config.translation) {
        Ok(()) => Ok(GummyConfigCheckResult {
            ready: true,
            message: "ok".to_string(),
        }),
        Err(error) => Ok(GummyConfigCheckResult {
            ready: false,
            message: error.message,
        }),
    }
}

#[tauri::command]
async fn check_gummy_connectivity(
    config_state: tauri::State<'_, Mutex<AppConfig>>,
) -> Result<GummyConnectivityResult, String> {
    let config = config_state.lock().map_err(|e| e.to_string())?.clone();
    Ok(asr::check_connectivity(&config.asr, &config.translation).await)
}

#[tauri::command]
fn list_audio_apps() -> Result<Vec<audio::AudioApp>, String> {
    audio::list_audio_apps()
}

#[tauri::command]
async fn open_floating_window(app: AppHandle) -> Result<(), String> {
    use tauri::WebviewWindowBuilder;

    if app.get_webview_window("subtitle").is_some() {
        return Ok(());
    }

    let monitor = app
        .primary_monitor()
        .ok()
        .flatten()
        .ok_or("无法获取主显示器尺寸")?;
    let scale_factor = monitor.scale_factor();
    let monitor_size = monitor.size();
    let screen_w = monitor_size.width as f64 / scale_factor;
    let screen_h = monitor_size.height as f64 / scale_factor;

    let win_w = 600.0;
    let win_h = 100.0;
    let x = (screen_w - win_w) / 2.0;
    let y = (screen_h * 2.0 / 3.0) - (win_h / 2.0);

    WebviewWindowBuilder::new(
        &app,
        "subtitle",
        tauri::WebviewUrl::App("floating.html".into()),
    )
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
async fn close_floating_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("subtitle") {
        window
            .close()
            .map_err(|e| format!("Failed to close floating window: {}", e))?;
    }
    Ok(())
}

fn emit_asr_failure(app: &AppHandle, err: GummyErrorInfo) {
    emit_subtitle_error(
        app,
        "asr",
        err.message,
        None,
        Some(err.kind),
        Some(err.provider),
        Some(err.resolved_url),
    );
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let config = load_config(&app_handle);
            app.manage(Mutex::new(config.clone()));
            app.manage(Mutex::new(CaptureRuntime::default()));
            app.manage(Mutex::new(SubtitleState::new()));
            app.manage(Mutex::new(SubtitleManager::new(&config.save.save_path)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            config::get_config,
            config::save_config,
            get_gummy_capabilities,
            list_audio_apps,
            start_capture,
            stop_capture,
            get_capture_status,
            set_translation_enabled,
            check_gummy_config,
            check_gummy_connectivity,
            open_floating_window,
            close_floating_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
