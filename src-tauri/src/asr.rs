use std::error::Error;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, HeaderValue};
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use crate::config::{AsrConfig, TranslationConfig};
use crate::gummy::{validate_language_selection, DEFAULT_SAMPLE_RATE};

const PROVIDER: &str = "dashscopeGummy";
const TASK_GROUP: &str = "audio";
const TASK_NAME: &str = "asr";
const TASK_FUNCTION: &str = "recognition";
const BINARY_AUDIO_FRAME_BYTES: usize = (DEFAULT_SAMPLE_RATE as usize * 2) / 10;
const MAX_END_SILENCE_MS: u32 = 6_000;

type GummyWsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrSegmentUpdate {
    pub sentence_id: u64,
    pub begin_time_ms: Option<u64>,
    pub end_time_ms: Option<u64>,
    pub original_text: String,
    pub translated_text: Option<String>,
    pub is_final: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GummyErrorInfo {
    pub kind: String,
    pub message: String,
    pub provider: String,
    pub resolved_url: String,
    pub model: String,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GummyConnectivityResult {
    pub ok: bool,
    pub provider: String,
    pub resolved_url: String,
    pub model: String,
    pub error_kind: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
struct ResolvedGummyTarget {
    resolved_url: String,
}

#[derive(Debug)]
enum ControlEvent {
    TaskStarted,
    TaskFinished,
    TaskFailed(GummyErrorInfo),
}

#[derive(Debug)]
enum ParsedServerEvent {
    Ignore,
    TaskStarted,
    TaskFinished,
    TaskFailed {
        error_code: Option<String>,
        error_message: String,
    },
    ResultGenerated(AsrSegmentUpdate),
}

pub fn validate_runtime_config(
    asr: &AsrConfig,
    translation: &TranslationConfig,
) -> Result<(), GummyErrorInfo> {
    let target = resolve_target(asr)?;

    if asr.api_key.trim().is_empty() {
        return Err(build_error(
            "invalid_config",
            "Gummy API Key 尚未配置。",
            &target,
            &asr.model,
            None,
        ));
    }
    if asr.model.trim().is_empty() {
        return Err(build_error(
            "invalid_config",
            "Gummy 模型名称尚未配置。",
            &target,
            &asr.model,
            None,
        ));
    }
    if asr.sample_rate != DEFAULT_SAMPLE_RATE {
        return Err(build_error(
            "invalid_config",
            format!(
                "当前桌面音频链路固定输出 {}Hz PCM，请将采样率保持为 {}。",
                DEFAULT_SAMPLE_RATE, DEFAULT_SAMPLE_RATE
            ),
            &target,
            &asr.model,
            None,
        ));
    }
    if !(200..=MAX_END_SILENCE_MS).contains(&asr.vad_silence_ms) {
        return Err(build_error(
            "invalid_config",
            format!(
                "句尾静音阈值必须在 200 到 {} 毫秒之间。",
                MAX_END_SILENCE_MS
            ),
            &target,
            &asr.model,
            None,
        ));
    }

    validate_language_selection(
        asr.language.trim(),
        translation.enabled,
        translation.target_language.trim(),
    )
    .map_err(|message| build_error("invalid_language_pair", message, &target, &asr.model, None))?;

    Ok(())
}

pub async fn run_asr_session(
    asr_config: AsrConfig,
    translation_config: TranslationConfig,
    mut audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    result_tx: mpsc::UnboundedSender<AsrSegmentUpdate>,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<(), GummyErrorInfo> {
    validate_runtime_config(&asr_config, &translation_config)?;
    let target = resolve_target(&asr_config)?;
    let task_id = make_task_id();
    let (ws_stream, _) = connect_ws(&target, &asr_config.api_key, &asr_config.model).await?;
    let (mut write, mut read) = ws_stream.split();
    let (control_tx, mut control_rx) = mpsc::unbounded_channel::<ControlEvent>();

    let target_for_reader = target.clone();
    let model_for_reader = asr_config.model.clone();
    let reader = tokio::spawn(async move {
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => match parse_server_message(&text) {
                    Ok(ParsedServerEvent::Ignore) => {}
                    Ok(ParsedServerEvent::TaskStarted) => {
                        let _ = control_tx.send(ControlEvent::TaskStarted);
                    }
                    Ok(ParsedServerEvent::TaskFinished) => {
                        let _ = control_tx.send(ControlEvent::TaskFinished);
                        break;
                    }
                    Ok(ParsedServerEvent::TaskFailed {
                        error_code,
                        error_message,
                    }) => {
                        let _ = control_tx.send(ControlEvent::TaskFailed(build_error(
                            "task_failed",
                            error_message,
                            &target_for_reader,
                            &model_for_reader,
                            error_code,
                        )));
                        break;
                    }
                    Ok(ParsedServerEvent::ResultGenerated(result)) => {
                        let _ = result_tx.send(result);
                    }
                    Err(message) => {
                        let _ = control_tx.send(ControlEvent::TaskFailed(build_error(
                            "protocol_parse",
                            message,
                            &target_for_reader,
                            &model_for_reader,
                            None,
                        )));
                        break;
                    }
                },
                Ok(Message::Close(_)) => {
                    let _ = control_tx.send(ControlEvent::TaskFailed(build_error(
                        "connection_closed",
                        "Gummy WebSocket 在任务结束前被关闭。",
                        &target_for_reader,
                        &model_for_reader,
                        None,
                    )));
                    break;
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Binary(_)) => {}
                Ok(Message::Frame(_)) => {}
                Err(err) => {
                    let _ = control_tx.send(ControlEvent::TaskFailed(classify_socket_error(
                        &err,
                        &target_for_reader,
                        &model_for_reader,
                    )));
                    break;
                }
            }
        }
    });

    let run_task = build_run_task_message(&task_id, &asr_config, &translation_config);
    write
        .send(Message::Text(run_task.into()))
        .await
        .map_err(|err| classify_socket_error(&err, &target, &asr_config.model))?;

    wait_for_task_started(&mut control_rx, &target, &asr_config.model).await?;

    let mut pending_audio = Vec::with_capacity(BINARY_AUDIO_FRAME_BYTES * 2);
    let mut finish_sent = false;

    while !finish_sent {
        tokio::select! {
            control = control_rx.recv() => {
                match control {
                    Some(ControlEvent::TaskStarted) => {}
                    Some(ControlEvent::TaskFinished) => {
                        let _ = write.send(Message::Close(None)).await;
                        reader.abort();
                        return Ok(());
                    }
                    Some(ControlEvent::TaskFailed(err)) => {
                        let _ = write.send(Message::Close(None)).await;
                        reader.abort();
                        return Err(err);
                    }
                    None => {
                        let _ = write.send(Message::Close(None)).await;
                        reader.abort();
                        return Err(build_error(
                            "connection_closed",
                            "Gummy WebSocket 在任务完成前中断。",
                            &target,
                            &asr_config.model,
                            None,
                        ));
                    }
                }
            }
            audio = audio_rx.recv(), if !finish_sent => {
                match audio {
                    Some(chunk) => {
                        pending_audio.extend_from_slice(&chunk);
                        while pending_audio.len() >= BINARY_AUDIO_FRAME_BYTES {
                            let frame = pending_audio.drain(..BINARY_AUDIO_FRAME_BYTES).collect::<Vec<u8>>();
                            write
                                .send(Message::Binary(frame.into()))
                                .await
                                .map_err(|err| classify_socket_error(&err, &target, &asr_config.model))?;
                        }
                    }
                    None => {
                        flush_audio_and_finish(
                            &mut write,
                            &mut pending_audio,
                            &task_id,
                            &target,
                            &asr_config.model,
                        )
                        .await?;
                        finish_sent = true;
                    }
                }
            }
            changed = stop_rx.changed(), if !finish_sent => {
                match changed {
                    Ok(_) if *stop_rx.borrow() => {
                        flush_audio_and_finish(
                            &mut write,
                            &mut pending_audio,
                            &task_id,
                            &target,
                            &asr_config.model,
                        )
                        .await?;
                        finish_sent = true;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        flush_audio_and_finish(
                            &mut write,
                            &mut pending_audio,
                            &task_id,
                            &target,
                            &asr_config.model,
                        )
                        .await?;
                        finish_sent = true;
                    }
                }
            }
        }
    }

    wait_for_task_finished(&mut control_rx, &target, &asr_config.model).await?;

    let _ = write.send(Message::Close(None)).await;
    reader.abort();
    Ok(())
}

pub async fn check_connectivity(
    asr_config: &AsrConfig,
    translation_config: &TranslationConfig,
) -> GummyConnectivityResult {
    let target = match resolve_target(asr_config) {
        Ok(target) => target,
        Err(error) => return connectivity_failure(error),
    };

    if let Err(error) = validate_runtime_config(asr_config, translation_config) {
        return connectivity_failure(error);
    }

    match run_connectivity_probe(asr_config, translation_config, &target).await {
        Ok(()) => GummyConnectivityResult {
            ok: true,
            provider: PROVIDER.to_string(),
            resolved_url: target.resolved_url,
            model: asr_config.model.clone(),
            error_kind: None,
            message: "Gummy 连接检测通过。".to_string(),
        },
        Err(error) => connectivity_failure(error),
    }
}

async fn run_connectivity_probe(
    asr_config: &AsrConfig,
    translation_config: &TranslationConfig,
    target: &ResolvedGummyTarget,
) -> Result<(), GummyErrorInfo> {
    let task_id = make_task_id();
    let (mut ws_stream, _) = connect_ws(target, &asr_config.api_key, &asr_config.model).await?;
    let run_task = build_run_task_message(&task_id, asr_config, translation_config);
    ws_stream
        .send(Message::Text(run_task.into()))
        .await
        .map_err(|err| classify_socket_error(&err, target, &asr_config.model))?;

    let started = timeout(Duration::from_secs(10), async {
        while let Some(message) = ws_stream.next().await {
            match message {
                Ok(Message::Text(text)) => match parse_server_message(&text) {
                    Ok(ParsedServerEvent::TaskStarted) => return Ok(()),
                    Ok(ParsedServerEvent::TaskFailed {
                        error_code,
                        error_message,
                    }) => {
                        return Err(build_error(
                            "task_failed",
                            error_message,
                            target,
                            &asr_config.model,
                            error_code,
                        ))
                    }
                    Ok(_) => {}
                    Err(message) => {
                        return Err(build_error(
                            "protocol_parse",
                            message,
                            target,
                            &asr_config.model,
                            None,
                        ))
                    }
                },
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Binary(_)) => {}
                Ok(Message::Close(_)) => {
                    return Err(build_error(
                        "connection_closed",
                        "Gummy WebSocket 在检测期间被关闭。",
                        target,
                        &asr_config.model,
                        None,
                    ))
                }
                Ok(Message::Frame(_)) => {}
                Err(err) => {
                    return Err(classify_socket_error(&err, target, &asr_config.model));
                }
            }
        }

        Err(build_error(
            "connection_closed",
            "Gummy WebSocket 在检测期间提前断开。",
            target,
            &asr_config.model,
            None,
        ))
    })
    .await
    .map_err(|_| {
        build_error(
            "network_timeout",
            "等待 Gummy task-started 超时。",
            target,
            &asr_config.model,
            None,
        )
    })?;
    started?;

    let silence = vec![0u8; BINARY_AUDIO_FRAME_BYTES];
    ws_stream
        .send(Message::Binary(silence.into()))
        .await
        .map_err(|err| classify_socket_error(&err, target, &asr_config.model))?;
    let finish = build_finish_task_message(&task_id);
    ws_stream
        .send(Message::Text(finish.into()))
        .await
        .map_err(|err| classify_socket_error(&err, target, &asr_config.model))?;

    let finished = timeout(Duration::from_secs(10), async {
        while let Some(message) = ws_stream.next().await {
            match message {
                Ok(Message::Text(text)) => match parse_server_message(&text) {
                    Ok(ParsedServerEvent::TaskFinished) => return Ok(()),
                    Ok(ParsedServerEvent::TaskFailed {
                        error_code,
                        error_message,
                    }) => {
                        return Err(build_error(
                            "task_failed",
                            error_message,
                            target,
                            &asr_config.model,
                            error_code,
                        ))
                    }
                    Ok(_) => {}
                    Err(message) => {
                        return Err(build_error(
                            "protocol_parse",
                            message,
                            target,
                            &asr_config.model,
                            None,
                        ))
                    }
                },
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Binary(_)) => {}
                Ok(Message::Close(_)) => break,
                Ok(Message::Frame(_)) => {}
                Err(err) => {
                    return Err(classify_socket_error(&err, target, &asr_config.model));
                }
            }
        }

        Err(build_error(
            "connection_closed",
            "Gummy 连接检测未收到 task-finished。",
            target,
            &asr_config.model,
            None,
        ))
    })
    .await
    .map_err(|_| {
        build_error(
            "network_timeout",
            "等待 Gummy task-finished 超时。",
            target,
            &asr_config.model,
            None,
        )
    })?;
    finished?;

    let _ = ws_stream.send(Message::Close(None)).await;
    Ok(())
}

