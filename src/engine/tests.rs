use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
};

const KEY: &str = "fake-test-key-do-not-display";

fn server(
    serve: impl FnOnce(TcpStream, String, serde_json::Value) + Send + 'static,
) -> (Settings, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(&stream);
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if line == "\r\n" {
                break;
            }
            headers.push_str(&line);
        }
        let len: usize = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap_or(0);
        let mut body = vec![0; len];
        reader.read_exact(&mut body).unwrap();
        serve(
            stream,
            headers,
            if body.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_slice(&body).unwrap()
            },
        );
    });
    (
        Settings {
            endpoint,
            model: "test-model".into(),
            api_key: KEY.into(),
            ..Default::default()
        },
        handle,
    )
}

fn response(mut stream: TcpStream, status: &str, kind: &str, body: &str) {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
}

fn chunk(stream: &mut TcpStream, bytes: &[u8]) {
    write!(stream, "{:x}\r\n", bytes.len()).unwrap();
    stream.write_all(bytes).unwrap();
    stream.write_all(b"\r\n").unwrap();
}

fn consume_post(stream: &TcpStream) {
    let mut reader = BufReader::new(stream);
    let headers = reader
        .by_ref()
        .lines()
        .map(Result::unwrap)
        .take_while(|l| !l.is_empty())
        .collect::<Vec<_>>();
    let len = headers
        .iter()
        .find_map(|l| {
            l.to_lowercase()
                .strip_prefix("content-length: ")
                .map(|n| n.parse().unwrap())
        })
        .unwrap();
    reader.read_exact(&mut vec![0; len]).unwrap();
}

#[test]
fn interrupted_stream_replaces_partial_translation_once() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let settings = Settings {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        ..Default::default()
    };
    let worker = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        consume_post(&first);
        first
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
            .unwrap();
        chunk(
            &mut first,
            b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        );
        drop(first);
        let (second, _) = listener.accept().unwrap();
        consume_post(&second);
        response(
            second,
            "200 OK",
            "application/json",
            r#"{"choices":[{"message":{"content":"complete"}}]}"#,
        );
    });
    let mut output = String::new();
    let mut retries = 0;
    translate_with_recovery(
        &settings,
        "Chinese",
        "English",
        "你好",
        &AtomicBool::new(false),
        |event| match event {
            StreamUpdate::Append(s) => output.push_str(s),
            StreamUpdate::UsingSse => panic!("unexpected protocol switch"),
            StreamUpdate::Retrying => {
                assert_eq!(output, "partial");
                retries += 1;
            }
            StreamUpdate::Replace(s) => output = s.to_owned(),
        },
    )
    .unwrap();
    worker.join().unwrap();
    assert_eq!((retries, output.as_str()), (1, "complete"));
}

#[test]
fn websocket_responses_upgrade_streams_and_finishes() {
    use tokio_tungstenite::tungstenite::{
        Message, accept_hdr,
        handshake::server::{Request, Response},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let settings = Settings {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        api_mode: ApiMode::ResponsesWs,
        api_key: KEY.into(),
        ..Default::default()
    };
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        #[expect(
            clippy::result_large_err,
            reason = "tungstenite fixes the callback error type"
        )]
        let mut ws = accept_hdr(stream, |r: &Request, response: Response| {
            assert_eq!(r.uri().path(), "/v1/responses");
            assert_eq!(r.headers()["authorization"], format!("Bearer {KEY}"));
            Ok(response)
        })
        .unwrap();
        let msg = ws.read().unwrap();
        let body: serde_json::Value = serde_json::from_str(msg.to_text().unwrap()).unwrap();
        assert_eq!(body["type"], "response.create");
        assert_eq!(body["input"][0]["content"], "你好");
        assert!(body.get("stream").is_none());
        for event in [
            json!({"type":"response.output_text.delta","delta":"Hello"}),
            json!({"type":"response.completed","response":{"status":"completed","output_text":"Hello!"}}),
        ] {
            ws.send(Message::text(event.to_string())).unwrap();
        }
    });
    let mut text = String::new();
    translate(
        &settings,
        "Chinese",
        "English",
        "你好",
        &AtomicBool::new(false),
        |s| text.push_str(s),
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(text, "Hello!");
}

