use std::error::Error;
use std::time::Instant;

use futures_util::StreamExt;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::TranslationConfig;

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationErrorInfo {
    pub kind: String,
    pub message: String,
    pub provider: String,
    pub resolved_url: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationConnectivityResult {
    pub ok: bool,
    pub provider: String,
    pub resolved_url: String,
    pub model: String,
    pub error_kind: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct TranslationRequestContext {
    pub segment_id: Option<u64>,
    pub revision: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
enum TranslationProvider {
    OpenAiCompatible,
    DashScopeCompatible,
}

impl TranslationProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openaiCompatible",
            Self::DashScopeCompatible => "dashscopeCompatible",
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedTranslationTarget {
    provider: TranslationProvider,
    resolved_url: String,
}

static HTTP_CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(4)
        .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("Failed to create HTTP client")
});

fn build_error(
    kind: &str,
    message: impl Into<String>,
    provider: TranslationProvider,
    resolved_url: impl Into<String>,
    model: &str,
) -> TranslationErrorInfo {
    TranslationErrorInfo {
        kind: kind.to_string(),
        message: message.into(),
        provider: provider.as_str().to_string(),
        resolved_url: resolved_url.into(),
        model: model.to_string(),
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

fn detect_provider(base_url: &str) -> TranslationProvider {
    if base_url.to_ascii_lowercase().contains("dashscope.aliyuncs.com") {
        TranslationProvider::DashScopeCompatible
    } else {
        TranslationProvider::OpenAiCompatible
    }
}

fn resolve_target(config: &TranslationConfig) -> Result<ResolvedTranslationTarget, TranslationErrorInfo> {
    let base_url = config.base_url.trim();
    let provider = detect_provider(base_url);

    if base_url.is_empty() {
        return Err(build_error(
            "invalid_config",
            "Translation base URL is empty.",
            provider,
            "",
            &config.model,
        ));
    }

    if base_url.starts_with("wss://") || base_url.contains("/api-ws/") {
        return Err(build_error(
            "invalid_config",
            "The translation base URL points to the ASR WebSocket endpoint. Use a text-generation compatible endpoint instead.",
            provider,
            base_url,
            &config.model,
        ));
    }

    if config.api_key.trim().is_empty() {
        return Err(build_error(
            "invalid_config",
            "Translation API Key is not configured.",
            provider,
            base_url,
            &config.model,
        ));
    }

    if config.model.trim().is_empty() {
        return Err(build_error(
            "invalid_config",
            "Translation model is not configured.",
            provider,
            base_url,
            &config.model,
        ));
    }

    let normalized = base_url.trim_end_matches('/');
    if matches!(provider, TranslationProvider::DashScopeCompatible)
        && !normalized.contains("/compatible-mode/v1")
    {
        return Err(build_error(
            "invalid_config",
            "DashScope translation must use a compatible-mode base URL such as https://dashscope.aliyuncs.com/compatible-mode/v1.",
            provider,
            normalized,
            &config.model,
        ));
    }

    let resolved_url = if normalized.ends_with("/chat/completions") {
        normalized.to_string()
    } else {
        format!("{normalized}/chat/completions")
    };

    Url::parse(&resolved_url).map_err(|err| {
        build_error(
            "invalid_config",
            format!("Invalid translation URL: {}", err),
            provider,
            resolved_url.clone(),
            &config.model,
        )
    })?;

    Ok(ResolvedTranslationTarget {
        provider,
        resolved_url,
    })
}

fn classify_send_error(
    err: &reqwest::Error,
    provider: TranslationProvider,
    resolved_url: &str,
    model: &str,
) -> TranslationErrorInfo {
    let chain = error_chain(err);
    let lower = chain.to_ascii_lowercase();
    let kind = if err.is_timeout() {
        "network_timeout"
    } else if lower.contains("dns") || lower.contains("failed to lookup address") {
        "network_dns"
    } else if lower.contains("certificate")
        || lower.contains("tls")
        || lower.contains("ssl")
        || lower.contains("handshake")
    {
        "tls_handshake"
    } else if lower.contains("proxy") {
        "proxy_connect"
    } else {
        "network_connect"
    };

    let user_message = match kind {
        "network_timeout" => {
            "The translation request timed out before the server returned a response."
        }
        "network_dns" => {
            "The translation request could not resolve the target host. Check the base URL or local network DNS."
        }
        "tls_handshake" => {
            "The translation request failed during TLS/SSL negotiation. Check certificates, proxy, or HTTPS interception."
        }
        "proxy_connect" => {
            "The translation request failed while connecting through a proxy. Check system proxy settings."
        }
        _ => {
            "The translation request failed before the model returned a response. This is usually a network, proxy, or TLS issue."
        }
    };

    build_error(
        kind,
        format!("{user_message} Root cause: {chain}"),
        provider,
        resolved_url,
        model,
    )
}

fn build_system_prompt(config: &TranslationConfig, override_prompt: Option<&str>) -> String {
    match override_prompt {
        Some(prompt) => prompt.to_string(),
        None => format!(
            "{}\nTarget language: {}",
            config.system_prompt, config.target_language
        ),
    }
}

fn build_request(config: &TranslationConfig, text: &str, override_prompt: Option<&str>, stream: bool) -> ChatRequest {
    ChatRequest {
        model: config.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: build_system_prompt(config, override_prompt),
            },
            ChatMessage {
                role: "user".to_string(),
                content: text.to_string(),
            },
        ],
        temperature: 0.1,
        max_tokens: 128,
        stream,
    }
}

fn extract_stream_delta(value: &Value) -> Option<String> {
    let delta = value.get("choices")?.get(0)?.get("delta")?;
    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
        return Some(content.to_string());
    }

    if let Some(content_array) = delta.get("content").and_then(|v| v.as_array()) {
        let text: String = content_array
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| item.as_str().map(|s| s.to_string()))
            })
            .collect();
        if !text.is_empty() {
            return Some(text);
        }
    }

    if let Some(content) = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
    {
        return Some(content.to_string());
    }

    None
}