async fn wait_for_task_started(
    control_rx: &mut mpsc::UnboundedReceiver<ControlEvent>,
    target: &ResolvedGummyTarget,
    model: &str,
) -> Result<(), GummyErrorInfo> {
    timeout(Duration::from_secs(10), async {
        loop {
            match control_rx.recv().await {
                Some(ControlEvent::TaskStarted) => return Ok(()),
                Some(ControlEvent::TaskFailed(err)) => return Err(err),
                Some(ControlEvent::TaskFinished) => {
                    return Err(build_error(
                        "protocol_parse",
                        "Gummy 在 task-started 之前就结束了任务。",
                        target,
                        model,
                        None,
                    ))
                }
                None => {
                    return Err(build_error(
                        "connection_closed",
                        "等待 Gummy task-started 时连接中断。",
                        target,
                        model,
                        None,
                    ))
                }
            }
        }
    })
    .await
    .map_err(|_| {
        build_error(
            "network_timeout",
            "等待 Gummy task-started 超时。",
            target,
            model,
            None,
        )
    })?
}

async fn wait_for_task_finished(
    control_rx: &mut mpsc::UnboundedReceiver<ControlEvent>,
    target: &ResolvedGummyTarget,
    model: &str,
) -> Result<(), GummyErrorInfo> {
    timeout(Duration::from_secs(10), async {
        loop {
            match control_rx.recv().await {
                Some(ControlEvent::TaskFinished) => return Ok(()),
                Some(ControlEvent::TaskFailed(err)) => return Err(err),
                Some(ControlEvent::TaskStarted) => {}
                None => {
                    return Err(build_error(
                        "connection_closed",
                        "等待 Gummy task-finished 时连接中断。",
                        target,
                        model,
                        None,
                    ))
                }
            }
        }
    })
    .await
    .map_err(|_| {
        build_error(
            "network_timeout",
            "等待 Gummy task-finished 超时。",
            target,
            model,
            None,
        )
    })?
}