#[test]
fn rejected_ws_route_falls_back_to_responses_on_the_same_origin() {
    use tokio_tungstenite::tungstenite::{Message, accept};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let settings = Settings {
        endpoint: format!("http://{}/proxy/v2", listener.local_addr().unwrap()),
        api_mode: ApiMode::ResponsesWs,
        api_key: KEY.into(),
        ..Default::default()
    };
    let worker = thread::spawn(move || {
        let mut ws = accept(listener.accept().unwrap().0).unwrap();
        let body: serde_json::Value =
            serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
        ws.send(Message::text(json!({"type":"error", "status":503, "error":{"type":"new_api_error", "code":"model_not_found", "message":KEY}}).to_string())).unwrap();
        drop(ws);
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&stream);
        let headers: Vec<_> = reader
            .by_ref()
            .lines()
            .map(Result::unwrap)
            .take_while(|l| !l.is_empty())
            .collect();
        assert_eq!(headers[0], "POST /proxy/v2/responses HTTP/1.1");
        assert!(
            headers
                .iter()
                .any(|h| h.eq_ignore_ascii_case(&format!("authorization: Bearer {KEY}")))
        );
        let len = headers
            .iter()
            .find_map(|h| {
                h.to_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|n| n.parse().ok())
            })
            .unwrap();
        let mut raw = vec![0; len];
        reader.read_exact(&mut raw).unwrap();
        let retry: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(retry["input"], body["input"][0]["content"]);
        assert_eq!(retry["model"], body["model"]);
        assert_eq!(retry["instructions"], body["instructions"]);
        assert_eq!(retry["stream"], true);
        response(
            stream,
            "200 OK",
            "text/event-stream",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"OK\"}\n\ndata: {\"type\":\"response.completed\"}\n\n",
        );
    });
    let mut out = String::new();
    let mut switched = 0;
    translate_with_recovery(
        &settings,
        "English",
        "French",
        "A sentence.",
        &AtomicBool::new(false),
        |ev| match ev {
            StreamUpdate::UsingSse => switched += 1,
            StreamUpdate::Append(s) => {
                assert_eq!(switched, 1);
                out.push_str(s);
            }
            _ => panic!("unexpected stream restart"),
        },
    )
    .unwrap();
    worker.join().unwrap();
    assert_eq!(out, "OK");
    assert_eq!(settings.api_mode, ApiMode::ResponsesWs);
}

#[test]
fn gateway_failure_retries_once_before_output() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let settings = Settings {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        ..Default::default()
    };
    let worker = thread::spawn(move || {
        for status in ["503 Service Unavailable", "200 OK"] {
            let (stream, _) = listener.accept().unwrap();
            consume_post(&stream);
            response(
                stream,
                status,
                "application/json",
                r#"{"choices":[{"message":{"content":"hello"}}]}"#,
            );
        }
    });
    let mut result = String::new();
    translate(
        &settings,
        "Chinese",
        "English",
        "你好",
        &AtomicBool::new(false),
        |s| result.push_str(s),
    )
    .unwrap();
    worker.join().unwrap();
    assert_eq!(result, "hello");
}

#[test]
fn streams_multiline_events_and_utf8_across_http_chunks_before_completion() {
    let (ack, first_delta) = mpsc::channel();
    let (settings, handle) = server(move |mut stream, headers, body| {
        assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains(&format!("authorization: bearer {KEY}\r\n"))
        );
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["stream"], true);
        assert_eq!(
            body["messages"][1],
            json!({"role":"user","content":"Hello,\nworld!"})
        );
        let prompt = body["messages"][0]["content"].as_str().unwrap();
        assert!(prompt.contains("Detect the source language") && prompt.contains("French"));
        assert!(!prompt.contains("dictionary") && !body.to_string().contains(KEY));

        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
        let first = "\u{feff}: heartbeat\r\n\r\ndata: {\"choices\":[]}\r\n\r\nevent: message\r\ndata: {\"choices\":[\r\ndata: {\"delta\":{\"content\":\"你\"}}]}\r\n\r\n";
        let split = first.find('你').unwrap() + 1;
        chunk(&mut stream, &first.as_bytes()[..split]);
        chunk(&mut stream, &first.as_bytes()[split..]);
        first_delta.recv_timeout(Duration::from_secs(5)).unwrap();
        chunk(
            &mut stream,
            "data: {\"choices\":[{\"delta\":{\"content\":\"好\"},\"finish_reason\":\"stop\"}]}\n\n"
                .as_bytes(),
        );
    });
    let mut parts = Vec::new();
    translate(
        &settings,
        "Auto",
        "French",
        "Hello,\nworld!",
        &AtomicBool::new(false),
        |part| {
            parts.push(part.to_owned());
            if parts.len() == 1 {
                ack.send(()).unwrap();
            }
        },
    )
    .unwrap();
    handle.join().unwrap();
    assert_eq!(parts, ["你", "好"]);
}