async fn request_translation_once(
    config: &TranslationConfig,
    text: &str,
    override_prompt: Option<&str>,
    context: Option<&TranslationRequestContext>,
) -> Result<String, TranslationErrorInfo> {
    let target = resolve_target(config)?;
    let request_body = build_request(config, text, override_prompt, false);

    if let Some(ctx) = context {
        eprintln!(
            "[translate_once] provider={} segment={:?} revision={:?} model={} url={}",
            target.provider.as_str(),
            ctx.segment_id,
            ctx.revision,
            config.model,
            target.resolved_url
        );
    }

    let response = HTTP_CLIENT
        .post(&target.resolved_url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|err| classify_send_error(&err, target.provider, &target.resolved_url, &config.model))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(build_error(
            "http_status",
            format!("Translation API returned HTTP {}: {}", status, body),
            target.provider,
            &target.resolved_url,
            &config.model,
        ));
    }

    let result: ChatResponse = response.json().await.map_err(|err| {
        build_error(
            "response_parse",
            format!(
                "Failed to parse translation response. Root cause: {}",
                error_chain(&err)
            ),
            target.provider,
            &target.resolved_url,
            &config.model,
        )
    })?;

    let translated = result
        .choices
        .first()
        .map(|choice| choice.message.content.trim().to_string())
        .unwrap_or_default();

    if translated.is_empty() {
        return Err(build_error(
            "response_parse",
            "The translation response was successful but empty.",
            target.provider,
            &target.resolved_url,
            &config.model,
        ));
    }

    Ok(translated)
}