async fn flush_audio_and_finish(
    write: &mut futures_util::stream::SplitSink<GummyWsStream, Message>,
    pending_audio: &mut Vec<u8>,
    task_id: &str,
    target: &ResolvedGummyTarget,
    model: &str,
) -> Result<(), GummyErrorInfo> {
    if !pending_audio.is_empty() {
        let frame = std::mem::take(pending_audio);
        write
            .send(Message::Binary(frame.into()))
            .await
            .map_err(|err| classify_socket_error(&err, target, model))?;
    }

    let finish = build_finish_task_message(task_id);
    write
        .send(Message::Text(finish.into()))
        .await
        .map_err(|err| classify_socket_error(&err, target, model))?;

    Ok(())
}

async fn connect_ws(
    target: &ResolvedGummyTarget,
    api_key: &str,
    model: &str,
) -> Result<(GummyWsStream, tungstenite::handshake::client::Response), GummyErrorInfo> {
    let mut request = target
        .resolved_url
        .clone()
        .into_client_request()
        .map_err(|err| build_error("invalid_config", err.to_string(), target, model, None))?;

    let auth_value = HeaderValue::from_str(&format!("bearer {}", api_key.trim())).map_err(|err| {
        build_error(
            "invalid_config",
            format!("Gummy API Key 格式非法: {}", err),
            target,
            model,
            None,
        )
    })?;
    request.headers_mut().insert(AUTHORIZATION, auth_value);

    connect_async(request)
        .await
        .map_err(|err| classify_socket_error(&err, target, model))
}

