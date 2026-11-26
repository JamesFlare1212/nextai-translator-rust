use super::{
    INCOMPLETE, MALFORMED, MAX_RESPONSE, PROVIDER_ERROR, Recovery, Responses, Settings, TOO_LARGE,
    Url, check_cancel,
};
use async_openai::types::responses::{
    CreateResponse, ResponseStreamEvent as Event,
    websocket::{
        ResponsesClientEvent, ResponsesClientEventResponseCreate, ResponsesServerEvent as Ws,
    },
};
use futures_util::{SinkExt, StreamExt};
use std::{io::ErrorKind, time::Duration};
use tokio::{sync::watch, time::timeout};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        Error, Message, client::IntoClientRequest, error::ProtocolError, http::header,
        protocol::WebSocketConfig,
    },
};

fn stream_event(event: Ws) -> Result<Option<Event>, String> {
    Ok(Some(match event {
        Ws::ResponseOutputTextDelta(e) => Event::ResponseOutputTextDelta(e.event),
        Ws::ResponseRefusalDelta(e) => Event::ResponseRefusalDelta(e.event),
        Ws::ResponseRefusalDone(e) => Event::ResponseRefusalDone(e.event),
        Ws::ResponseContentPartAdded(e) => Event::ResponseContentPartAdded(e.event),
        Ws::ResponseContentPartDone(e) => Event::ResponseContentPartDone(e.event),
        Ws::ResponseOutputItemAdded(e) => Event::ResponseOutputItemAdded(e.event),
        Ws::ResponseOutputItemDone(e) => Event::ResponseOutputItemDone(e.event),
        Ws::ResponseIncomplete(e) => Event::ResponseIncomplete(e.event),
        Ws::ResponseFailed(e) => Event::ResponseFailed(e.event),
        Ws::ResponseCompleted(e) => Event::ResponseCompleted(e.event),
        Ws::Error(e) => {
            return Err(match e.status {
                Some(status @ 400..=599) => format!("WebSocket 服务返回错误（{status}）。"),
                _ => PROVIDER_ERROR.into(),
            });
        }
        Ws::SteerFailed(_) => return Err(PROVIDER_ERROR.into()),
        _ => return Ok(None),
    }))
}

fn interrupted(recovery: &mut Recovery, message: &str) -> String {
    *recovery = Recovery::Interrupted;
    message.into()
}

fn socket_error(error: Error, recovery: &mut Recovery) -> String {
    match error {
        Error::Http(r) => format!(
            "WebSocket 握手失败（HTTP {}），请检查服务是否支持 Responses WebSocket 及密钥权限。",
            r.status().as_u16()
        ),
        Error::Capacity(_) => TOO_LARGE.into(),
        Error::ConnectionClosed => INCOMPLETE.into(),
        Error::Protocol(ProtocolError::ResetWithoutClosingHandshake) => {
            interrupted(recovery, INCOMPLETE)
        }
        Error::Io(e)
            if matches!(
                e.kind(),
                ErrorKind::ConnectionRefused
                    | ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::BrokenPipe
                    | ErrorKind::UnexpectedEof
                    | ErrorKind::NotConnected
                    | ErrorKind::TimedOut
                    | ErrorKind::NetworkDown
                    | ErrorKind::NetworkUnreachable
                    | ErrorKind::HostUnreachable
            ) =>
        {
            interrupted(recovery, INCOMPLETE)
        }
        _ => "WebSocket 协议或安全连接出错。".into(),
    }
}

pub(super) async fn translate(
    mut url: Url,
    settings: &Settings,
    mut body: CreateResponse,
    cancel: &watch::Receiver<bool>,
    on_delta: &mut impl FnMut(&str),
    recovery: &mut Recovery,
) -> Result<(), String> {
    check_cancel(cancel)?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme).map_err(|_| "WebSocket 地址无效。")?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "WebSocket 地址无效。")?;
    if !settings.api_key.trim().is_empty() {
        let mut token = format!("Bearer {}", settings.api_key.trim())
            .parse::<header::HeaderValue>()
            .map_err(|_| "API 密钥无效。")?;
        token.set_sensitive(true);
        request.headers_mut().insert(header::AUTHORIZATION, token);
    }

    body.stream = None;
    body.background = None;
    let create = ResponsesClientEvent::Create(Box::new(ResponsesClientEventResponseCreate {
        response: body,
        ..Default::default()
    }));
    let payload = serde_json::to_string(&create).map_err(|_| MALFORMED)?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_RESPONSE))
        .max_frame_size(Some(MAX_RESPONSE));
    let (mut socket, _) = timeout(
        Duration::from_secs(15),
        connect_async_with_config(request, Some(config), false),
    )
    .await
    .map_err(|_| interrupted(recovery, "WebSocket 连接超时。"))?
    .map_err(|e| socket_error(e, recovery))?;
    timeout(Duration::from_secs(15), socket.send(Message::text(payload)))
        .await
        .map_err(|_| interrupted(recovery, "WebSocket 发送超时。"))?
        .map_err(|e| socket_error(e, recovery))?;

    let mut responses = Responses::default();
    let mut size = 0;
    loop {
        let msg = timeout(Duration::from_secs(120), socket.next())
            .await
            .map_err(|_| interrupted(recovery, "译文流长时间没有响应。"))?
            .ok_or(INCOMPLETE)?
            .map_err(|e| socket_error(e, recovery))?;
        check_cancel(cancel)?;
        match msg {
            Message::Text(data) => {
                if data.len() > MAX_RESPONSE - size {
                    return Err(TOO_LARGE.into());
                }
                size += data.len();
                let event = serde_json::from_str(&data).map_err(|_| MALFORMED)?;
                if let Some(event) = stream_event(event)?
                    && responses.event(event, |s| {
                        check_cancel(cancel)?;
                        on_delta(s);
                        check_cancel(cancel)
                    })?
                {
                    return Ok(());
                }
            }
            Message::Close(_) => return Err(INCOMPLETE.into()),
            Message::Ping(_) | Message::Pong(_) => continue,
            _ => return Err(MALFORMED.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_errors_do_not_reveal_arbitrary_fields() {
        let server_error = serde_json::from_str(
            r#"{"type":"error","status":503,"error":{"message":"sensitive","code":"sensitive"}}"#,
        )
        .unwrap();
        let message = stream_event(server_error).unwrap_err();
        assert!(message.contains("503"));
        assert!(!message.contains("sensitive"));
    }

    #[test]
    fn transport_loss_is_distinct_from_a_clean_close_or_protocol_error() {
        for (err, retry) in [
            (Error::ConnectionClosed, false),
            (
                Error::Protocol(ProtocolError::UnmaskedFrameFromClient),
                false,
            ),
            (Error::Io(ErrorKind::InvalidData.into()), false),
            (
                Error::Protocol(ProtocolError::ResetWithoutClosingHandshake),
                true,
            ),
            (Error::Io(ErrorKind::ConnectionReset.into()), true),
        ] {
            let mut recovery = Recovery::None;
            let _ = socket_error(err, &mut recovery);
            assert_eq!(recovery == Recovery::Interrupted, retry);
        }
    }
}
