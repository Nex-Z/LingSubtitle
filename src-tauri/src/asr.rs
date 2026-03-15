use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite};

use crate::config::AsrConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrResult {
    pub text: String,
    pub is_final: bool,
}

/// Run a qwen3-asr-flash-realtime WebSocket ASR session.
/// Receives raw PCM audio chunks from `audio_rx`, encodes them as base64,
/// sends them via `input_audio_buffer.append` events, and forwards
/// recognized text results to `result_tx`.
pub async fn run_asr_session(
    config: AsrConfig,
    mut audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    result_tx: mpsc::UnboundedSender<AsrResult>,
    mut stop_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    // Build WebSocket URL with model query parameter
    let ws_url = format!("{}?model={}", config.base_url, config.model);

    // Build WebSocket request with auth headers
    let request = tungstenite::http::Request::builder()
        .uri(&ws_url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("OpenAI-Beta", "realtime=v1")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .header("Sec-WebSocket-Version", "13")
        .header("Host", extract_host(&config.base_url))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .body(())
        .map_err(|e| format!("Failed to build WebSocket request: {}", e))?;

    let (ws_stream, _) = connect_async(request)
        .await
        .map_err(|e| format!("WebSocket connection failed: {}", e))?;

    let (mut write, mut read) = ws_stream.split();

    // 1. Wait for session.created event
    let mut session_created = false;
    while let Some(msg) = read.next().await {
        match msg {
            Ok(tungstenite::Message::Text(ref text)) => {
                if let Ok(json) = serde_json::from_str::<Value>(text) {
                    let event_type = json["type"].as_str().unwrap_or("");
                    if event_type == "session.created" {
                        session_created = true;
                        break;
                    } else if event_type == "error" {
                        let err_msg = json["error"]["message"]
                            .as_str()
                            .unwrap_or("Unknown error");
                        return Err(format!("ASR session error: {}", err_msg));
                    }
                }
            }
            Err(e) => {
                return Err(format!(
                    "WebSocket error waiting for session.created: {}",
                    e
                ))
            }
            _ => {}
        }
    }

    if !session_created {
        return Err("WebSocket closed before session.created".to_string());
    }

    // 2. Send session.update to configure transcription
    let transcription_config = if config.language.is_empty() || config.language == "auto" {
        json!({})
    } else {
        json!({ "language": config.language })
    };

    let session_update = json!({
        "event_id": "event_session_update",
        "type": "session.update",
        "session": {
            "modalities": ["text"],
            "input_audio_format": "pcm",
            "sample_rate": config.sample_rate,
            "input_audio_transcription": transcription_config,
            "turn_detection": {
                "type": "server_vad",
                "threshold": 0.0,
                "silence_duration_ms": config.vad_silence_ms.clamp(100, 2000)
            }
        }
    });

    write
        .send(tungstenite::Message::Text(
            session_update.to_string().into(),
        ))
        .await
        .map_err(|e| format!("Failed to send session.update: {}", e))?;

    // 3. Spawn task to read ASR result events
    let result_tx_clone = result_tx.clone();
    let read_handle = tokio::spawn(async move {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(tungstenite::Message::Text(ref text)) => {
                    if let Ok(json) = serde_json::from_str::<Value>(text) {
                        let event_type = json["type"].as_str().unwrap_or("");
                        match event_type {
                            // Intermediate transcription result
                            "conversation.item.input_audio_transcription.text" => {
                                // Some payloads use "text", others use "stash" for partials.
                                let text = json["text"]
                                    .as_str()
                                    .or_else(|| json["stash"].as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if !text.is_empty() {
                                    let _ = result_tx_clone.send(AsrResult {
                                        text,
                                        is_final: false,
                                    });
                                }
                            }
                            // Final transcription result
                            "conversation.item.input_audio_transcription.completed" => {
                                let text = json["transcript"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string();
                                if !text.is_empty() {
                                    let _ = result_tx_clone.send(AsrResult {
                                        text,
                                        is_final: true,
                                    });
                                }
                            }
                            // VAD events (log only)
                            "input_audio_buffer.speech_started" => {
                                println!("[ASR] VAD: Speech started");
                            }
                            "input_audio_buffer.speech_stopped" => {
                                println!("[ASR] VAD: Speech stopped");
                            }
                            // Session finished
                            "session.finished" => {
                                println!("[ASR] Session finished");
                                break;
                            }
                            // Error event
                            "error" => {
                                let err_msg = json["error"]["message"]
                                    .as_str()
                                    .unwrap_or("Unknown error");
                                eprintln!("[ASR] Error: {}", err_msg);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[ASR] WebSocket read error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // 4. Send audio frames as base64-encoded input_audio_buffer.append events
    let mut event_counter: u64 = 0;
    loop {
        tokio::select! {
            audio = audio_rx.recv() => {
                match audio {
                    Some(pcm_data) => {
                        event_counter += 1;
                        let audio_b64 = BASE64.encode(&pcm_data);
                        // Direct string formatting avoids serde_json::Value allocation
                        let msg = format!(
                            r#"{{"event_id":"evt_{}","type":"input_audio_buffer.append","audio":"{}"}}"#,
                            event_counter, audio_b64
                        );

                        if write
                            .send(tungstenite::Message::Text(msg.into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    None => break, // Channel closed
                }
            }
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    break; // Stop signal received
                }
            }
        }
    }

    // 5. Send session.finish event
    let finish_event = json!({
        "event_id": "event_session_finish",
        "type": "session.finish"
    });

    let _ = write
        .send(tungstenite::Message::Text(
            finish_event.to_string().into(),
        ))
        .await;

    // Wait for reader to finish (receives remaining results + session.finished)
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), read_handle).await;

    Ok(())
}

fn extract_host(url: &str) -> String {
    url.replace("wss://", "")
        .replace("ws://", "")
        .split('/')
        .next()
        .unwrap_or("dashscope.aliyuncs.com")
        .to_string()
}