fn parse_server_message(text: &str) -> Result<ParsedServerEvent, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|err| format!("无法解析 Gummy 返回的 JSON: {}", err))?;
    let event = value
        .get("header")
        .and_then(|header| header.get("event"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    match event {
        "task-started" => Ok(ParsedServerEvent::TaskStarted),
        "task-finished" => Ok(ParsedServerEvent::TaskFinished),
        "task-failed" => Ok(ParsedServerEvent::TaskFailed {
            error_code: value
                .get("header")
                .and_then(|header| header.get("error_code"))
                .and_then(Value::as_str)
                .map(ToString::to_string),
            error_message: value
                .get("header")
                .and_then(|header| header.get("error_message"))
                .and_then(Value::as_str)
                .filter(|message| !message.trim().is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| "Gummy 任务执行失败。".to_string()),
        }),
        "result-generated" => parse_result_generated(&value).map(ParsedServerEvent::ResultGenerated),
        _ => Ok(ParsedServerEvent::Ignore),
    }
}

fn parse_result_generated(value: &Value) -> Result<AsrSegmentUpdate, String> {
    let output = value
        .get("payload")
        .and_then(|payload| payload.get("output"))
        .ok_or_else(|| "result-generated 缺少 payload.output。".to_string())?;

    let transcription = output
        .get("transcription")
        .ok_or_else(|| "result-generated 缺少 transcription。".to_string())?;
    let sentence_id = transcription
        .get("sentence_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| "result-generated 缺少 transcription.sentence_id。".to_string())?;
    let original_text = transcription
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let is_final = transcription
        .get("sentence_end")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let begin_time_ms = transcription.get("begin_time").and_then(Value::as_u64);
    let end_time_ms = transcription.get("end_time").and_then(Value::as_u64);
    let translated_text = output
        .get("translations")
        .or_else(|| output.get("translation"))
        .and_then(Value::as_array)
        .and_then(|translations| {
            translations
                .iter()
                .find(|item| item.get("sentence_id").and_then(Value::as_u64) == Some(sentence_id))
                .or_else(|| translations.first())
        })
        .and_then(|translation| translation.get("text"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string);

    Ok(AsrSegmentUpdate {
        sentence_id,
        begin_time_ms,
        end_time_ms,
        original_text,
        translated_text,
        is_final,
    })
}

fn build_run_task_message(
    task_id: &str,
    asr_config: &AsrConfig,
    translation_config: &TranslationConfig,
) -> String {
    let source_language = if asr_config.language == "auto" {
        Value::Null
    } else {
        Value::String(asr_config.language.clone())
    };
    let target_languages = if translation_config.enabled {
        json!([translation_config.target_language.clone()])
    } else {
        json!([])
    };

    json!({
        "header": {
            "action": "run-task",
            "task_id": task_id,
            "streaming": "duplex"
        },
        "payload": {
            "task_group": TASK_GROUP,
            "task": TASK_NAME,
            "function": TASK_FUNCTION,
            "model": asr_config.model,
            "input": {},
            "parameters": {
                "format": "pcm",
                "sample_rate": asr_config.sample_rate,
                "transcription_enabled": true,
                "translation_enabled": translation_config.enabled,
                "source_language": source_language,
                "translation_target_languages": target_languages,
                "max_end_silence": asr_config.vad_silence_ms.clamp(200, MAX_END_SILENCE_MS),
                "vocabulary_id": nullable_string(&asr_config.vocabulary_id),
            }
        }
    })
    .to_string()
}

fn build_finish_task_message(task_id: &str) -> String {
    json!({
        "header": {
            "action": "finish-task",
            "task_id": task_id,
            "streaming": "duplex"
        },
        "payload": {
            "input": {}
        }
    })
    .to_string()
}

fn nullable_string(value: &str) -> Value {
    if value.trim().is_empty() {
        Value::Null
    } else {
        Value::String(value.trim().to_string())
    }
}

fn resolve_target(config: &AsrConfig) -> Result<ResolvedGummyTarget, GummyErrorInfo> {
    let raw = config.base_url.trim();
    if raw.is_empty() {
        return Err(build_error(
            "invalid_config",
            "Gummy WebSocket 地址尚未配置。",
            &ResolvedGummyTarget {
                resolved_url: String::new(),
            },
            &config.model,
            None,
        ));
    }

    let normalized = raw.trim_end_matches('/').to_string();
    let url = tungstenite::http::Uri::try_from(normalized.as_str()).map_err(|err| {
        build_error(
            "invalid_config",
            format!("Gummy WebSocket 地址无效: {}", err),
            &ResolvedGummyTarget {
                resolved_url: normalized.clone(),
            },
            &config.model,
            None,
        )
    })?;

    match url.scheme_str() {
        Some("ws") | Some("wss") => Ok(ResolvedGummyTarget {
            resolved_url: normalized,
        }),
        _ => Err(build_error(
            "invalid_config",
            "Gummy 必须使用 ws:// 或 wss:// WebSocket 地址。",
            &ResolvedGummyTarget {
                resolved_url: normalized,
            },
            &config.model,
            None,
        )),
    }
}

fn build_error(
    kind: &str,
    message: impl Into<String>,
    target: &ResolvedGummyTarget,
    model: &str,
    error_code: Option<String>,
) -> GummyErrorInfo {
    GummyErrorInfo {
        kind: kind.to_string(),
        message: message.into(),
        provider: PROVIDER.to_string(),
        resolved_url: target.resolved_url.clone(),
        model: model.to_string(),
        error_code,
    }
}

fn classify_socket_error(
    err: &dyn Error,
    target: &ResolvedGummyTarget,
    model: &str,
) -> GummyErrorInfo {
    let chain = error_chain(err);
    let lower = chain.to_ascii_lowercase();
    let kind = if lower.contains("dns") || lower.contains("failed to lookup address") {
        "network_dns"
    } else if lower.contains("tls") || lower.contains("certificate") || lower.contains("ssl") {
        "tls_handshake"
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "network_timeout"
    } else {
        "network_connect"
    };

    build_error(
        kind,
        format!("Gummy WebSocket 连接失败: {}", chain),
        target,
        model,
        None,
    )
}

fn connectivity_failure(error: GummyErrorInfo) -> GummyConnectivityResult {
    GummyConnectivityResult {
        ok: false,
        provider: error.provider,
        resolved_url: error.resolved_url,
        model: error.model,
        error_kind: Some(error.kind),
        message: error.message,
    }
}

fn error_chain(error: &dyn Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut current = error.source();
    while let Some(source) = current {
        parts.push(source.to_string());
        current = source.source();
    }
    parts.join(" <- ")
}

fn make_task_id() -> String {
    format!("lingsubtitle-{}", Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{build_run_task_message, parse_server_message, parse_result_generated, ParsedServerEvent};
    use crate::config::{AsrConfig, TranslationConfig};

    #[test]
    fn parses_result_generated_event() {
        let result = parse_result_generated(&json!({
            "payload": {
                "output": {
                    "transcription": {
                        "sentence_id": 12,
                        "begin_time": 100,
                        "end_time": 450,
                        "text": "hello world",
                        "sentence_end": false
                    },
                    "translations": [
                        {
                            "sentence_id": 12,
                            "text": "你好，世界",
                            "sentence_end": false
                        }
                    ]
                }
            }
        }))
        .unwrap();

        assert_eq!(result.sentence_id, 12);
        assert_eq!(result.original_text, "hello world");
        assert_eq!(result.translated_text.as_deref(), Some("你好，世界"));
        assert!(!result.is_final);
    }

    #[test]
    fn parses_task_failed_event() {
        let parsed = parse_server_message(r#"{
            "header": {
                "event": "task-failed",
                "error_code": "InvalidParameter",
                "error_message": "bad language pair"
            }
        }"#)
        .unwrap();

        match parsed {
            ParsedServerEvent::TaskFailed {
                error_code,
                error_message,
            } => {
                assert_eq!(error_code.as_deref(), Some("InvalidParameter"));
                assert_eq!(error_message, "bad language pair");
            }
            _ => panic!("expected task-failed"),
        }
    }

    #[test]
    fn builds_run_task_message_for_translation() {
        let asr = AsrConfig {
            language: "zh".to_string(),
            ..AsrConfig::default()
        };
        let translation = TranslationConfig {
            enabled: true,
            target_language: "en".to_string(),
        };

        let message = build_run_task_message("task-1", &asr, &translation);
        let parsed: serde_json::Value = serde_json::from_str(&message).unwrap();
        assert_eq!(parsed["header"]["action"], "run-task");
        assert_eq!(parsed["payload"]["task"], "asr");
        assert_eq!(parsed["payload"]["input"], json!({}));
        assert_eq!(parsed["payload"]["model"], asr.model);
        assert_eq!(
            parsed["payload"]["parameters"]["translation_target_languages"][0],
            "en"
        );
        assert_eq!(parsed["payload"]["parameters"]["source_language"], "zh");
    }
}