#[test]
fn keyless_model_accepts_json_and_receives_dictionary_instructions_for_one_word() {
    let (mut settings, handle) = server(|stream, headers, body| {
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        let prompt = body["messages"][0]["content"].as_str().unwrap();
        for expected in [
            "English into Chinese",
            "dictionary entry in Chinese",
            "original form",
            "parts of speech",
            "three bilingual example sentences",
            "etymology",
        ] {
            assert!(prompt.contains(expected), "missing {expected}");
        }
        assert_eq!(body["messages"][1]["content"], "rivers");
        response(
            stream,
            "200 OK",
            "application/json",
            r#"{"choices":[{"message":{"role":"assistant","content":"河流\nriver, noun"},"finish_reason":"stop"}]}"#,
        );
    });
    settings.api_key.clear();
    let mut parts = Vec::new();
    translate(
        &settings,
        "English",
        "Chinese",
        "rivers",
        &AtomicBool::new(false),
        |part| parts.push(part.to_owned()),
    )
    .unwrap();
    handle.join().unwrap();
    assert_eq!(parts, ["河流\nriver, noun"]);
}

fn exchange(status: &'static str, body: String) -> (String, Result<(), String>) {
    exchange_mode(ApiMode::ChatCompletions, status, body)
}

fn exchange_mode(
    api_mode: ApiMode,
    status: &'static str,
    body: String,
) -> (String, Result<(), String>) {
    let (mut settings, handle) =
        server(move |stream, _, _| response(stream, status, "text/plain", &body));
    settings.api_mode = api_mode;
    let mut text = String::new();
    let result = translate(
        &settings,
        "English",
        "German",
        "Some text",
        &AtomicBool::new(false),
        |part| text.push_str(part),
    );
    handle.join().unwrap();
    (text, result)
}

#[test]
fn done_marker_and_incomplete_results_are_distinct() {
    let partial = "data: {\"choices\":[{\"delta\":{\"content\":\"Hallo\"}}]}\n\n";
    let (text, result) = exchange("200 OK", format!("{partial}data: [DONE]\n\n"));
    assert_eq!((text.as_str(), result), ("Hallo", Ok(())));
    let (text, result) = exchange("200 OK", partial.into());
    assert_eq!(text, "Hallo");
    assert!(result.unwrap_err().contains("提前结束"));
    let (_, result) = exchange("200 OK", "data: [DONE]\n\n".into());
    assert_eq!(result.unwrap_err(), EMPTY);
    let (_, result) = exchange(
        "200 OK",
        "data: {\"choices\":[{\"finish_reason\":\"length\"}]}\n\n".into(),
    );
    assert!(result.unwrap_err().contains("截断"));
}

