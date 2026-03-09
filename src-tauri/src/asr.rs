use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite};
use uuid::Uuid;

use crate::config::AsrConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrResult {
    pub text: String,
    pub is_final: bool,
}

/// Run a DashScope WebSocket ASR session.
/// Receives raw PCM audio chunks from `audio_rx`, sends them to the WebSocket,
/// and forwards recognized text results to `result_tx`.
pub async fn run_asr_session(
    config: AsrConfig,
    mut audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    result_tx: mpsc::UnboundedSender<AsrResult>,
    mut stop_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    let task_id = Uuid::new_v4().to_string();

    // Build WebSocket request with auth header
    let url = config.base_url.clone();
    let request = tungstenite::http::Request::builder()
        .uri(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
        .header("Sec-WebSocket-Version", "13")
        .header("Host", extract_host(&url))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .body(())
        .map_err(|e| format!("Failed to build WebSocket request: {}", e))?;

    let (ws_stream, _) = connect_async(request)
        .await
        .map_err(|e| format!("WebSocket connection failed: {}", e))?;

    let (mut write, mut read) = ws_stream.split();

    // 1. Send run-task command
    let run_task = json!({
        "header": {
            "action": "run-task",
            "task_id": task_id,
            "streaming": "duplex"
        },
        "payload": {
            "task_group": "audio",
            "task": "asr",
            "function": "recognition",
            "model": config.model,
            "parameters": {
                "format": "pcm",
                "sample_rate": config.sample_rate
            },
            "input": {}
        }
    });

    write
        .send(tungstenite::Message::Text(run_task.to_string().into()))
        .await
        .map_err(|e| format!("Failed to send run-task: {}", e))?;

    // 2. Wait for task-started event
    let mut task_started = false;
    while let Some(msg) = read.next().await {
        match msg {
            Ok(tungstenite::Message::Text(ref text)) => {
                if let Ok(json) = serde_json::from_str::<Value>(&text) {
                    let event = json["header"]["event"].as_str().unwrap_or("");
                    if event == "task-started" {
                        task_started = true;
                        break;
                    } else if event == "task-failed" {
                        let msg = json["header"]["error_message"]
                            .as_str()
                            .unwrap_or("Unknown error");
                        return Err(format!("ASR task failed: {}", msg));
                    }
                }
            }
            Err(e) => return Err(format!("WebSocket error waiting for task-started: {}", e)),
            _ => {}
        }
    }

    if !task_started {
        return Err("WebSocket closed before task-started".to_string());
    }

    // 3. Spawn task to read ASR results
    let result_tx_clone = result_tx.clone();
    let read_handle = tokio::spawn(async move {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(tungstenite::Message::Text(ref text)) => {
                    if let Ok(json) = serde_json::from_str::<Value>(&text) {
                        let event = json["header"]["event"].as_str().unwrap_or("");
                        match event {
                            "result-generated" => {
                                let sentence = &json["payload"]["output"]["sentence"];
                                let text = sentence["text"].as_str().unwrap_or("").to_string();
                                let is_final =
                                    sentence["sentence_end"].as_bool().unwrap_or(false);

                                // Skip heartbeats and empty text
                                let is_heartbeat =
                                    sentence["heartbeat"].as_bool().unwrap_or(false);
                                if is_heartbeat || text.is_empty() {
                                    continue;
                                }

                                let _ = result_tx_clone.send(AsrResult {
                                    text,
                                    is_final,
                                });
                            }
                            "task-finished" => {
                                break;
                            }
                            "task-failed" => {
                                eprintln!("ASR task failed during streaming");
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    eprintln!("WebSocket read error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // 4. Send audio frames and handle stop signal
    loop {
        tokio::select! {
            audio = audio_rx.recv() => {
                match audio {
                    Some(pcm_data) => {
                        if write
                            .send(tungstenite::Message::Binary(pcm_data.into()))
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

    // 5. Send finish-task
    let finish_task = json!({
        "header": {
            "action": "finish-task",
            "task_id": task_id,
            "streaming": "duplex"
        },
        "payload": {
            "input": {}
        }
    });

    let _ = write
        .send(tungstenite::Message::Text(finish_task.to_string().into()))
        .await;

    // Wait for reader to finish (receives remaining results + task-finished)
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
