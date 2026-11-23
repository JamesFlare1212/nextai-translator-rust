use super::{Page, Settings, configured_view};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};

#[gpui_kit::test]
fn compose_clear_and_cancel_settings_at_minimum_size(cx: &mut TestAppContext) {
    let (handle, view) = open_test_window(cx);
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        w.input("She reads", cx);
        w.press("shift-enter", cx);
        w.input("a book.", cx);
        assert_eq!(
            view.read(cx).editor.read(cx).value().as_ref(),
            "She reads\na book."
        );
        assert!(view.read(cx).request.is_none());
        w.click("settings", cx);
        assert!(view.read(cx).page == Page::Settings);
        assert!(w.find("back").visible());
        assert!(w.find("save").bounds().right() <= px(540.));
        view.read(cx)
            .endpoint
            .clone()
            .update(cx, |s, cx| s.set_value("not a URL", w, cx));
        w.click("save", cx);
        assert!(view.read(cx).notice.is_some());
        w.click("back", cx);
        assert!(view.read(cx).page == Page::Translator);
        assert_eq!(
            view.read(cx).settings.endpoint,
            Settings::default().endpoint
        );
        w.click("clear", cx);
        assert!(view.read(cx).editor.read(cx).value().is_empty());
        assert!(view.read(cx).output.is_empty());
        assert!(w.find("settings").bounds().top() > w.find("translate").bounds().bottom());
    })
    .unwrap();
}

fn open_test_window(
    cx: &mut TestAppContext,
) -> (
    gpui_kit::AnyWindowHandle,
    gpui_kit::Entity<super::Translator>,
) {
    cx.update(|cx| {
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
            |window, cx| configured_view(window, cx, Ok(Default::default())),
        )
        .unwrap()
    })
}

#[gpui_kit::test]
fn restored_pair_can_be_copied_without_a_network_request(cx: &mut TestAppContext) {
    let (handle, view) = open_test_window(cx);
    cx.update_window(handle, |_, w, cx| {
        view.update(cx, |v, cx| {
            v.entries.push(super::Entry {
                original: "The train is here.".into(),
                translated: "列车到了。".into(),
                source: super::ENGLISH.into(),
                target: super::CHINESE.into(),
            });
            v.restore(0, w, cx);
        });
        w.render_frame(cx);
        assert!(view.read(cx).status == super::Status::Done);
        assert!(view.read(cx).cancel.is_none() && view.read(cx).pump.is_none());
        for (button, expected) in [
            ("copy-source", "The train is here."),
            ("copy-target", "列车到了。"),
        ] {
            w.click(button, cx);
            assert_eq!(
                cx.read_from_clipboard()
                    .and_then(|item| item.text())
                    .as_deref(),
                Some(expected)
            );
        }
    })
    .unwrap();
}
