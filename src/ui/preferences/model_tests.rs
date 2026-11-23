use super::{ApiMode, Models, Preferences, Settings, configured_view, validate_draft_origin};
use gpui_kit::test::{TestQueryExt, TestWindowExt};
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
        assert_eq!(view.read(cx).form.api_mode(cx), ApiMode::Responses);
        assert!(view.read(cx).form.reasoning());
        w.click("reasoning", cx);
        assert!(!view.read(cx).form.reasoning());
        assert!(w.find_by_label("API 协议").bounds().right() <= px(540.));
        view.update(cx, |v, cx| {
            v.endpoint
                .update(cx, |s, cx| s.set_value("http://127.0.0.1:2/v1", w, cx));
            v.refresh_models(
                |_| panic!("credential guard must reject before starting a worker"),
                w,
                cx,
            );
            assert!(matches!(v.current_models(cx), Some(Models::Error(_))));
            v.api_key
                .update(cx, |s, cx| s.set_value("test-replacement", w, cx));
            v.refresh_models(
                |draft| {
                    assert_eq!(draft.endpoint, "http://127.0.0.1:2/v1");
                    assert_eq!(draft.api_key, "test-replacement");
                    assert_eq!(draft.api_mode, ApiMode::Responses);
                    assert!(!draft.reasoning);
                    Ok(vec!["from-server".into()])
                },
                w,
                cx,
            );
            assert!(matches!(v.current_models(cx), Some(Models::Loading { .. })));
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        assert!(matches!(
            view.read(cx).current_models(cx),
            Some(Models::Ready { .. })
        ));
        assert_eq!(view.read(cx).model.read(cx).value().as_ref(), "custom");
        assert!(w.find_by_label("模型").bounds().size.width > px(120.));
        assert!(w.find("fetch-models").bounds().right() <= px(540.));
        w.click("model-picker", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        w.press("down", cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        assert_eq!(view.read(cx).model.read(cx).value().as_ref(), "from-server");
        view.update(cx, |v, cx| {
            v.model
                .update(cx, |s, cx| s.set_value("edited-custom", w, cx));
            assert!(matches!(v.current_models(cx), Some(Models::Ready { .. })));
            v.api_key.update(cx, |s, cx| s.set_value("changed", w, cx));
            assert!(v.current_models(cx).is_none());
        });
        w.render_frame(cx);
        assert!(w.try_find("model-picker").is_none());
        view.update(cx, |v, cx| {
            v.refresh_models(|_| Ok(vec!["stale".into()]), w, cx);
            v.back(w, cx);
            v.show_settings(w, cx);
            assert!(v.form.reasoning());
            assert_eq!(v.form.api_mode(cx), ApiMode::Responses);
            assert!(v.form.models.is_none());
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        assert!(w.try_find("model-picker").is_none());
        assert_eq!(view.read(cx).model.read(cx).value().as_ref(), "custom");
    })
    .unwrap();
}
