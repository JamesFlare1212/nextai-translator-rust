use super::*;
use futures_util::{SinkExt, StreamExt};
use tokio::time::timeout;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Error, Message, client::IntoClientRequest, protocol::WebSocketConfig},
};

fn provider_failure(data: &[u8], first_event: bool) -> (String, Recovery) {
    let Ok(event) = serde_json::from_slice::<serde_json::Value>(data) else {
        return (PROVIDER_ERROR.into(), Recovery::None);
    };
    let status = event["status"].as_u64().filter(|n| (400..=599).contains(n));
    let error = event
        .get("error")
        .or_else(|| event.pointer("/response/error"));
    let code = error.and_then(|e| e["code"].as_str());
    if first_event
        && event["type"] == "error"
        && status == Some(503)
        && code == Some("model_not_found")
        && error.is_some_and(|e| e["type"] == "new_api_error")
    {
        return (
            "WebSocket 无可用模型渠道（503/model_not_found）。".into(),
            Recovery::NoWsRoute,
        );
    }
    let label = match code {
        Some("model_not_found") => "模型不可用（model_not_found）",
        Some("invalid_api_key") => "API 密钥无效",
        Some("insufficient_quota") => "额度不足",
        Some("rate_limit_exceeded") => "请求受到限流",
        Some("invalid_request_error") => "请求参数无效",
        _ => "服务返回错误",
    };
    let status = status.map(|n| format!("，状态码 {n}")).unwrap_or_default();
    (format!("WebSocket {label}{status}。"), Recovery::None)
}

pub(super) async fn translate<F: FnMut(&str)>(
    mut url: Url,
    settings: &Settings,
    mut body: serde_json::Value,
    answer: &mut Answer<'_, F>,
    recovery: &mut Recovery,
) -> Result<(), String> {
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme).map_err(|_| "WebSocket 地址无效。")?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "WebSocket 地址无效。")?;
    if !settings.api_key.trim().is_empty() {
        let mut token = format!("Bearer {}", settings.api_key.trim())
            .parse::<reqwest::header::HeaderValue>()
            .map_err(|_| "API 密钥无效。")?;
        token.set_sensitive(true);
        request
            .headers_mut()
            .insert(reqwest::header::AUTHORIZATION, token);
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_RESPONSE))
        .max_frame_size(Some(MAX_RESPONSE));
    let transport_error = |e: Error| match e {
        Error::Http(r) => format!(
            "WebSocket 握手失败（HTTP {}），请检查服务是否支持 Responses WebSocket 及密钥权限。",
            r.status().as_u16()
        ),
        Error::Capacity(_) => TOO_LARGE.into(),
        Error::Io(_)
        | Error::ConnectionClosed
        | Error::Protocol(
            tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
        ) => {
            *recovery = Recovery::Interrupted;
            INCOMPLETE.into()
        }
        _ => "WebSocket 协议或安全连接出错。".into(),
    };
    let mut transport_error = transport_error;
    let (mut socket, _) = timeout(
        Duration::from_secs(15),
        connect_async_with_config(request, Some(config), false),
    )
    .await
    .map_err(|_| "WebSocket 连接超时。")?
    .map_err(&mut transport_error)?;
    body.as_object_mut()
        .expect("translation request object")
        .remove("stream");
    body["type"] = json!("response.create");
    let input = body["input"].take();
    body["input"] = json!([{"role":"user", "content":input}]);
    timeout(
        Duration::from_secs(15),
        socket.send(Message::text(body.to_string())),
    )
    .await
    .map_err(|_| "WebSocket 发送超时。")?
    .map_err(&mut transport_error)?;
    let mut size = 0;
    loop {
        let msg = match timeout(Duration::from_secs(120), socket.next()).await {
            Ok(Some(msg)) => msg.map_err(&mut transport_error)?,
            _ => {
                *recovery = Recovery::Interrupted;
                return Err(INCOMPLETE.into());
            }
        };
        match msg {
            Message::Text(data) => {
                if data.len() > MAX_RESPONSE - size {
                    return Err(TOO_LARGE.into());
                }
                let first_event = size == 0;
                size += data.len();
                match answer.decode(data.as_bytes(), Some(b"")) {
                    Ok(true) => return answer.complete(),
                    Ok(false) => {}
                    Err(e) if e == PROVIDER_ERROR => {
                        let (detail, next) = provider_failure(data.as_bytes(), first_event);
                        *recovery = next;
                        return Err(detail);
                    }
                    Err(e) => return Err(e),
                }
            }
            Message::Close(_) => {
                *recovery = Recovery::Interrupted;
                return Err(INCOMPLETE.into());
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            _ => return Err(MALFORMED.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_explicit_route_rejection_before_generation_can_switch_transport() {
        for (first, kind, status, code, origin, fallback) in [
            (true, "error", 503, "model_not_found", "new_api_error", true),
            (
                false,
                "error",
                503,
                "model_not_found",
                "new_api_error",
                false,
            ),
            (
                true,
                "response.failed",
                503,
                "model_not_found",
                "new_api_error",
                false,
            ),
            (
                true,
                "error",
                404,
                "model_not_found",
                "new_api_error",
                false,
            ),
            (
                true,
                "error",
                503,
                "model_not_found",
                "invalid_request_error",
                false,
            ),
            (
                true,
                "error",
                401,
                "invalid_api_key",
                "new_api_error",
                false,
            ),
            (
                true,
                "error",
                429,
                "insufficient_quota",
                "new_api_error",
                false,
            ),
        ] {
            let event = json!({"type":kind, "status":status,"error":{"code":code,"type":origin,"message":"sensitive-content"}});
            let (detail, recovery) = provider_failure(event.to_string().as_bytes(), first);
            assert_eq!(recovery == Recovery::NoWsRoute, fallback);
            assert!(detail.contains(&status.to_string()));
            assert!(!detail.contains("sensitive-content"));
        }
    }
}