#[test]
fn handles_http_json_sse_and_malformed_errors_without_displaying_secrets() {
    for (status, body, expected) in [
        ("401 Unauthorized", KEY.to_owned(), "HTTP 401"),
        (
            "200 OK",
            format!(r#"{{"error":{{"message":"{KEY}"}}}}"#),
            PROVIDER_ERROR,
        ),
        (
            "200 OK",
            format!("data: {{\"error\":\"{KEY}\"}}\n\n"),
            PROVIDER_ERROR,
        ),
        (
            "200 OK",
            format!("event: error\ndata: {KEY}\n\n"),
            PROVIDER_ERROR,
        ),
        ("200 OK", format!("data: {KEY}\n\n"), MALFORMED),
    ] {
        let (text, result) = exchange(status, body);
        assert!(text.is_empty());
        let err = result.unwrap_err();
        assert!(err.contains(expected) && !err.contains(KEY));
    }
}

#[test]
fn cancels_while_waiting_for_headers_and_disconnects() {
    let cancel = Arc::new(AtomicBool::new(false));
    let request_cancel = cancel.clone();
    let (settings, handle) = server(move |mut stream, _, _| {
        request_cancel.store(true, Ordering::Release);
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(n) => assert_eq!(n, 0),
            Err(e) => assert!(matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            )),
        }
    });
    let result = translate(&settings, "Auto", "English", "Hallo", &cancel, |_| {
        panic!("unexpected delta")
    });
    assert_eq!(result.unwrap_err(), CANCELLED);
    handle.join().unwrap();
}

