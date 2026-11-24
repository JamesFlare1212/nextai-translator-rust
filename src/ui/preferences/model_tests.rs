use super::{ApiMode, Models, Preferences, Settings, configured_view, validate_draft_origin};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};

#[test]
fn only_the_original_origin_can_reuse_the_saved_credential() {
    let saved = Settings {
        endpoint: "https://example.test/v1".into(),
        api_key: "test-original".into(),
        ..Default::default()
    };
    let mut draft = saved.clone();
    draft.endpoint = "https://EXAMPLE.test:443/api/responses".into();
    assert!(validate_draft_origin(&saved, &draft).is_ok());
    for origin in [
        "https://other.test",
        "https://example.test:8443",
        "http://example.test",
        "invalid",
    ] {
        draft.endpoint = origin.into();
        draft.api_key = format!(" {} ", saved.api_key);
        let error = validate_draft_origin(&saved, &draft).unwrap_err();
        assert!(!error.contains(&saved.api_key));
        draft.api_key = "test-replacement".into();
        assert!(validate_draft_origin(&saved, &draft).is_ok());
        draft.api_key.clear();
        assert!(validate_draft_origin(&saved, &draft).is_ok());
    }
}

fn serve_catalog() -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1", server.local_addr().unwrap());
    let task = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        let headers = BufReader::new(&stream)
            .lines()
            .map(Result::unwrap)
            .take_while(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(headers.starts_with("GET /v1/models "));
        assert!(
            headers
                .to_lowercase()
                .contains("authorization: bearer test-replacement")
        );
        let body = r#"{"data":[{"id":"from-server"},{"id":"other-choice"}]}"#;
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    });
    (endpoint, task)
}

#[gpui_kit::test]
fn model_choices_track_the_current_settings_draft(cx: &mut TestAppContext) {
    let (endpoint, server) = serve_catalog();
    let (handle, view) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Default::default(),
                    size: size(px(540.), px(600.)),
                })),
                ..Default::default()
            },
            cx,
            |window, cx| {
                let saved = Settings {
                    endpoint: "http://127.0.0.1:1/v1".into(),
                    api_key: "test-original".into(),
                    model: "custom".into(),
                    api_mode: ApiMode::Responses,
                    reasoning: true,
                };
                configured_view(window, cx, Ok((saved, Preferences::default())))
            },
        )
        .unwrap()
    });
    cx.update_window(handle, |_, w, cx| {
        w.click("settings", cx);
        assert!(view.read(cx).form.models.is_none());
        assert_eq!(view.read(cx).form.model_name(cx), "custom");
        assert!(w.find("model-picker").bounds().size.width > px(450.));
        assert!(
            w.find("refresh-models").bounds().left() >= w.find("model-picker").bounds().right()
        );
        view.read(cx)
            .endpoint
            .clone()
            .update(cx, |s, cx| s.set_value(endpoint, w, cx));
        w.click("model-picker", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        assert!(matches!(
            view.read(cx).current_models(cx),
            Some(Models::Error(_))
        ));
        w.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        view.read(cx)
            .api_key
            .clone()
            .update(cx, |s, cx| s.set_value("test-replacement", w, cx));
        assert!(view.read(cx).current_models(cx).is_none());
        w.click("refresh-models", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        match view.read(cx).current_models(cx) {
            Some(Models::Ready) => (),
            Some(Models::Error(e)) => panic!("catalog failed: {e}"),
            Some(Models::Loading { .. }) => panic!("catalog not finished"),
            None => panic!("no lookup on re-open"),
        };
        assert_eq!(view.read(cx).form.model_name(cx), "custom");
        w.click("model-picker", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        w.input("from-ser", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    server.join().unwrap();
    cx.update_window(handle, |_, w, cx| {
        assert_eq!(view.read(cx).form.model_name(cx), "from-server");
        w.click("model-picker", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        w.input("offline-custom", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        assert_eq!(view.read(cx).form.model_name(cx), "offline-custom");
        view.update(cx, |v, cx| {
            v.refresh_models(|_| Ok(vec!["stale".into()]), w, cx);
            v.back(w, cx);
            v.show_settings(w, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, _, cx| {
        assert!(view.read(cx).form.models.is_none());
        assert_eq!(view.read(cx).form.model_name(cx), "custom");
    })
    .unwrap();
}