pub async fn translate_stream<F>(
    config: &TranslationConfig,
    text: &str,
    context: Option<&TranslationRequestContext>,
    mut on_delta: F,
) -> Result<String, TranslationErrorInfo>
where
    F: FnMut(String, String),
{
    let target = resolve_target(config)?;
    let request_body = build_request(config, text, None, true);
    let started_at = Instant::now();

    if let Some(ctx) = context {
        eprintln!(
            "[translate_stream] provider={} segment={:?} revision={:?} model={} url={}",
            target.provider.as_str(),
            ctx.segment_id,
            ctx.revision,
            config.model,
            target.resolved_url
        );
    }

    let response = HTTP_CLIENT
        .post(&target.resolved_url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|err| classify_send_error(&err, target.provider, &target.resolved_url, &config.model))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(build_error(
            "http_status",
            format!("Translation API returned HTTP {}: {}", status, body),
            target.provider,
            &target.resolved_url,
            &config.model,
        ));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut accumulated = String::new();
    let mut saw_delta = false;
    let mut first_token_logged = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| {
            build_error(
                "stream_read",
                format!("Failed to read the translation stream. Root cause: {}", error_chain(&err)),
                target.provider,
                &target.resolved_url,
                &config.model,
            )
        })?;

        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(delimiter) = buffer.find("\n\n") {
            let event = buffer[..delimiter].to_string();
            buffer = buffer[delimiter + 2..].to_string();

            for line in event.lines() {
                let line = line.trim();
                if !line.starts_with("data:") {
                    continue;
                }

                let data = line.trim_start_matches("data:").trim();
                if data.is_empty() {
                    continue;
                }
                if data == "[DONE]" {
                    if accumulated.is_empty() {
                        return Err(build_error(
                            "response_parse",
                            "The translation stream finished without any text.",
                            target.provider,
                            &target.resolved_url,
                            &config.model,
                        ));
                    }
                    eprintln!(
                        "[translate_stream] provider={} completed total_latency_ms={}",
                        target.provider.as_str(),
                        started_at.elapsed().as_millis()
                    );
                    return Ok(accumulated);
                }

                let value: Value = serde_json::from_str(data).map_err(|err| {
                    build_error(
                        "response_parse",
                        format!("Failed to parse translation stream chunk: {}", err),
                        target.provider,
                        &target.resolved_url,
                        &config.model,
                    )
                })?;

                if let Some(delta) = extract_stream_delta(&value) {
                    if !delta.is_empty() {
                        if !first_token_logged {
                            eprintln!(
                                "[translate_stream] provider={} first_token_latency_ms={}",
                                target.provider.as_str(),
                                started_at.elapsed().as_millis()
                            );
                            first_token_logged = true;
                        }
                        saw_delta = true;
                        accumulated.push_str(&delta);
                        on_delta(delta, accumulated.clone());
                    }
                }
            }
        }
    }

    if saw_delta && !accumulated.is_empty() {
        eprintln!(
            "[translate_stream] provider={} completed_without_done total_latency_ms={}",
            target.provider.as_str(),
            started_at.elapsed().as_millis()
        );
        Ok(accumulated)
    } else {
        Err(build_error(
            "response_parse",
            "The translation stream ended before any text was received.",
            target.provider,
            &target.resolved_url,
            &config.model,
        ))
    }
}

pub async fn translate(
    config: &TranslationConfig,
    text: &str,
    context: Option<&TranslationRequestContext>,
) -> Result<String, TranslationErrorInfo> {
    request_translation_once(config, text, None, context).await
}

pub async fn check_connectivity(config: &TranslationConfig) -> TranslationConnectivityResult {
    let target = match resolve_target(config) {
        Ok(target) => target,
        Err(error) => {
            return TranslationConnectivityResult {
                ok: false,
                provider: error.provider,
                resolved_url: error.resolved_url,
                model: error.model,
                error_kind: Some(error.kind),
                message: error.message,
            };
        }
    };

    match request_translation_once(config, "ping", Some("Reply with OK only."), None).await {
        Ok(_) => TranslationConnectivityResult {
            ok: true,
            provider: target.provider.as_str().to_string(),
            resolved_url: target.resolved_url,
            model: config.model.clone(),
            error_kind: None,
            message: "Translation connectivity check passed.".to_string(),
        },
        Err(error) => TranslationConnectivityResult {
            ok: false,
            provider: error.provider,
            resolved_url: error.resolved_url,
            model: error.model,
            error_kind: Some(error.kind),
            message: error.message,
        },
    }
}
