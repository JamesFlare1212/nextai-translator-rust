mod models;
use self::models::{ModelLookup, ModelPicker};
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
const PROTOCOLS: [&str; 3] = [
    "Chat Completions（SSE）",
    "Responses（SSE）",
    "Responses（WebSocket）",
];

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
        ApiMode::ResponsesWs => PROTOCOLS[2],
    }
}

pub(super) fn apply(mode: Appearance, window: &mut Window, cx: &mut App) {
    let theme = match mode {
        Appearance::System => ThemeMode::from(window.appearance()),
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
    };
    Theme::change(theme, Some(window), cx);
    Theme::update(cx, |theme| {
        theme.focus_ring = false;
        theme.ring = theme.input.blend(theme.foreground.alpha(0.16));
    });
}

pub(super) struct OptionsForm {
    provider: Menu,
    pub(super) drafts: config::Profiles,
    protocol: Menu,
    reasoning: bool,
    model: ModelPicker,
    models: Option<ModelLookup>,
    theme: Menu,
    target: Picker,
    source: Picker,
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
        let model = ModelPicker::new(&settings.model, window, cx);
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
                this.switch_provider(name, w, cx);
            },
        )
        .detach();
        cx.observe_in(&protocol, window, |this, _, w, cx| {
            this.discard_stale_models(w, cx);
            cx.notify();
        })
        .detach();
        Self {
            drafts: Default::default(),
            provider,
            protocol,
            reasoning: settings.reasoning,
            model,
            models: None,
            theme,
            target,
            source: picker(false, window, cx),
            fonts,
        }
    }

    pub(super) fn model_name(&self, cx: &App) -> String {
        self.model.value(cx)
    }

    pub(super) fn cancel_models(&mut self) {
        self.models = None;
    }

    pub(super) fn api_mode(&self, cx: &App) -> ApiMode {
        if self.protocol.read(cx).selected_value() == Some(&PROTOCOLS[2]) {
            ApiMode::ResponsesWs
        } else if self.protocol.read(cx).selected_value() == Some(&PROTOCOLS[1]) {
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
        profiles: &config::Profiles,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.drafts = profiles.clone();
        let active = PROVIDERS
            .iter()
            .map(|p| p.name)
            .chain([CUSTOM])
            .find(|name| *name == profiles.active)
            .unwrap_or_else(|| provider::identify(&settings.endpoint));
        self.drafts.active = active.into();
        self.cancel_models();
        self.model.reset(&settings.model, window, cx);
        self.reasoning = settings.reasoning;
        for (select, label) in [
            (&self.provider, active),
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
        select(
            &self.source,
            language(&prefs.source).map_or(ENGLISH, |l| l.0),
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
            source: self
                .source
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or(ENGLISH)
                .into(),
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
    fn switch_provider(&mut self, name: &str, w: &mut Window, cx: &mut Context<Self>) {
        if self.form.drafts.active == name {
            return;
        }
        let current = self.draft(cx);
        let drafts = &mut self.form.drafts;
        drafts.others.insert(
            std::mem::replace(&mut drafts.active, name.to_string()),
            current,
        );
        let next = drafts.others.remove(name).unwrap_or_else(|| {
            let preset = PROVIDERS.iter().find(|p| p.name == name);
            Settings {
                endpoint: preset.map_or("", |p| p.endpoint).into(),
                model: preset.map_or("", |p| p.model).into(),
                ..Default::default()
            }
        });
        self.form.cancel_models();
        self.endpoint
            .update(cx, |s, cx| s.set_value(next.endpoint, w, cx));
        self.api_key
            .update(cx, |s, cx| s.set_value(next.api_key, w, cx));
        self.form.model.reset(&next.model, w, cx);
        self.form.protocol.update(cx, |s, cx| {
            s.set_selected_value(&protocol_name(next.api_mode), w, cx)
        });
        self.form.reasoning = next.reasoning;
        self.notice = None;
        cx.notify();
    }

    fn endpoint_input(&self, cx: &App) -> impl IntoElement {
        let endpoint = self.endpoint.clone();
        let mode = self.form.api_mode(cx);
        div()
            .size_full()
            .relative()
            .child(
                Input::new(&endpoint)
                    .focus_bordered(false)
                    .aria_label("API 地址"),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, cx| {
                        let state = endpoint.read(cx);
                        let value = state.value();
                        let Some(hint) = engine::endpoint_hint(&value, mode) else {
                            return;
                        };
                        let end = value.len();
                        let Some(caret) = state.range_to_bounds(&(end..end)) else {
                            return;
                        };
                        let mask = ContentMask {
                            bounds: state.input_bounds(),
                        };
                        let mut style = window.text_style();
                        style.color = cx.theme().muted_foreground;
                        let line = window.text_system().shape_line(
                            hint.clone().into(),
                            style.font_size.to_pixels(window.rem_size()),
                            &[style.to_run(hint.len())],
                            None,
                        );
                        window.with_content_mask(Some(mask), |w| {
                            let _ = line.paint(
                                caret.origin,
                                caret.size.height,
                                TextAlign::Left,
                                None,
                                w,
                                cx,
                            );
                        });
                    },
                )
                .absolute()
                .inset_0()
                .text_sm(),
            )
    }

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
                            .child(field("API 地址", self.endpoint_input(cx)))
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
                                        "默认来源语言",
                                        Select::new(&self.form.source)
                                            .w_full()
                                            .focus_ring(false)
                                            .menu_max_h(px(230.))
                                            .search_placeholder("搜索语言")
                                            .accessibility_label("默认来源语言"),
                                    ))
                                    .child(field(
                                        "默认目标语言",
                                        Select::new(&self.form.target)
                                            .w_full()
                                            .focus_ring(false)
                                            .menu_max_h(px(230.))
                                            .search_placeholder("搜索语言")
                                            .accessibility_label("默认目标语言"),
                                    )),
                            )
                            .child(menu_field("外观", &self.form.theme))
                            .child(self.form.fonts.render()),
                    ),
            )
    }
}
