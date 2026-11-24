#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("NextAI Translator is a Windows desktop application.");

mod config;
mod engine;
mod history;
#[cfg(test)]
mod live_tests;
mod provider;
mod ui;
mod updater;

use gpui_kit::{base::Root, *};

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let bounds = Bounds::centered(None, size(px(620.), px(700.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(540.), px(600.))),
                    window_background: WindowBackgroundAppearance::Blurred,
                    titlebar: Some(TitlebarOptions {
                        title: Some("NextAI Translator".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    let view = ui::view(window, cx);
                    cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
                },
            )
            .expect("Could not open the translation window");
            cx.activate(true);
        });
}