#[test]
fn cancellation_in_callback_skips_even_already_buffered_deltas() {
    let (settings, handle) = server(|stream, _, _| {
        response(
            stream,
            "200 OK",
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{\"content\":\"one\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"two\"}}]}\n\ndata: [DONE]\n\n",
        );
    });
    let cancel = AtomicBool::new(false);
    let mut text = String::new();
    let result = translate(&settings, "Auto", "Chinese", "one two", &cancel, |part| {
        text.push_str(part);
        cancel.store(true, Ordering::Release);
    });
    assert_eq!(result.unwrap_err(), CANCELLED);
    assert_eq!(text, "one");
    handle.join().unwrap();
}

#[test]
fn never_follows_a_redirect_with_credentials_or_user_text() {
    let dest = TcpListener::bind("127.0.0.1:0").unwrap();
    let location = format!("http://{}/elsewhere", dest.local_addr().unwrap());
    let (settings, handle) = server(move |mut stream, _, _| {
        write!(
            stream,
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
        )
        .unwrap();
    });
    let err = translate(
        &settings,
        "Auto",
        "Chinese",
        "secret text",
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap_err();
    assert!(err.contains("HTTP 307"));
    handle.join().unwrap();
    dest.set_nonblocking(true).unwrap();
    assert_eq!(
        dest.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn endpoint_hint_only_shows_the_protocol_change() {
    use ApiMode::{ChatCompletions, Responses};
    for (endpoint, mode, hint) in [
        (
            "https://example.org",
            ChatCompletions,
            Some("/v1/chat/completions"),
        ),
        ("https://example.org/", Responses, Some("v1/responses")),
        ("https://example.org/v2", Responses, Some("/responses")),
        ("https://example.org/v1/responses", Responses, None),
        (
            "https://example.org/v1/responses",
            ChatCompletions,
            Some(" → /v1/chat/completions"),
        ),
        (
            "https://example.org/v1?version=2",
            Responses,
            Some(" → /v1/responses"),
        ),
        ("", Responses, None),
    ] {
        assert_eq!(endpoint_hint(endpoint, mode).as_deref(), hint);
    }
}

#[test]
fn defaults_and_route_normalization_keep_each_api_family_together() {
    let mut settings = Settings::default();
    assert_eq!(settings.endpoint, "https://new-api.jamesflare.com");
    assert_eq!(settings.model, "gpt-6-luna");
    assert_eq!(settings.api_mode, ApiMode::ChatCompletions);
    assert!(!settings.reasoning);
    for (path, base) in [
        ("", "/v1"),
        ("/", "/v1"),
        ("/v1/", "/v1"),
        ("/v1/chat/completions", "/v1"),
        ("/responses/", ""),
        ("/proxy/site%20one/v2/responses", "/proxy/site%20one/v2"),
        ("/v1beta/openai/chat/completions/", "/v1beta/openai"),
        ("/api/v2", "/api/v2"),
    ] {
        settings.endpoint = format!("https://api.example{path}?api-version=42");
        for (mode, route, serialized) in [
            (
                ApiMode::ChatCompletions,
                "chat/completions",
                "chat_completions",
            ),
            (ApiMode::Responses, "responses", "responses"),
        ] {
            settings.api_mode = mode;
            assert_eq!(serde_json::to_value(mode).unwrap(), json!(serialized));
            assert_eq!(
                serde_json::from_value::<ApiMode>(json!(serialized)).unwrap(),
                mode
            );
            let url = validated_url(&settings).unwrap();
            assert_eq!(
                url.as_str(),
                format!("https://api.example{base}/{route}?api-version=42")
            );
            assert_eq!(
                api_url(&settings, "models").unwrap().as_str(),
                format!("https://api.example{base}/models?api-version=42")
            );
        }
    }
}

#[test]
fn reasoning_is_requested_explicitly_without_breaking_plain_chat_models() {
    let mut settings = Settings::default();
    for api_mode in [ApiMode::ChatCompletions, ApiMode::Responses] {
        settings.api_mode = api_mode;
        let (field, path, other) = match api_mode {
            ApiMode::ChatCompletions => ("reasoning_effort", "/reasoning_effort", "reasoning"),
            ApiMode::Responses | ApiMode::ResponsesWs => {
                ("reasoning", "/reasoning/effort", "reasoning_effort")
            }
        };
        for (enabled, model, effort) in [
            (true, "o3", Some("medium")),
            (false, "gpt-6-luna", Some("none")),
            (false, "gpt-4o-mini", None),
            (false, "mistral", None),
        ] {
            settings.reasoning = enabled;
            let body = translation_body(&settings, model, "prompt", "input");
            assert_eq!(body.pointer(path).and_then(|e| e.as_str()), effort);
            assert_eq!(body.get(field).is_some(), effort.is_some());
            assert!(body.get(other).is_none());
        }
    }
    let body = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hidden\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"visible\"},\"finish_reason\":\"stop\"}]}\n\n";
    assert_eq!(exchange("200 OK", body.into()), ("visible".into(), Ok(())));
}

#[test]
fn responses_streams_only_output_text_and_reconciles_all_done_snapshots() {
    let (ack, first_delta) = mpsc::channel();
    let (mut settings, handle) = server(move |mut stream, headers, body| {
        assert!(headers.starts_with("POST /v1/responses HTTP/1.1\r\n"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains(&format!("authorization: bearer {KEY}\r\n"))
        );
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["stream"], true);
        assert_eq!(body["input"], "Hello there");
        assert!(body["instructions"].as_str().unwrap().contains("French"));
        assert_eq!(body["reasoning"], json!({"effort":"medium"}));
        assert!(body.get("messages").is_none());
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
        let events = [
            json!({"type":"response.reasoning_summary_text.delta", "delta":KEY}),
            json!({"type":"response.reasoning_text.done", "text":KEY}),
            json!({"type":"response.output_item.done", "output_index":0,"item":{"type":"reasoning","summary":[{"type":"summary_text","text":KEY}]}}),
            json!({"type":"response.output_text.delta","output_index":1,"delta":"Bon"}),
        ];
        for e in events {
            chunk(&mut stream, format!("data: {e}\r\n\r\n").as_bytes());
        }
        first_delta.recv_timeout(Duration::from_secs(5)).unwrap();
        chunk(&mut stream, b"event: response.output_text.done\ndata: {\"output_index\":1,\"text\":\"Bonjour\"}\n\n");
        let part = json!({"type":"output_text", "text":"Bonjour"});
        let item = json!({"type":"message", "role":"assistant", "content":[part, {"type":"output_text","text":" !"}]});
        for e in [
            json!({"type":"response.content_part.done", "output_index":1, "content_index":0, "part":part}),
            json!({"type":"response.output_item.done", "output_index":1, "item":item}),
            json!({"type":"response.completed", "response":{"status":"completed","output":[{"type":"reasoning"},item]}}),
        ] {
            chunk(&mut stream, format!("data: {e}\n\n").as_bytes());
        }
    });
    settings.api_mode = ApiMode::Responses;
    settings.reasoning = true;
    let mut parts = Vec::new();
    translate(
        &settings,
        "Auto",
        "French",
        "Hello there",
        &AtomicBool::new(false),
        |part| {
            parts.push(part.to_owned());
            if parts.len() == 1 {
                ack.send(()).unwrap();
            }
        },
    )
    .unwrap();
    handle.join().unwrap();
    assert_eq!(parts, ["Bon", "jour", " !"]);
}

#[test]
fn responses_json_and_terminal_only_streams_handle_multiple_output_items() {
    let document = json!({"status":"completed", "output":[
        {"type":"reasoning","summary":[{"text":KEY}]},
        {"type":"message","role":"user","content":[{"type":"output_text","text":KEY}]},
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"你"},{"type":"output_text","text":"好"}]},
        {"type":"function_call", "arguments":KEY},
        {"type":"message","content":[{"type":"output_text","text":"\n世界"}]}
    ]});
    let event = json!({"type":"response.completed", "response":document});
    for body in [document.to_string(), format!("data: {event}\n\n")] {
        assert_eq!(
            exchange_mode(ApiMode::Responses, "200 OK", body),
            ("你好\n世界".into(), Ok(()))
        );
    }
    let (text, result) = exchange_mode(
        ApiMode::Responses,
        "200 OK",
        json!({"output":[{"type":"reasoning","content":[{"type":"reasoning_text","text":KEY}]}]})
            .to_string(),
    );
    assert!(text.is_empty());
    assert_eq!(result.unwrap_err(), EMPTY);
}

#[test]
fn responses_never_reports_failed_or_incomplete_output_as_success() {
    let partial = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"part\"}\n\n";
    for (ending, expected) in [
        (
            json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}),
            TRUNCATED,
        ),
        (
            json!({"type":"response.failed","response":{"error":{"message":KEY}}}),
            PROVIDER_ERROR,
        ),
        (json!({"type":"error","message":KEY}), PROVIDER_ERROR),
        (
            json!({"type":"response.refusal.delta","delta":KEY}),
            REFUSED,
        ),
        (
            json!({"type":"response.completed","response":{"status":"incomplete"}}),
            INCOMPLETE,
        ),
    ] {
        let (text, result) = exchange_mode(
            ApiMode::Responses,
            "200 OK",
            format!("{partial}data: {ending}\n\n"),
        );
        assert_eq!(text, "part");
        assert_eq!(result.unwrap_err(), expected);
    }
    for ending in ["", "data: [DONE]\n\n"] {
        let (text, result) =
            exchange_mode(ApiMode::Responses, "200 OK", format!("{partial}{ending}"));
        assert_eq!(text, "part");
        assert_eq!(result.unwrap_err(), INCOMPLETE);
    }
    for (body, expected) in [
        (
            json!({"status":"failed","error":{"message":KEY},"output_text":"partial"}),
            PROVIDER_ERROR,
        ),
        (
            json!({"status":"incomplete","incomplete_details":{"reason":"content_filter"},"output_text":"partial"}),
            FILTERED,
        ),
        (
            json!({"output":[{"type":"message","content":[{"type":"refusal","refusal":KEY}]}]}),
            REFUSED,
        ),
    ] {
        assert_eq!(
            exchange_mode(ApiMode::Responses, "200 OK", body.to_string()),
            (String::new(), Err(expected.into()))
        );
    }
}

