//! 连通性自测：用发放的 sk-key 对站点 /v1/responses 发一次最小请求，
//! 把「配置成功」变成「验证成功」。

use serde::Serialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::site::v1_base_url;
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ConnectivityResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub http_status: u16,
    pub model: String,
    /// 失败时的可读错误说明（成功时为空）。
    pub message: String,
}

/// POST {site}/v1/responses 最小请求（max_output_tokens 取最小合法值 16）。
/// 注意：渠道必须原生支持 Responses API，否则会在这里直接暴露错误。
#[tauri::command]
pub async fn test_connection(
    state: tauri::State<'_, AppState>,
) -> Result<ConnectivityResult, AppError> {
    let sk_key = {
        let store = state.store.lock().await;
        store
            .sk_key
            .clone()
            .ok_or_else(|| AppError::msg("尚未获取 API 令牌，请先完成「自动开卡」"))?
    };
    // 优先用用户在 Smart Agent 里选的唯一模型，否则回落站点默认
    let model = {
        let store = state.store.lock().await;
        store.default_model.clone()
    };
    let model = match model {
        Some(m) if !m.trim().is_empty() => m,
        _ => {
            crate::api::get_client_config_inner(&state)
                .await?
                .default_model
        }
    };

    let body = json!({
        "model": model,
        "input": "ping",
        "max_output_tokens": 16,
        "stream": false,
    });

    let started = std::time::Instant::now();
    let resp = state
        .http
        .post(format!("{}/responses", v1_base_url()))
        .header("Authorization", format!("Bearer {sk_key}"))
        .json(&body)
        .send()
        .await;
    let latency_ms = started.elapsed().as_millis() as u64;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            let msg = if e.is_timeout() {
                "连接超时：请检查网络后重试".to_string()
            } else if e.is_connect() {
                "无法连接到站点服务：请确认服务是否在线".to_string()
            } else {
                format!("网络错误：{e}")
            };
            return Ok(ConnectivityResult {
                ok: false,
                latency_ms,
                http_status: 0,
                model,
                message: msg,
            });
        }
    };

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if status.is_success() {
        return Ok(ConnectivityResult {
            ok: true,
            latency_ms,
            http_status: status.as_u16(),
            model,
            message: String::new(),
        });
    }

    // 提取上游错误信息（OpenAI 风格 {"error":{"message":...}} 或站点 {message}）
    let detail = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| v.get("message").and_then(Value::as_str).map(str::to_string))
        })
        .unwrap_or_else(|| {
            let mut t = text;
            t.truncate(200);
            t
        });

    let hint = match status.as_u16() {
        401 | 403 => "令牌无效或已被禁用，请尝试重新运行向导重新开卡",
        404 => "站点未开放 /v1/responses（渠道需原生支持 Responses API）",
        429 => "触发限流，请稍后重试",
        _ => "请求失败",
    };

    Ok(ConnectivityResult {
        ok: false,
        latency_ms,
        http_status: status.as_u16(),
        model,
        message: format!("{hint}（HTTP {}）：{detail}", status.as_u16()),
    })
}
