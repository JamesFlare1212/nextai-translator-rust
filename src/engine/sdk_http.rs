use super::{
    INCOMPLETE, MALFORMED, MAX_EVENT, MAX_RESPONSE, PROVIDER_ERROR, Recovery, Settings, TOO_LARGE,
};
use async_openai::{
    Client,
    config::OpenAIConfig,
    error::{OpenAIError, StreamError},
    middleware::HttpRequestFactory,
};
use bytes::{Buf, Bytes};
use futures_util::stream;
use reqwest::{Body, RequestBuilder, Response, StatusCode, Url, header};
use std::{
    io, mem,
    sync::{Arc, OnceLock},
};

#[derive(Clone, Default)]
pub(super) struct WireState(Arc<OnceLock<(String, Recovery)>>);

impl WireState {
    fn fail(&self, message: impl Into<String>, recovery: Recovery) -> io::Error {
        let _ = self.0.set((message.into(), recovery));
        io::Error::other("translation stream rejected")
    }

    pub(super) fn check(&self, recovery: &mut Recovery) -> Result<(), String> {
        if let Some((message, action)) = self.0.get() {
            *recovery = *action;
            Err(message.clone())
        } else {
            Ok(())
        }
    }

    pub(super) fn error(&self, error: OpenAIError, recovery: &mut Recovery) -> String {
        *recovery = Recovery::None;
        match error {
            OpenAIError::Boxed(_) => self
                .check(recovery)
                .err()
                .unwrap_or_else(|| MALFORMED.into()),
            OpenAIError::StreamError(e) if matches!(*e, StreamError::EventStream(_)) => self
                .check(recovery)
                .err()
                .unwrap_or_else(|| MALFORMED.into()),
            OpenAIError::ApiError(_) => PROVIDER_ERROR.into(),
            _ => MALFORMED.into(),
        }
    }
}

pub(super) fn sdk_client(
    url: Url,
    settings: &Settings,
) -> Result<(Client<OpenAIConfig>, WireState), String> {
    let http = super::client(&url, None)?;
    let config = OpenAIConfig::new()
        .with_api_base(url.origin().ascii_serialization())
        .with_api_key("")
        .with_org_id("")
        .with_project_id("");
    let sdk = Client::build(http.clone(), config);
    let settings = settings.clone();
    let wire = WireState::default();
    let state = wire.clone();
    let service = tower::service_fn(move |factory: HttpRequestFactory| {
        let (http, url, settings, wire) =
            (http.clone(), url.clone(), settings.clone(), state.clone());
        async move {
            let mut request = factory.build().await?;
            *request.url_mut() = url;
            request.headers_mut().remove(header::AUTHORIZATION);
            request.headers_mut().insert(
                header::ACCEPT,
                header::HeaderValue::from_static("text/event-stream"),
            );
            let mut recovery = Recovery::None;
            let response = super::send(
                RequestBuilder::from_parts(http, request),
                &settings,
                &mut recovery,
            )
            .await
            .map_err(|message| OpenAIError::Boxed(Box::new(wire.fail(message, recovery))))?;
            bounded_response(
                response,
                wire,
                settings.api_mode == super::ApiMode::ChatCompletions,
            )
        }
    });
    Ok((sdk.with_http_service(service), wire))
}

fn bounded_response(
    response: Response,
    wire: WireState,
    require_done: bool,
) -> Result<Response, OpenAIError> {
    let mut types = response.headers().get_all(header::CONTENT_TYPE).iter();
    let is_sse = types.next().and_then(|v| v.to_str().ok()).is_some_and(|s| {
        s.split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("text/event-stream")
    }) && types.next().is_none();
    let problem = if response.status() != StatusCode::OK {
        Some("翻译服务未返回 HTTP 200 流式响应。")
    } else if !is_sse {
        Some("服务没有返回 text/event-stream 响应，请检查 API 地址和协议。")
    } else if response
        .content_length()
        .is_some_and(|n| n > MAX_RESPONSE as u64)
    {
        Some(TOO_LARGE)
    } else {
        None
    };
    if let Some(problem) = problem {
        return Err(OpenAIError::Boxed(Box::new(
            wire.fail(problem, Recovery::None),
        )));
    }
    let stream = stream::try_unfold(
        (response, WireLines::default(), wire),
        move |(mut response, mut lines, wire)| async move {
            loop {
                if let Some(line) = lines
                    .next_line()
                    .map_err(|s| wire.fail(s, Recovery::None))?
                {
                    return Ok::<_, io::Error>(Some((line, (response, lines, wire))));
                }
                match response.chunk().await {
                    Ok(Some(chunk)) => lines
                        .feed(chunk)
                        .map_err(|s| wire.fail(s, Recovery::None))?,
                    Ok(None) => {
                        lines.end().map_err(|s| wire.fail(s, Recovery::None))?;
                        if require_done {
                            return Err(wire.fail(INCOMPLETE, Recovery::None));
                        }
                        return Ok(None);
                    }
                    Err(error) => {
                        if lines.invalid_start()
                            || std::str::from_utf8(lines.content())
                                .is_err_and(|e| e.error_len().is_some())
                        {
                            return Err(wire.fail(MALFORMED, Recovery::None));
                        }
                        let message = if error.is_timeout() {
                            "译文流长时间没有响应。"
                        } else {
                            INCOMPLETE
                        };
                        return Err(wire.fail(message, Recovery::Interrupted));
                    }
                }
            }
        },
    );
    Ok(http::Response::new(Body::wrap_stream(stream)).into())
}