#[test]
fn model_discovery_uses_sibling_get_and_does_not_require_a_selected_model() {
    let (mut settings, handle) = server(|stream, headers, body| {
        assert!(headers.starts_with("GET /v1/models?api-version=42 HTTP/1.1\r\n"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains(&format!("authorization: bearer {KEY}\r\n"))
        );
        assert!(body.is_null());
        response(
            stream,
            "200 OK",
            "application/json",
            r#"{"data":[{"id":"z"},{"id":"a"},{"id":"z"},{"id":""},{},{"id":"a\nb"}]}"#,
        );
    });
    settings.endpoint = settings
        .endpoint
        .replace("chat/completions", "responses/?api-version=42");
    settings.model.clear();
    assert_eq!(list_models(&settings).unwrap(), ["z", "a"]);
    handle.join().unwrap();
}

#[test]
fn model_discovery_hides_specialists_but_keeps_translation_candidates() {
    let body = json!({"data":[
        {"id":"gpt-6.1-terra"},
        {"id":"gpt-image-2.5-flare"}, {"id":"gpt-image-2.5-sunburst"},
        {"id":"gpt-image-2.6-flare"}, {"id":"openai/GPT-IMAGE-2.6-SUNBURST"},
        {"id":"google/gemini-2.5-flash-image"}, {"id":"black-forest-labs/FLUX.1-dev"},
        {"id":"dall-e-3"}, {"id":"veo-3.1"}, {"id":"text-embedding-3-small"},
        {"id":"bge-reranker-v2-m3"}, {"id":"whisper-1"}, {"id":"gpt-4o-mini-tts"},
        {"id":"omni-moderation-latest"}, {"id":"my-private-deployment"},
        {"id":"opaque-img", "architecture":{"output_modalities":["image"]}},
        {"id":"opaque-audio", "output_modalities":["audio"]},
        {"id":"opaque-embed", "supported_endpoint_types":["embeddings"]},
        {"id":"custom-vision", "architecture":{"input_modalities":["text","image"],"output_modalities":["text"]}},
        {"id":"image-understanding-chat", "supported_endpoint_types":["openai","openai-image"]},
        {"id":"Acme/fluxion-chat", "supported_endpoint_types":["future-api"]}
    ]});
    assert_eq!(
        parse_models(body.to_string().as_bytes()).unwrap(),
        [
            "gpt-6.1-terra",
            "my-private-deployment",
            "custom-vision",
            "image-understanding-chat",
            "Acme/fluxion-chat"
        ]
    );
}

#[test]
fn model_discovery_bounds_and_redacts_service_responses() {
    for (status, body, expected) in [
        ("403 Forbidden", KEY.into(), "HTTP 403"),
        (
            "200 OK",
            json!({"error":{"message":KEY}}).to_string(),
            PROVIDER_ERROR,
        ),
        ("200 OK", format!("not JSON: {KEY}"), "格式无效"),
        ("200 OK", " ".repeat(MAX_MODELS_BODY + 1), "列表过长"),
    ] {
        let (settings, handle) =
            server(move |stream, _, _| response(stream, status, "application/json", &body));
        let err = list_models(&settings).unwrap_err();
        assert!(err.contains(expected) && !err.contains(KEY));
        handle.join().unwrap();
    }
    let unsafe_settings = Settings {
        endpoint: "http://example.org/v1".into(),
        model: String::new(),
        ..Default::default()
    };
    assert!(list_models(&unsafe_settings).unwrap_err().contains("HTTPS"));
}

#[test]
fn responses_enforces_per_event_and_total_byte_limits() {
    for body in [
        format!("data: {}\n\n", "x".repeat(MAX_EVENT)),
        " ".repeat(MAX_RESPONSE + 1),
    ] {
        let (text, result) = exchange_mode(ApiMode::Responses, "200 OK", body);
        assert!(text.is_empty());
        assert_eq!(result.unwrap_err(), TOO_LARGE);
    }
}

#[test]
fn validates_settings_without_network_or_secret_disclosure() {
    let mut settings = Settings::default();
    for address in [
        "https://api.example/v1/chat/completions?api-version=2025-01",
        "http://127.0.0.42:11434/proxy",
        "http://localhost:8080/completions",
        "http://[::1]/v1/chat/completions",
    ] {
        settings.endpoint = address.into();
        validate(&settings).unwrap();
    }
    for address in [
        "http://api.example/v1/chat/completions",
        "http://localhost.example/proxy",
        "https://secret@example.org/path",
        "http://[::]/path",
        "file:///tmp/model",
        "https://example.org/path#token",
        "https://example.org/path?api_key=secret",
        "not a URL",
    ] {
        settings.endpoint = address.into();
        assert!(!validate(&settings).unwrap_err().contains("secret"));
    }
    settings = Settings::default();
    settings.api_key = format!("{KEY}\r\nHeader: value");
    assert!(!validate(&settings).unwrap_err().contains(KEY));
    settings = Settings::default();
    settings.model = " ".into();
    assert!(validate(&settings).is_err());
    assert_eq!(
        translate(
            &settings,
            "Auto",
            "English",
            "a",
            &AtomicBool::new(true),
            |_| {}
        )
        .unwrap_err(),
        CANCELLED
    );
}
