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

#[gpui_kit::test]
fn source_viewport_fills_the_frame_and_keeps_the_last_line_visible(cx: &mut TestAppContext) {
    let (handle, view) = open_test_window(cx);
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        let text = "wrapped source line\n".repeat(40) + "END";
        w.input(&text, cx);
        w.render_frame(cx);
        let editor = view.read(cx).editor.read(cx);
        let last = editor
            .range_to_bounds(&(text.len() - 3..text.len()))
            .unwrap();
        let frame = w.find(("input", view.read(cx).editor.entity_id())).bounds();
        let viewport = w.find("input-state").bounds();
        assert_eq!(viewport.top(), frame.top() + px(1.));
        assert_eq!(viewport.bottom(), frame.bottom() - px(1.));
        assert!(last.top() >= viewport.top());
        assert!(last.bottom() <= viewport.bottom());
    })
    .unwrap();
}

#[gpui_kit::test]
fn update_status_stays_visible_at_minimum_size(cx: &mut TestAppContext) {
    use super::updates::State;
    let (handle, view) = open_test_window(cx);
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        w.click("settings", cx);
        for state in [
            State::Checking,
            State::Downloading,
            State::Failed("无法连接 GitHub，请检查网络".into()),
            State::Current,
        ] {
            view.update(cx, |v, cx| {
                v.update = state;
                cx.notify();
            });
            w.render_frame(cx);
            let action = w.find("software-update");
            assert!(action.visible());
            assert!(action.bounds().right() <= px(540.));
        }
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
fn theme_switch_keeps_the_focus_cue_on_the_existing_border(cx: &mut TestAppContext) {
    use crate::config::Appearance;
    use gpui_kit::component::ActiveTheme;

    let (handle, _) = open_test_window(cx);
    cx.update_window(handle, |_, window, cx| {
        for mode in [Appearance::Light, Appearance::Dark, Appearance::System] {
            super::preferences::apply(mode, window, cx);
            let theme = cx.theme();
            let delta = theme.ring.l - theme.input.l;
            assert!(!theme.focus_ring);
            assert!(delta.abs() > 0.0 && delta.abs() < 0.2);
            assert_eq!(delta.is_sign_positive(), theme.is_dark());
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn language_menu_surface_follows_the_inset_of_the_selected_row(cx: &mut TestAppContext) {
    use gpui_kit::component::ActiveTheme;
    let (handle, view) = open_test_window(cx);
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        let id = view.read(cx).source.entity_id();
        w.within(("select", id)).click("input", cx);
    })
    .unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        let scale = w.scale_factor();
        let quads = w.painted_quads();
        let surface = quads
            .iter()
            .find(|q| q.bounds.size.width.0 == 196. * scale && q.corner_radii.bottom_left.0 > 0.)
            .expect("language popup surface");
        let row = quads
            .iter()
            .find(|q| {
                q.bounds.size.width.0 == 188. * scale
                    && q.corner_radii.bottom_left == cx.theme().radius.scale(scale)
            })
            .expect("highlighted language row");
        let inset = row.bounds.left().0 - surface.bounds.left().0;
        assert_eq!(inset, 4. * scale);
        assert_eq!(
            surface.corner_radii.bottom_left.0,
            row.corner_radii.bottom_left.0 + inset
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn dictionary_markup_is_rendered_but_full_copy_keeps_the_source(cx: &mut TestAppContext) {
    let (handle, view) = open_test_window(cx);
    let entry =
        "**token**  \n**语言：**英语  \n**发音**：英 /ˈtəʊkən/；美 /ˈtoʊkən/\n\n- **释义**：标志";
    cx.update_window(handle, |_, w, cx| {
        view.update(cx, |v, cx| {
            v.entries.push(super::Entry {
                original: "token".into(),
                translated: entry.into(),
                source: super::ENGLISH.into(),
                target: super::CHINESE.into(),
            });
            v.restore(0, w, cx);
        });
        w.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        let runs = gpui_kit::base::test_support::texts(w);
        assert!(runs.iter().any(|r| r.text().contains("语言：英语")));
        assert!(runs.iter().any(|r| r.text().contains("发音：英 /ˈtəʊkən/")));
        assert!(runs.iter().any(|r| r.text().contains("释义：标志")));
        assert!(!runs.iter().any(|r| r.text().contains("**")));
        w.click("copy-target", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(entry)
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn streaming_wakes_on_first_delta_and_batches_retry_replacement(cx: &mut TestAppContext) {
    use super::{Message, STREAM_CAPACITY, STREAM_FRAME, Status};

    let (handle, view) = open_test_window(cx);
    let (tx, rx) = async_channel::bounded(STREAM_CAPACITY);
    let (stop, _) = tokio::sync::watch::channel(false);
    cx.update(|cx| {
        view.update(cx, |v, cx| {
            v.status = Status::Running;
            v.listen(stop, rx, cx);
        });
    });
    cx.run_until_parked();
    tx.try_send(Message::Delta("first".into())).unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).output, "first"));

    tx.try_send(Message::Retrying).unwrap();
    cx.run_until_parked();
    for update in [
        Message::Replace("new".into()),
        Message::Delta(" answer".into()),
        Message::Finished(Ok(())),
    ] {
        tx.try_send(update).unwrap();
    }
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).output, "first"));
    cx.executor().advance_clock(STREAM_FRAME);
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        let v = view.read(cx);
        assert_eq!(v.output, "new answer");
        assert!(v.status == Status::Done && v.translation.is_none());
        w.render_frame(cx);
        assert!(w.find("retry").visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn retrying_translation_can_be_stopped_or_replaced(cx: &mut TestAppContext) {
    use super::{Message, Status, updates::State};

    for stop in [false, true] {
        let (handle, view) = open_test_window(cx);
        cx.update_window(handle, |_, w, cx| {
            let send = |message, cx: &mut gpui_kit::App| {
                view.update(cx, |v, cx| {
                    v.apply_batch(std::iter::once(message));
                    cx.notify();
                });
            };
            view.update(cx, |v, _| {
                v.status = Status::Running;
                v.update = State::Ready(crate::updater::test_update(), tempfile::tempdir().unwrap());
            });
            send(Message::Delta("first".into()), cx);
            send(Message::Retrying, cx);
            w.render_frame(cx);
            assert_eq!(w.find("translate").label(), Some("停止"));
            assert!(w.try_find("retry").is_none());
            w.click("copy-target", cx);
            assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("first"));
            w.click("settings", cx);
            assert_eq!(w.find("software-update").disabled(), Some(true));
            w.click("back", cx);
            if stop {
                w.click("translate", cx);
                assert!(view.read(cx).status == Status::Stopped);
            } else {
                send(Message::Replace("new answer".into()), cx);
                assert!(view.read(cx).status == Status::Running);
                w.click("copy-target", cx);
                assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("new answer"));
                send(Message::Finished(Err("stream interrupted".into())), cx);
                w.render_frame(cx);
                assert!(matches!(&view.read(cx).status, Status::Error(error) if error == "stream interrupted"));
            }
            assert!(w.find("retry").visible());
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn cancelling_full_inbox_closes_sender_and_discards_stale_completion(cx: &mut TestAppContext) {
    use super::{Message, Status};

    let (_, view) = open_test_window(cx);
    let (tx, rx) = async_channel::bounded(2);
    let (stop, cancelled) = tokio::sync::watch::channel(false);
    let (new_tx, new_rx) = async_channel::bounded(2);
    cx.update(|cx| {
        view.update(cx, |v, cx| {
            v.status = Status::Running;
            v.listen(stop, rx, cx);
            tx.try_send(Message::Delta("stale".into())).unwrap();
            tx.try_send(Message::Finished(Ok(()))).unwrap();
            assert!(tx.is_full());
            v.stop(cx);
            assert!(*cancelled.borrow());
            assert!(tx.try_send(Message::Retrying).unwrap_err().is_closed());
            assert!(v.status == Status::Stopped);
            v.status = Status::Running;
            v.listen(tokio::sync::watch::channel(false).0, new_rx, cx);
        });
    });
    new_tx.try_send(Message::Delta("current".into())).unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(view.read(cx).output, "current");
        assert!(view.read(cx).status == Status::Running);
    });
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
        assert!(view.read(cx).translation.is_none());
        for (button, expected) in [
            ("copy-source", "The train is here."),
            ("copy-target", "列车到了。"),
            ("copy-target", "列车到了。"),
        ] {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("其他内容".into()));
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
