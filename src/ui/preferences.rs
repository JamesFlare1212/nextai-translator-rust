use super::*;
use crate::{
    config::{Appearance, Preferences},
    provider::{self, CUSTOM, PROVIDERS},
};
use gpui_kit::component::{Theme, ThemeMode};

type Menu = Entity<SelectState<Vec<&'static str>>>;
const MODES: [&str; 3] = ["跟随系统", "浅色", "深色"];

fn menu(items: Vec<&'static str>, value: &str, window: &mut Window, cx: &mut App) -> Menu {
    let index = items.iter().position(|&it| it == value).unwrap_or_default();
    cx.new(|cx| SelectState::new(items, Some(IndexPath::default().row(index)), window, cx))
}

fn mode_name(mode: Appearance) -> &'static str {
    MODES[match mode {
        Appearance::System => 0,
        Appearance::Light => 1,
        Appearance::Dark => 2,
    }]
}

pub(super) fn apply(mode: Appearance, window: &mut Window, cx: &mut App) {
    let theme = match mode {
        Appearance::System => ThemeMode::from(window.appearance()),
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
    };
    Theme::change(theme, Some(window), cx);
}

pub(super) struct OptionsForm {
    provider: Menu,
    theme: Menu,
    target: Picker,
}

impl OptionsForm {
    pub(super) fn new(
        settings: &Settings,
        prefs: &Preferences,
        window: &mut Window,
        cx: &mut Context<Translator>,
    ) -> Self {
        let provider = menu(
            PROVIDERS.iter().map(|p| p.name).chain([CUSTOM]).collect(),
            provider::identify(&settings.endpoint),
            window,
            cx,
        );
        let theme = menu(MODES.to_vec(), mode_name(prefs.appearance), window, cx);
        let target = picker(false, window, cx);
        cx.subscribe_in(
            &provider,
            window,
            |this, _, event: &SelectEvent<Vec<&'static str>>, w, cx| {
                let SelectEvent::Confirm(Some(name)) = event else {
                    return;
                };
                let preset = PROVIDERS.iter().find(|p| p.name == *name);
                let url = preset.map_or("", |p| p.endpoint);
                if url != this.endpoint.read(cx).value().as_ref() {
                    this.api_key.update(cx, |s, cx| s.set_value("", w, cx));
                    this.endpoint.update(cx, |s, cx| s.set_value(url, w, cx));
                    this.model.update(cx, |s, cx| {
                        s.set_value(preset.map_or("", |p| p.model), w, cx)
                    });
                }
                cx.notify();
            },
        )
        .detach();
        Self {
            provider,
            theme,
            target,
        }
    }

    pub(super) fn reset(
        &self,
        settings: &Settings,
        prefs: &Preferences,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (select, label) in [
            (&self.provider, provider::identify(&settings.endpoint)),
            (&self.theme, mode_name(prefs.appearance)),
        ] {
            select.update(cx, |s, cx| s.set_selected_value(&label, window, cx));
        }
        select(
            &self.target,
            language(&prefs.target).map_or(CHINESE, |l| l.0),
            window,
            cx,
        );
    }

    pub(super) fn preferences(&self, cx: &App) -> Preferences {
        let appearance = match self.theme.read(cx).selected_value().copied() {
            Some("深色") => Appearance::Dark,
            Some("浅色") => Appearance::Light,
            _ => Appearance::System,
        };
        Preferences {
            appearance,
            target: self
                .target
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or(CHINESE)
                .into(),
        }
    }
}

fn field(name: &'static str, control: impl IntoElement) -> Div {
    v_flex()
        .w_full()
        .gap(px(7.))
        .child(Label::new(name).text_sm().font_weight(FontWeight::SEMIBOLD))
        .child(div().w_full().h(px(32.)).child(control))
}

impl Translator {
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_h_0()
            .child(page_header("NextAI Translator · 设置", cx))
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .p(px(24.))
                            .gap(px(17.))
                            .child(field(
                                "默认服务提供商",
                                Select::new(&self.form.provider)
                                    .w_full()
                                    .accessibility_label("默认服务提供商"),
                            ))
                            .child(field(
                                "API 地址（Chat Completions）",
                                Input::new(&self.endpoint).aria_label("API 地址"),
                            ))
                            .child(field(
                                "API 密钥",
                                Input::new(&self.api_key)
                                    .aria_label("API 密钥")
                                    .mask_toggle(),
                            ))
                            .child(field("模型", Input::new(&self.model).aria_label("模型")))
                            .child(field(
                                "默认目标语言",
                                Select::new(&self.form.target)
                                    .w_full()
                                    .menu_max_h(px(230.))
                                    .search_placeholder("搜索语言")
                                    .accessibility_label("默认目标语言"),
                            ))
                            .child(field(
                                "外观",
                                Select::new(&self.form.theme)
                                    .w_full()
                                    .accessibility_label("外观"),
                            )),
                    ),
            )
    }
}
