mod models;
use self::models::ModelLookup;
pub(super) use self::models::{observe_model_inputs, validate_draft_origin};
use super::*;
use crate::{
    config::{Appearance, Preferences},
    engine::ApiMode,
    provider::{self, CUSTOM, PROVIDERS},
};
use gpui_kit::component::{FocusableExt, Theme, ThemeMode, switch::Switch};

type Menu = Entity<SelectState<Vec<&'static str>>>;
const MODES: [&str; 3] = ["跟随系统", "浅色", "深色"];
const PROTOCOLS: [&str; 2] = ["Chat Completions", "Responses"];

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

fn protocol_name(mode: ApiMode) -> &'static str {
    match mode {
        ApiMode::ChatCompletions => PROTOCOLS[0],
        ApiMode::Responses => PROTOCOLS[1],
    }
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
    protocol: Menu,
    reasoning: bool,
    models: Option<ModelLookup>,
    theme: Menu,
    target: Picker,
    fonts: super::typography::FontControls,
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
        let protocol = menu(
            PROTOCOLS.to_vec(),
            protocol_name(settings.api_mode),
            window,
            cx,
        );
        let theme = menu(MODES.to_vec(), mode_name(prefs.appearance), window, cx);
        let target = picker(false, window, cx);
        let fonts = super::typography::FontControls::new(prefs, window, cx);
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
                    this.form.cancel_models();
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
        cx.observe_in(&protocol, window, |this, _, _, cx| {
            this.discard_stale_models(cx)
        })
        .detach();
        Self {
            provider,
            protocol,
            reasoning: settings.reasoning,
            models: None,
            theme,
            target,
            fonts,
        }
    }

    pub(super) fn cancel_models(&mut self) {
        self.models = None;
    }

    pub(super) fn api_mode(&self, cx: &App) -> ApiMode {
        if self.protocol.read(cx).selected_value() == Some(&PROTOCOLS[1]) {
            ApiMode::Responses
        } else {
            ApiMode::ChatCompletions
        }
    }

    pub(super) fn reasoning(&self) -> bool {
        self.reasoning
    }

    pub(super) fn reset(
        &mut self,
        settings: &Settings,
        prefs: &Preferences,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.cancel_models();
        self.reasoning = settings.reasoning;
        for (select, label) in [
            (&self.provider, provider::identify(&settings.endpoint)),
            (&self.protocol, protocol_name(settings.api_mode)),
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
        self.fonts.reset(prefs, window, cx);
    }

    pub(super) fn preferences(&self, cx: &App) -> Preferences {
        let appearance = match self.theme.read(cx).selected_value().copied() {
            Some("深色") => Appearance::Dark,
            Some("浅色") => Appearance::Light,
            _ => Appearance::System,
        };
        let mut prefs = Preferences {
            appearance,
            target: self
                .target
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or(CHINESE)
                .into(),
            ..Default::default()
        };
        self.fonts.write(&mut prefs, cx);
        prefs
    }
}

fn field(name: &'static str, control: impl IntoElement) -> Div {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap(px(6.))
        .child(Label::new(name).text_sm().font_weight(FontWeight::SEMIBOLD))
        .child(div().w_full().h(px(32.)).child(control))
}

fn menu_field(name: &'static str, state: &Menu) -> Div {
    field(
        name,
        Select::new(state)
            .w_full()
            .focus_ring(false)
            .accessibility_label(name),
    )
}

impl Translator {
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let row = || h_flex().w_full().items_start().gap(px(14.));
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
                            .gap(px(16.))
                            .child(
                                row()
                                    .child(menu_field("默认服务提供商", &self.form.provider))
                                    .child(menu_field("API 协议", &self.form.protocol)),
                            )
                            .child(field(
                                "API 地址",
                                Input::new(&self.endpoint)
                                    .focus_bordered(false)
                                    .aria_label("API 地址"),
                            ))
                            .child(field(
                                "API 密钥",
                                Input::new(&self.api_key)
                                    .focus_bordered(false)
                                    .aria_label("API 密钥")
                                    .mask_toggle(),
                            ))
                            .child(self.render_models(cx))
                            .child(
                                Switch::new("reasoning")
                                    .label("思考模型")
                                    .checked(self.form.reasoning)
                                    .on_change(cx.listener(|this, enabled, _, cx| {
                                        this.form.reasoning = *enabled;
                                        cx.notify();
                                    })),
                            )
                            .child(div().w_full().h(px(1.)).bg(cx.theme().border.opacity(0.65)))
                            .child(
                                row()
                                    .child(field(
                                        "默认目标语言",
                                        Select::new(&self.form.target)
                                            .w_full()
                                            .focus_ring(false)
                                            .menu_max_h(px(230.))
                                            .search_placeholder("搜索语言")
                                            .accessibility_label("默认目标语言"),
                                    ))
                                    .child(menu_field("外观", &self.form.theme)),
                            )
                            .child(self.form.fonts.render()),
                    ),
            )
    }
}
