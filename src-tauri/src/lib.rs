mod asr;
mod audio;
mod config;
mod subtitle;
mod translation;

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use asr::AsrResult;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, watch};

use crate::audio::AudioCapture;
use crate::config::{load_config, AppConfig};
use crate::subtitle::{SubtitleEntry, SubtitleManager};
use crate::translation::{
    check_connectivity, translate, translate_stream, TranslationConnectivityResult,
    TranslationErrorInfo, TranslationRequestContext,
};

const STABLE_WINDOW: Duration = Duration::from_millis(500);
const SOFT_SEGMENT_MIN_CHARS: usize = 12;
const SOFT_SEGMENT_FORCE_CHARS: usize = 32;
const SOFT_SEGMENT_TIMEOUT: Duration = Duration::from_millis(1500);
const TRANSLATE_MIN_CHARS: usize = 6;
const PREVIEW_COOLDOWN: Duration = Duration::from_millis(800);

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum SegmentState {
    Streaming,
    StablePreview,
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
struct SubtitleTranslationStartedPayload {
    segment_id: u64,
    revision: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleTranslationDeltaPayload {
    segment_id: u64,
    revision: u32,
    delta_text: String,
    accumulated_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleTranslationFinishedPayload {
    segment_id: u64,
    revision: u32,
    translated_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleTranslationFailedPayload {
    segment_id: u64,
    revision: u32,
    message: String,
    error_kind: Option<String>,
    provider: Option<String>,
    resolved_url: Option<String>,
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
    updated_at: Instant,
    translation_error: bool,
    translation_status: TranslationStatus,
    translation_started_at: Option<i64>,
    translation_finished_at: Option<i64>,
    translate_revision: Option<u32>,
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
    next_segment_id: u64,
    segments: BTreeMap<u64, SegmentRecord>,
    active_segment_id: Option<u64>,
    committed_prefix_text: String,
    latest_asr_text: String,
    last_active_text: String,
    stable_since: Option<Instant>,
    soft_commit_backoff_until: Option<Instant>,
    last_preview_scheduled_at: Option<Instant>,
}

impl SubtitleSession {
    fn new(session_id: u64) -> Self {
        Self {
            session_id,
            next_segment_id: 1,
            segments: BTreeMap::new(),
            active_segment_id: None,
            committed_prefix_text: String::new(),
            latest_asr_text: String::new(),
            last_active_text: String::new(),
            stable_since: None,
            soft_commit_backoff_until: None,
            last_preview_scheduled_at: None,
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
}

#[derive(Debug, Clone)]
struct TranslateJob {
    segment_id: u64,
    revision: u32,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TranslationPriority {
    Preview,
    Final,
}

#[derive(Debug, Clone)]
enum TranslationWorkerEvent {
    Started {
        segment_id: u64,
        revision: u32,
    },
    Delta {
        segment_id: u64,
        revision: u32,
        delta_text: String,
        accumulated_text: String,
    },
    Finished {
        segment_id: u64,
        revision: u32,
        translated_text: String,
    },
    Failed {
        segment_id: u64,
        revision: u32,
        error: TranslationErrorInfo,
    },
}

struct TranslationQueueState {
    sender: mpsc::UnboundedSender<TranslateJob>,
}

fn now_unix_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn subtitle_timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end.saturating_sub(start)).collect()
}

fn slice_from(text: &str, start: usize) -> String {
    text.chars().skip(start).collect()
}

fn common_prefix_chars(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(l, r)| l == r)
        .count()
}

fn find_commit_boundary(text: &str, stable_for: Duration) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < SOFT_SEGMENT_MIN_CHARS {
        return None;
    }

    for (idx, ch) in chars.iter().enumerate().rev() {
        if "，。！？；：,.!?;:".contains(*ch) && idx + 1 >= SOFT_SEGMENT_MIN_CHARS {
            return Some(idx + 1);
        }
    }

    if chars.len() >= SOFT_SEGMENT_FORCE_CHARS && stable_for >= SOFT_SEGMENT_TIMEOUT {
        return Some(SOFT_SEGMENT_FORCE_CHARS.min(chars.len()));
    }

    None
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

fn ensure_active_segment(session: &mut SubtitleSession) -> u64 {
    if let Some(segment_id) = session.active_segment_id {
        return segment_id;
    }

    let segment_id = session.next_segment_id;
    session.next_segment_id += 1;
    let record = SegmentRecord {
        segment_id,
        session_id: session.session_id,
        timestamp: subtitle_timestamp(),
        original_text: String::new(),
        translated_text: None,
        translated_draft_text: None,
        state: SegmentState::Streaming,
        revision: 1,
        updated_at: Instant::now(),
        translation_error: false,
        translation_status: TranslationStatus::Idle,
        translation_started_at: None,
        translation_finished_at: None,
        translate_revision: None,
    };
    session.segments.insert(segment_id, record);
    session.active_segment_id = Some(segment_id);
    segment_id
}

fn maybe_queue_translation(
    app: &AppHandle,
    queue: &TranslationQueueState,
    config: &AppConfig,
    session: &mut SubtitleSession,
    segment_id: u64,
    priority: TranslationPriority,
) {
    if !config.translation.enabled {
        return;
    }

    let now = Instant::now();
    let segment = match session.segments.get_mut(&segment_id) {
        Some(segment) => segment,
        None => return,
    };

    if char_count(&segment.original_text) < TRANSLATE_MIN_CHARS {
        return;
    }

    if segment.translate_revision == Some(segment.revision) {
        return;
    }

    if priority == TranslationPriority::Preview {
        if let Some(last_preview) = session.last_preview_scheduled_at {
            if now.duration_since(last_preview) < PREVIEW_COOLDOWN {
                return;
            }
        }
        session.last_preview_scheduled_at = Some(now);
    }

    segment.translate_revision = Some(segment.revision);
    segment.translation_error = false;
    segment.translated_draft_text = None;
    segment.translation_status = TranslationStatus::Idle;
    segment.translation_started_at = None;
    segment.translation_finished_at = None;

    let _ = queue.sender.send(TranslateJob {
        segment_id,
        revision: segment.revision,
        text: segment.original_text.clone(),
    });
    emit_segment_upsert(app, segment);
}

fn commit_active_prefix(
    app: &AppHandle,
    config: &AppConfig,
    subtitle_state: &mut SubtitleState,
    queue: &TranslationQueueState,
    boundary: usize,
) {
    let session = &mut subtitle_state.current;
    let active_segment_id = match session.active_segment_id {
        Some(id) => id,
        None => return,
    };

    let current_text = match session.segments.get(&active_segment_id) {
        Some(segment) => segment.original_text.clone(),
        None => return,
    };

    let current_len = char_count(&current_text);
    if boundary == 0 || boundary >= current_len {
        return;
    }

    let committed_part = slice_chars(&current_text, 0, boundary);
    let remaining_part = slice_from(&current_text, boundary);
    if committed_part.is_empty() {
        return;
    }

    if let Some(segment) = session.segments.get_mut(&active_segment_id) {
        segment.original_text = committed_part.clone();
        segment.state = SegmentState::StablePreview;
        segment.revision += 1;
        segment.updated_at = Instant::now();
        segment.translated_text = None;
        segment.translated_draft_text = None;
        segment.translation_status = TranslationStatus::Idle;
        segment.translation_started_at = None;
        segment.translation_finished_at = None;
        emit_segment_upsert(app, segment);
    }

    maybe_queue_translation(
        app,
        queue,
        config,
        session,
        active_segment_id,
        TranslationPriority::Preview,
    );

    session.committed_prefix_text.push_str(&committed_part);
    session.active_segment_id = None;
    session.last_active_text = remaining_part.clone();
    session.stable_since = Some(Instant::now());

    if !remaining_part.is_empty() {
        let new_segment_id = ensure_active_segment(session);
        if let Some(segment) = session.segments.get_mut(&new_segment_id) {
            segment.original_text = remaining_part;
            segment.updated_at = Instant::now();
            segment.revision += 1;
            emit_segment_upsert(app, segment);
        }
    }
}

fn finalize_active_segment(
    app: &AppHandle,
    config: &AppConfig,
    subtitle_state: &mut SubtitleState,
    queue: &TranslationQueueState,
    subtitle_manager: &Mutex<SubtitleManager>,
    final_text: String,
) {
    let session = &mut subtitle_state.current;
    if final_text.is_empty() {
        session.active_segment_id = None;
        session.committed_prefix_text.clear();
        session.latest_asr_text.clear();
        session.last_active_text.clear();
        session.stable_since = None;
        session.soft_commit_backoff_until = None;
        return;
    }

    let active_segment_id = ensure_active_segment(session);
    if let Some(segment) = session.segments.get_mut(&active_segment_id) {
        segment.original_text = final_text.clone();
        segment.state = SegmentState::Final;
        segment.revision += 1;
        segment.updated_at = Instant::now();
        emit_segment_upsert(app, segment);
    }
    emit_legacy_original(app, &final_text);

    maybe_queue_translation(
        app,
        queue,
        config,
        session,
        active_segment_id,
        TranslationPriority::Final,
    );

    if let Ok(manager) = subtitle_manager.lock() {
        let _ = manager.save_entry(&SubtitleEntry {
            timestamp: subtitle_timestamp(),
            original: final_text.clone(),
            translated: None,
        });
    }

    session.active_segment_id = None;
    session.committed_prefix_text.clear();
    session.latest_asr_text.clear();
    session.last_active_text.clear();
    session.stable_since = None;
    session.soft_commit_backoff_until = None;
}

fn process_asr_result(
    app: &AppHandle,
    config: &AppConfig,
    subtitle_state: &mut SubtitleState,
    queue: &TranslationQueueState,
    subtitle_manager: &Mutex<SubtitleManager>,
    asr_result: AsrResult,
) {
    let session = &mut subtitle_state.current;
    let new_text = asr_result.text.trim().to_string();
    if new_text.is_empty() {
        return;
    }

    session.latest_asr_text = new_text.clone();

    let combined_previous = format!("{}{}", session.committed_prefix_text, session.last_active_text);
    let common = common_prefix_chars(&new_text, &combined_previous);
    let committed_chars = char_count(&session.committed_prefix_text);
    if common < committed_chars {
        session.soft_commit_backoff_until = Some(Instant::now() + STABLE_WINDOW);
    }

    let active_text = if new_text.starts_with(&session.committed_prefix_text) {
        slice_from(&new_text, committed_chars)
    } else {
        new_text.clone()
    };

    if asr_result.is_final {
        finalize_active_segment(
            app,
            config,
            subtitle_state,
            queue,
            subtitle_manager,
            active_text,
        );
        return;
    }

    let active_segment_id = ensure_active_segment(session);
    let now = Instant::now();
    if active_text == session.last_active_text {
        if session.stable_since.is_none() {
            session.stable_since = Some(now);
        }
    } else {
        session.last_active_text = active_text.clone();
        session.stable_since = Some(now);
    }

    if let Some(segment) = session.segments.get_mut(&active_segment_id) {
        segment.original_text = active_text.clone();
        segment.state = SegmentState::Streaming;
        segment.revision += 1;
        segment.updated_at = now;
        segment.translated_text = None;
        segment.translated_draft_text = None;
        segment.translation_status = TranslationStatus::Idle;
        segment.translation_started_at = None;
        segment.translation_finished_at = None;
        emit_segment_upsert(app, segment);
    }
    emit_legacy_original(app, &active_text);

    let stable_for = session
        .stable_since
        .map(|instant| now.saturating_duration_since(instant))
        .unwrap_or_default();
    let in_backoff = session
        .soft_commit_backoff_until
        .map(|deadline| deadline > now)
        .unwrap_or(false);

    if !in_backoff && stable_for >= STABLE_WINDOW {
        if let Some(boundary) = find_commit_boundary(&active_text, stable_for) {
            commit_active_prefix(app, config, subtitle_state, queue, boundary);
        }
    }
}

async fn apply_translation_event(app: &AppHandle, event: TranslationWorkerEvent) {
    let subtitle_state = app.state::<Mutex<SubtitleState>>();
    let mut subtitle_state = match subtitle_state.lock() {
        Ok(state) => state,
        Err(err) => {
            eprintln!("Failed to lock subtitle state: {}", err);
            return;
        }
    };
    let session = &mut subtitle_state.current;

    match event {
        TranslationWorkerEvent::Started {
            segment_id,
            revision,
        } => {
            if let Some(segment) = session.segments.get_mut(&segment_id) {
                if segment.revision != revision {
                    return;
                }
                segment.translation_status = TranslationStatus::Streaming;
                segment.translation_error = false;
                segment.translated_draft_text = Some(String::new());
                segment.translation_started_at = Some(now_unix_ms());
                segment.translation_finished_at = None;
                emit_segment_upsert(app, segment);
                let _ = app.emit(
                    "subtitle-segment-translation-started",
                    SubtitleTranslationStartedPayload { segment_id, revision },
                );
            }
        }
        TranslationWorkerEvent::Delta {
            segment_id,
            revision,
            delta_text,
            accumulated_text,
        } => {
            if let Some(segment) = session.segments.get_mut(&segment_id) {
                if segment.revision != revision {
                    return;
                }
                segment.translation_status = TranslationStatus::Streaming;
                segment.translation_error = false;
                segment.translated_draft_text = Some(accumulated_text.clone());
                emit_segment_upsert(app, segment);
                let _ = app.emit(
                    "subtitle-segment-translation-delta",
                    SubtitleTranslationDeltaPayload {
                        segment_id,
                        revision,
                        delta_text,
                        accumulated_text,
                    },
                );
            }
        }
        TranslationWorkerEvent::Finished {
            segment_id,
            revision,
            translated_text,
        } => {
            if let Some(segment) = session.segments.get_mut(&segment_id) {
                if segment.revision != revision {
                    return;
                }
                segment.translation_status = TranslationStatus::Completed;
                segment.translation_error = false;
                segment.translated_text = Some(translated_text.clone());
                segment.translated_draft_text = None;
                segment.translation_finished_at = Some(now_unix_ms());
                emit_segment_upsert(app, segment);
                let payload = SubtitleTranslationFinishedPayload {
                    segment_id,
                    revision,
                    translated_text,
                };
                emit_legacy_translated(app, &payload.translated_text);
                let _ = app.emit("subtitle-segment-translation-finished", payload.clone());
                let _ = app.emit("subtitle-segment-translated", payload);
            }
        }
        TranslationWorkerEvent::Failed {
            segment_id,
            revision,
            error,
        } => {
            if let Some(segment) = session.segments.get_mut(&segment_id) {
                if segment.revision != revision {
                    return;
                }
                segment.translation_status = TranslationStatus::Failed;
                segment.translation_error = true;
                segment.translated_draft_text = None;
                segment.translation_finished_at = Some(now_unix_ms());
                emit_segment_upsert(app, segment);
                emit_subtitle_error(
                    app,
                    "translation",
                    error.message.clone(),
                    Some(segment_id),
                    Some(error.kind.clone()),
                    Some(error.provider.clone()),
                    Some(error.resolved_url.clone()),
                );
                let _ = app.emit(
                    "subtitle-segment-translation-failed",
                    SubtitleTranslationFailedPayload {
                        segment_id,
                        revision,
                        message: error.message,
                        error_kind: Some(error.kind),
                        provider: Some(error.provider),
                        resolved_url: Some(error.resolved_url),
                    },
                );
            }
        }
    }
}

async fn run_translation_job(
    config: &AppConfig,
    event_tx: &mpsc::UnboundedSender<TranslationWorkerEvent>,
    job: TranslateJob,
) {
    let ctx = TranslationRequestContext {
        segment_id: Some(job.segment_id),
        revision: Some(job.revision),
    };
    let event_tx_stream = event_tx.clone();

    match translate_stream(&config.translation, &job.text, Some(&ctx), move |delta_text, accumulated_text| {
        let _ = event_tx_stream.send(TranslationWorkerEvent::Delta {
            segment_id: job.segment_id,
            revision: job.revision,
            delta_text,
            accumulated_text,
        });
    })
    .await
    {
        Ok(translated_text) => {
            let _ = event_tx.send(TranslationWorkerEvent::Finished {
                segment_id: job.segment_id,
                revision: job.revision,
                translated_text,
            });
        }
        Err(stream_error) => {
            eprintln!(
                "[translate] segment={} revision={} stream_failed={} fallback=once",
                job.segment_id, job.revision, stream_error.message
            );
            match translate(&config.translation, &job.text, Some(&ctx)).await {
                Ok(translated_text) => {
                    let _ = event_tx.send(TranslationWorkerEvent::Finished {
                        segment_id: job.segment_id,
                        revision: job.revision,
                        translated_text,
                    });
                }
                Err(error) => {
                    let _ = event_tx.send(TranslationWorkerEvent::Failed {
                        segment_id: job.segment_id,
                        revision: job.revision,
                        error,
                    });
                }
            }
        }
    }
}

async fn translation_worker_loop(
    app: AppHandle,
    mut job_rx: mpsc::UnboundedReceiver<TranslateJob>,
) {
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TranslationWorkerEvent>();
    let app_for_events = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            apply_translation_event(&app_for_events, event).await;
        }
    });

    while let Some(job) = job_rx.recv().await {
        let _ = event_tx.send(TranslationWorkerEvent::Started {
            segment_id: job.segment_id,
            revision: job.revision,
        });
        let config_state = app.state::<Mutex<AppConfig>>();
        let config = match config_state.lock() {
            Ok(cfg) => cfg.clone(),
            Err(err) => {
                eprintln!("Failed to lock config for translation: {}", err);
                continue;
            }
        };
        run_translation_job(&config, &event_tx, job).await;
    }
}

#[tauri::command]
fn start_capture(
    app: AppHandle,
    capture_state: tauri::State<'_, Mutex<CaptureRuntime>>,
) -> Result<(), String> {
    let config_state = app.state::<Mutex<AppConfig>>();
    let config = config_state.lock().map_err(|e| e.to_string())?.clone();

    {
        let subtitle_state = app.state::<Mutex<SubtitleState>>();
        let mut subtitle_state = subtitle_state.lock().map_err(|e| e.to_string())?;
        let next_session_id = subtitle_state.current.session_id + 1;
        subtitle_state.current = SubtitleSession::new(next_session_id);
    }

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

    let process_id = if config.capture.source == "app" {
        config.capture.app_pid
    } else {
        None
    };

    audio_capture.start(audio_tx, process_id)?;

    let asr_config = config.asr.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = asr::run_asr_session(asr_config, audio_rx, result_tx, stop_rx).await {
            eprintln!("[ASR] session failed: {}", err);
        }
    });

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(asr_result) = result_rx.recv().await {
            let config_state = app_handle.state::<Mutex<AppConfig>>();
            let config = match config_state.lock() {
                Ok(cfg) => cfg.clone(),
                Err(err) => {
                    eprintln!("Failed to lock config during ASR processing: {}", err);
                    continue;
                }
            };
            let subtitle_state = app_handle.state::<Mutex<SubtitleState>>();
            let mut subtitle_state = match subtitle_state.lock() {
                Ok(state) => state,
                Err(err) => {
                    eprintln!("Failed to lock subtitle state during ASR processing: {}", err);
                    continue;
                }
            };
            let translation_queue = app_handle.state::<TranslationQueueState>();
            let subtitle_manager = app_handle.state::<Mutex<SubtitleManager>>();
            process_asr_result(
                &app_handle,
                &config,
                &mut subtitle_state,
                &translation_queue,
                &subtitle_manager,
                asr_result,
            );
        }
    });

    let mut capture_state = capture_state.lock().map_err(|e| e.to_string())?;
    if let Some(existing) = capture_state.audio_capture.as_mut() {
        existing.stop();
    }
    capture_state.audio_capture = Some(audio_capture);
    capture_state.stop_tx = Some(stop_tx);
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslationConfigCheckResult {
    ready: bool,
    message: String,
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
fn check_translation_config(
    config_state: tauri::State<'_, Mutex<AppConfig>>,
) -> Result<TranslationConfigCheckResult, String> {
    let config = config_state.lock().map_err(|e| e.to_string())?;
    let translation = &config.translation;

    let mut missing = Vec::new();
    if translation.base_url.trim().is_empty() {
        missing.push("Base URL");
    }
    if translation.api_key.trim().is_empty() {
        missing.push("API Key");
    }
    if translation.model.trim().is_empty() {
        missing.push("Model");
    }

    if missing.is_empty() {
        Ok(TranslationConfigCheckResult {
            ready: true,
            message: "ok".to_string(),
        })
    } else {
        Ok(TranslationConfigCheckResult {
            ready: false,
            message: format!("请先配置翻译服务：{}", missing.join("、")),
        })
    }
}

#[tauri::command]
async fn check_translation_connectivity(
    config_state: tauri::State<'_, Mutex<AppConfig>>,
) -> Result<TranslationConnectivityResult, String> {
    let config = config_state.lock().map_err(|e| e.to_string())?.clone();
    Ok(check_connectivity(&config.translation).await)
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

fn spawn_translation_worker(app: &AppHandle) -> TranslationQueueState {
    let (job_tx, job_rx) = mpsc::unbounded_channel();
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        translation_worker_loop(app_handle, job_rx).await;
    });
    TranslationQueueState { sender: job_tx }
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
            let translation_queue = spawn_translation_worker(&app_handle);
            app.manage(translation_queue);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            config::get_config,
            config::save_config,
            list_audio_apps,
            start_capture,
            stop_capture,
            get_capture_status,
            set_translation_enabled,
            check_translation_config,
            check_translation_connectivity,
            open_floating_window,
            close_floating_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
