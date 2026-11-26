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

#[gpui_kit::test]
fn model_choices_track_the_current_settings_draft(cx: &mut TestAppContext) {
    let mut server = mockito::Server::new();
    let endpoint = format!("{}/v1", server.url());
    let leaked_key = server
        .mock("GET", "/v1/models")
        .match_header("authorization", "Bearer test-original")
        .expect(0)
        .create();
    let catalog = server
        .mock("GET", "/v1/models")
        .match_header("authorization", "Bearer test-replacement")
        .expect(3)
        .with_header("content-type", "application/json")
        .with_body(r#"{"data":[{"id":"from-server-illustrator","output_modalities":["image"]},{"id":"gpt-image-2.6-flare"},{"id":"from-server"},{"id":"other-choice"}]}"#)
        .create();
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
                configured_view(
                    window,
                    cx,
                    Ok((saved, Preferences::default(), Default::default())),
                )
            },
        )
        .unwrap()
    });
    cx.update_window(handle, |_, w, cx| {
        w.click("settings", cx);
        assert!(view.read(cx).form.models.is_none());
        view.update(cx, |v, cx| {
            v.switch_provider("DeepSeek", w, cx);
            assert!(v.draft(cx).api_key.is_empty());
            assert!(!v.draft(cx).reasoning);
            v.form.model.reset("my-deepseek", w, cx);
            v.switch_provider(crate::provider::CUSTOM, w, cx);
            let restored = v.draft(cx);
            assert_eq!(restored.endpoint, "http://127.0.0.1:1/v1");
            assert!(restored.api_key == "test-original");
            assert_eq!(restored.model, "custom");
            assert_eq!(restored.api_mode, ApiMode::Responses);
            assert!(restored.reasoning);
            v.switch_provider("DeepSeek", w, cx);
            assert_eq!(v.draft(cx).model, "my-deepseek");
            v.switch_provider(crate::provider::CUSTOM, w, cx);
        });
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
    leaked_key.assert();
    catalog.assert();
}