#[derive(Default)]
struct WireLines {
    pending: Bytes,
    line: Vec<u8>,
    total: usize,
    event: usize,
    started: bool,
    skip_lf: bool,
    ended_event: bool,
}

impl WireLines {
    fn feed(&mut self, bytes: Bytes) -> Result<(), &'static str> {
        if bytes.len() > MAX_RESPONSE - self.total {
            return Err(TOO_LARGE);
        }
        self.total += bytes.len();
        self.pending = bytes;
        Ok(())
    }

    fn content(&self) -> &[u8] {
        if self.started {
            &self.line
        } else {
            self.line
                .strip_prefix(b"\xef\xbb\xbf")
                .unwrap_or(&self.line)
        }
    }

    fn invalid_start(&self) -> bool {
        !self.started
            && (self.content().starts_with(b"\xef\xbb\xbf")
                || matches!(self.content().trim_ascii_start().first(), Some(b'{' | b'[')))
    }

    fn charge(&mut self, n: usize) -> Result<(), &'static str> {
        if n > MAX_EVENT - self.event {
            return Err(TOO_LARGE);
        }
        self.event += n;
        Ok(())
    }

    fn next_line(&mut self) -> Result<Option<Bytes>, &'static str> {
        if self.pending.is_empty() {
            return Ok(None);
        }
        if mem::take(&mut self.skip_lf) && self.pending.starts_with(b"\n") {
            self.charge(1)?;
            self.pending.advance(1);
            if self.pending.is_empty() {
                return Ok(None);
            }
        }
        if mem::take(&mut self.ended_event) {
            self.event = 0;
        }
        let n = self
            .pending
            .iter()
            .position(|b| matches!(b, b'\r' | b'\n'))
            .unwrap_or(self.pending.len());
        self.charge(n + usize::from(n < self.pending.len()))?;
        self.line.extend_from_slice(&self.pending.split_to(n));
        if self.pending.is_empty() {
            return Ok(None);
        }
        self.skip_lf = self.pending.get_u8() == b'\r';
        if self.invalid_start() {
            return Err(MALFORMED);
        }
        let content = self.content();
        std::str::from_utf8(content).map_err(|_| MALFORMED)?;
        if matches!(
            content.strip_prefix(b"event:").map(<[u8]>::trim_ascii),
            Some(b"error" | b"response.failed")
        ) {
            return Err(PROVIDER_ERROR);
        }
        let start = self.line.len() - content.len();
        self.ended_event = content.is_empty();
        self.started = true;
        self.line.push(b'\n');
        Ok(Some(Bytes::from(mem::take(&mut self.line)).slice(start..)))
    }

    fn end(&self) -> Result<(), &'static str> {
        if self.invalid_start() || std::str::from_utf8(self.content()).is_err() {
            return Err(MALFORMED);
        }
        if self.event != 0 && !self.ended_event {
            return Err(INCOMPLETE);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(lines: &mut WireLines, bytes: Bytes) -> Result<Vec<u8>, &'static str> {
        lines.feed(bytes)?;
        let mut output = Vec::new();
        while let Some(line) = lines.next_line()? {
            output.extend_from_slice(&line);
        }
        Ok(output)
    }

    #[test]
    fn split_bom_multibyte_and_all_sse_newlines() {
        let input = "\u{feff}: 你好\r\n\r\ndata: x\rdata: y\n\r".as_bytes();
        for n in 1..=input.len() {
            let mut lines = WireLines::default();
            let mut output = Vec::new();
            for chunk in input.chunks(n) {
                output.extend(drain(&mut lines, Bytes::copy_from_slice(chunk)).unwrap());
            }
            assert_eq!(output, ": 你好\n\ndata: x\ndata: y\n\n".as_bytes());
            lines.end().unwrap();
        }
    }

    #[test]
    fn bounds_lines_and_multiline_events_before_forwarding() {
        let mut lines = WireLines::default();
        let long = Bytes::from(vec![b'x'; MAX_EVENT / 2]);
        assert!(drain(&mut lines, long.clone()).unwrap().is_empty());
        assert!(drain(&mut lines, long.clone()).unwrap().is_empty());
        assert_eq!(drain(&mut lines, Bytes::from_static(b"!")), Err(TOO_LARGE));

        let mut lines = WireLines::default();
        drain(&mut lines, long.clone()).unwrap();
        drain(&mut lines, Bytes::from_static(b"\n")).unwrap();
        assert_eq!(drain(&mut lines, long), Err(TOO_LARGE));
    }

    #[test]
    fn bounds_total_even_for_short_ignored_events() {
        let mut event = vec![b'x'; MAX_EVENT - 2];
        event[0] = b':';
        event.extend_from_slice(b"\n\n");
        let event = Bytes::from(event);
        let mut lines = WireLines::default();
        for _ in 0..(MAX_RESPONSE / event.len()) {
            assert_eq!(drain(&mut lines, event.clone()).unwrap().len(), event.len());
        }
        assert_eq!(drain(&mut lines, Bytes::from_static(b"\n")), Err(TOO_LARGE));
    }

    #[test]
    fn malformed_non_sse_and_truncated_event_are_distinct() {
        for payload in [
            b"  {\"error\":1}".as_slice(),
            b"[{}]\r",
            b"data: \xff\n",
            b"\xef\xbb\xbf\xef\xbb\xbf: hello\n",
        ] {
            let mut lines = WireLines::default();
            assert_eq!(
                drain(&mut lines, Bytes::copy_from_slice(payload)).and_then(|_| lines.end()),
                Err(MALFORMED)
            );
        }
        let mut lines = WireLines::default();
        drain(&mut lines, Bytes::from_static(b"data: \"partial\"\n")).unwrap();
        assert_eq!(lines.end(), Err(INCOMPLETE));
    }

    #[tokio::test]
    async fn sdk_stops_before_reading_physical_eof_only_with_a_done_marker() {
        use futures_util::StreamExt;
        for (ending, expected) in [
            ("", Err(INCOMPLETE.to_string())),
            ("data: [DONE]\r\n\r\n", Ok(())),
        ] {
            let mut server = mockito::Server::new_async().await;
            let route = "/v1/chat/completions?api-version=2026";
            let event = r#"{"id":"cmpl-test","object":"chat.completion.chunk","created":1,"model":"test","choices":[{"index":0,"delta":{"content":"OK"},"finish_reason":"stop"}]}"#;
            let mock = server
                .mock("POST", route)
                .match_header("authorization", mockito::Matcher::Missing)
                .with_header("content-type", "text/event-stream")
                .with_body(format!("\u{feff}data: {event}\r\n\r\n{ending}"))
                .expect(1)
                .create_async()
                .await;
            let (sdk, wire) = sdk_client(
                Url::parse(&(server.url() + route)).unwrap(),
                &Settings::default(),
            )
            .unwrap();
            let mut recovery = Recovery::None;
            let mut stream = sdk.chat().create_stream(Default::default()).await.unwrap();
            let chunk = stream.next().await.unwrap().unwrap();
            assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("OK"));
            let end = match stream.next().await {
                Some(Err(e)) => Err(wire.error(e, &mut recovery)),
                None => wire.check(&mut recovery),
                Some(Ok(_)) => panic!("unexpected extra chunk"),
            };
            assert_eq!(end, expected);
            assert!(recovery == Recovery::None);
            mock.assert_async().await;
        }
    }

    #[tokio::test]
    async fn sdk_body_adapter_retains_safe_wire_reason() {
        let body = Body::wrap_stream(stream::iter([
            Ok(Bytes::from_static(b"data: ok\n\n")),
            Err(io::Error::other("private error contents")),
        ]));
        let response = http::Response::builder()
            .header(header::CONTENT_TYPE, "Text/Event-Stream; charset=utf-8")
            .body(body)
            .unwrap()
            .into();
        let wire = WireState::default();
        let mut response = bounded_response(response, wire.clone(), false).unwrap();
        let mut output = Vec::new();
        while let Ok(Some(bytes)) = response.chunk().await {
            output.extend_from_slice(&bytes);
        }
        assert_eq!(output, b"data: ok\n\n");
        let mut recovery = Recovery::None;
        assert_eq!(wire.check(&mut recovery), Err(INCOMPLETE.into()));
        assert!(recovery == Recovery::Interrupted);
        let invalid = serde_json::from_str::<u8>("oops").unwrap_err();
        assert_eq!(
            wire.error(
                OpenAIError::JSONDeserialize(invalid, "private".into()),
                &mut recovery
            ),
            MALFORMED
        );
        assert!(recovery == Recovery::None);
    }
}
