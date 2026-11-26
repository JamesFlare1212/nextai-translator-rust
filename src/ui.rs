mod dictionary;
mod language;
mod preferences;
mod typography;
mod updates;
use language::{AUTO, CHINESE, ENGLISH, LANGUAGES, Language, automatic_target, detect, language};

use crate::{
    config,
    engine::{self, Settings},
    history::{self, Entry},
};
use gpui_kit::{
    assets::IconName,
    base::{SelectableText, TextView},
    component::{
        ActiveTheme, Disableable, IndexPath, Sizable, TitleBar,
        button::{Button, ButtonVariants},
        clipboard::Clipboard,
        h_flex,
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
        label::Label,
        resizable::{resizable_panel, v_resizable},
        select::{SearchableVec, Select, SelectEvent, SelectState},
        spinner::Spinner,
        v_flex,
    },
    prelude::*,
    *,
};
use std::{cell::OnceCell, sync::Arc, time::Duration};
use tokio::sync::watch;

gpui_kit::actions!(translator, [OpenSettings, GoBack]);

type Choices = SearchableVec<Language>;
type Picker = Entity<SelectState<Choices>>;

fn picker(source: bool, window: &mut Window, cx: &mut App) -> Picker {
    let items: Vec<_> = source
        .then_some(AUTO)
        .into_iter()
        .chain(
            LANGUAGES
                .iter()
                .copied()
                .filter(|l| !source || l.2.is_none()),
        )
        .collect();
    let ix = if source {
        0
    } else {
        items.iter().position(|l| l.0 == CHINESE).unwrap()
    };
    cx.new(|cx| {
        SelectState::new(
            Choices::new(items),
            Some(IndexPath::default().row(ix)),
            window,
            cx,
        )
        .searchable(true)
    })
}

fn select(picker: &Picker, value: &'static str, window: &mut Window, cx: &mut App) {
    picker.update(cx, |state, cx| state.set_selected_value(&value, window, cx));
}

fn icon_button(id: &'static str, icon: IconName, label: &'static str) -> Button {
    Button::new(id)
        .icon(icon)
        .small()
        .ghost()
        .tooltip(label)
        .accessibility_label(label)
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Page {
    Translator,
    Settings,
    History,
}
#[derive(PartialEq, Eq)]
enum Status {
    Idle,
    Running,
    Done,
    Stopped,
    Error(String),
}

#[derive(Clone)]
struct Request {
    source: &'static str,
    target: &'static str,
    detected: Option<&'static str>,
    text: SharedString,
}

enum Message {
    Retrying,
    Replace(String),
    Delta(String),
    Finished(Result<(), String>),
}

const STREAM_CAPACITY: usize = 128;
const STREAM_FRAME: Duration = Duration::from_millis(24);

struct TranslationTask {
    stop: watch::Sender<bool>,
    inbox: async_channel::Receiver<Message>,
    _pump: Task<()>,
}

impl Drop for TranslationTask {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        self.inbox.close();
    }
}

pub struct Translator {
    app_icon: Arc<Image>,
    focus: FocusHandle,
    page: Page,
    editor: Entity<TextareaState>,
    source: Picker,
    target: Picker,
    manual_target: bool,
    auto_source: bool,
    preferences: config::Preferences,
    profiles: config::Profiles,
    form: preferences::OptionsForm,
    status: Status,
    request: Option<Request>,
    output: String,
    dictionary_html: OnceCell<SharedString>,
    translation: Option<TranslationTask>,
    retrying: bool,
    settings: Settings,
    endpoint: Entity<InputState>,
    api_key: Entity<InputState>,
    notice: Option<String>,
    entries: Vec<Entry>,
    confirming_clear: bool,
    update: updates::State,
    package: crate::updater::Kind,
}

pub fn view(window: &mut Window, cx: &mut App) -> Entity<Translator> {
    let view = configured_view(window, cx, config::load());
    view.update(cx, |this, cx| this.check_update(cx));
    view
}

fn configured_view(
    window: &mut Window,
    cx: &mut App,
    loaded: Result<(Settings, config::Preferences, config::Profiles), String>,
) -> Entity<Translator> {
    gpui_kit::component::set_locale("zh-CN");
    cx.bind_keys([
        KeyBinding::new("ctrl-,", OpenSettings, Some("Translator")),
        KeyBinding::new("escape", GoBack, Some("Translator")),
    ]);
    cx.new(|cx| Translator::new(window, cx, loaded))
}

impl Translator {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        loaded: Result<(Settings, config::Preferences, config::Profiles), String>,
    ) -> Self {
        let ((settings, preferences, profiles), notice) = match loaded {
            Ok(s) => (s, None),
            Err(e) => (Default::default(), Some(e)),
        };
        let page = if notice.is_some() {
            Page::Settings
        } else {
            Page::Translator
        };
        let editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(1)
                .submit_on_enter(true)
                .placeholder("输入要翻译的文本…")
        });
        let source = picker(true, window, cx);
        let target = picker(false, window, cx);
        select(
            &target,
            language(&preferences.target).map_or(CHINESE, |l| l.0),
            window,
            cx,
        );
        let mut form = preferences::OptionsForm::new(&settings, &preferences, window, cx);
        form.reset(&settings, &preferences, &profiles, window, cx);
        preferences::apply(preferences.appearance, window, cx);
        typography::apply(&preferences, cx);
        cx.observe_window_appearance(window, |this, w, cx| {
            if this.preferences.appearance == config::Appearance::System {
                preferences::apply(config::Appearance::System, w, cx);
                typography::apply(&this.preferences, cx);
            }
        })
        .detach();
        cx.subscribe_in(
            &editor,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { shift: false, .. } if this.page == Page::Translator => {
                    this.submit(window, cx)
                }
                InputEvent::Change => {
                    if this.editor.read(cx).text().len() == 0 {
                        this.reset();
                    }
                    cx.notify();
                }
                _ => {}
            },
        )
        .detach();
        for (is_source, picker) in [(true, &source), (false, &target)] {
            cx.subscribe_in(
                picker,
                window,
                move |this, _, event: &SelectEvent<Choices>, window, cx| {
                    if let SelectEvent::Confirm(Some(chosen)) = event {
                        if is_source {
                            this.auto_source = *chosen == AUTO.0;
                        } else {
                            this.manual_target = true;
                        }
                        this.submit(window, cx);
                        cx.notify();
                    }
                },
            )
            .detach();
        }
        let endpoint =
            cx.new(|cx| InputState::new(window, cx).default_value(settings.endpoint.clone()));
        let api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .default_value(settings.api_key.clone())
        });
        preferences::observe_model_inputs(&endpoint, &api_key, window, cx);
        if page == Page::Translator {
            editor.focus_handle(cx).focus(window, cx);
        } else {
            endpoint.focus_handle(cx).focus(window, cx);
        }
        Self {
            app_icon: Arc::new(Image::from_bytes(
                ImageFormat::Ico,
                include_bytes!("../assets/nextai.ico").to_vec(),
            )),
            focus: cx.focus_handle(),
            page,
            editor,
            source,
            target,
            manual_target: false,
            auto_source: true,
            form,
            preferences,
            profiles,
            status: Status::Idle,
            request: None,
            output: String::new(),
            dictionary_html: OnceCell::new(),
            translation: None,
            retrying: false,
            endpoint,
            api_key,
            settings,
            notice,
            entries: Vec::new(),
            confirming_clear: false,
            update: updates::State::Idle,
            package: crate::updater::Kind::current(),
        }
    }

    fn reset(&mut self) {
        self.translation.take();
        self.status = Status::Idle;
        self.retrying = false;
        self.output.clear();
        self.dictionary_html.take();
        self.request = None;
        self.notice = None;
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if self.status == Status::Running {
            self.translation.take();
            self.status = Status::Stopped;
            cx.notify();
        }
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reset();
        self.editor.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).value();
        if text.trim().is_empty() {
            return;
        }
        let source = self
            .source
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or(AUTO.0);
        let detected = if self.auto_source {
            let detected = detect(&text);
            select(&self.source, detected.unwrap_or(AUTO.0), window, cx);
            detected
        } else {
            Some(source)
        };
        let target = if self.manual_target {
            self.target
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or(CHINESE)
        } else {
            let target = automatic_target(
                detected,
                language(&self.preferences.target).map_or(CHINESE, |l| l.0),
                language(&self.preferences.source).map_or(ENGLISH, |l| l.0),
            );
            select(&self.target, target, window, cx);
            target
        };
        self.start(
            Request {
                source: detected.unwrap_or(AUTO.0),
                target,
                detected,
                text,
            },
            cx,
        );
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        if let Some(request) = self.request.clone() {
            self.start(request, cx);
        }
    }

    fn start(&mut self, request: Request, cx: &mut Context<Self>) {
        self.reset();
        self.request = Some(request.clone());
        self.status = Status::Running;
        let (stop, cancel) = watch::channel(false);
        let settings = self.settings.clone();
        let (tx, rx) = async_channel::bounded(STREAM_CAPACITY);
        let worker = std::thread::Builder::new()
            .name("translation".into())
            .spawn(move || {
                let result = engine::translate_with_recovery(
                    &settings,
                    request.source,
                    request.target,
                    &request.text,
                    &cancel,
                    |update| {
                        let _ = tx.send_blocking(match update {
                            engine::StreamUpdate::Append(s) => Message::Delta(s.to_owned()),
                            engine::StreamUpdate::Replace(s) => Message::Replace(s.to_owned()),
                            engine::StreamUpdate::Retrying => Message::Retrying,
                        });
                    },
                );
                let _ = tx.send_blocking(Message::Finished(result));
            });
        if let Err(e) = worker {
            self.status = Status::Error(e.to_string());
            cx.notify();
            return;
        }
        self.listen(stop, rx, cx);
        cx.notify();
    }

    fn listen(
        &mut self,
        stop: watch::Sender<bool>,
        inbox: async_channel::Receiver<Message>,
        cx: &mut Context<Self>,
    ) {
        let cancel = stop.subscribe();
        let rx = inbox.clone();
        let pump = cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            let mut next_frame = executor.now();
            loop {
                let first = rx
                    .recv()
                    .await
                    .unwrap_or_else(|_| Message::Finished(Err("翻译线程已退出。".into())));
                let delay = next_frame.saturating_duration_since(executor.now());
                if !delay.is_zero() && !matches!(first, Message::Finished(_)) {
                    executor.timer(delay).await;
                }
                let batch = std::iter::once(first)
                    .chain((1..STREAM_CAPACITY).map_while(|_| rx.try_recv().ok()));
                let keep_listening = this.update(cx, |this, cx| {
                    if *cancel.borrow() {
                        return false;
                    }
                    let more = this.apply_batch(batch);
                    cx.notify();
                    more
                });
                if !matches!(keep_listening, Ok(true)) {
                    break;
                }
                next_frame = executor.now() + STREAM_FRAME;
            }
        });
        self.translation = Some(TranslationTask {
            stop,
            inbox,
            _pump: pump,
        });
    }

    fn apply_batch(&mut self, batch: impl Iterator<Item = Message>) -> bool {
        for message in batch {
            match message {
                Message::Retrying => self.retrying = true,
                Message::Replace(text) => {
                    self.retrying = false;
                    self.output = text;
                    self.dictionary_html.take();
                }
                Message::Delta(text) => {
                    self.output.push_str(&text);
                    self.dictionary_html.take();
                }
                Message::Finished(result) => {
                    self.finish(result);
                    return false;
                }
            }
        }
        true
    }

    fn finish(&mut self, result: Result<(), String>) {
        self.translation.take();
        self.status = match result {
            Err(error) => Status::Error(error),
            Ok(()) => {
                if let Some(r) = self.request.as_ref() {
                    let entry = Entry {
                        source: r.detected.unwrap_or(r.source).into(),
                        target: r.target.into(),
                        original: r.text.to_string(),
                        translated: self.output.clone(),
                    };
                    if let Err(e) = history::record(entry) {
                        self.notice = Some(e);
                    } else if self.page == Page::History {
                        self.load_history();
                    }
                }
                Status::Done
            }
        };
    }

    fn swap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).value();
        let old_source = self
            .source
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or(AUTO.0);
        let old_target = self
            .target
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or(CHINESE);
        let translated_current =
            self.status == Status::Done && self.request.as_ref().is_some_and(|r| r.text == text);
        let reverse = if old_source != AUTO.0 {
            Some(old_source)
        } else if text.trim().is_empty() {
            Some(automatic_target(
                Some(old_target),
                language(&self.preferences.target).map_or(CHINESE, |l| l.0),
                language(&self.preferences.source).map_or(ENGLISH, |l| l.0),
            ))
        } else if translated_current {
            self.request.as_ref().and_then(|r| r.detected)
        } else {
            detect(&text)
        };
        let Some(target) = reverse else {
            self.notice = Some("请先选择原文语言，再交换方向。".into());
            cx.notify();
            return;
        };
        let source = language(old_target).and_then(|l| l.2).unwrap_or(old_target);
        select(&self.source, source, window, cx);
        select(&self.target, target, window, cx);
        self.manual_target = true;
        self.auto_source = false;
        if translated_current {
            self.editor
                .update(cx, |s, cx| s.set_value(self.output.clone(), window, cx));
        }
        self.editor.focus_handle(cx).focus(window, cx);
        self.submit(window, cx);
        cx.notify();
    }

    fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Settings {
            return;
        }
        for (field, value) in [
            (&self.endpoint, &self.settings.endpoint),
            (&self.api_key, &self.settings.api_key),
        ] {
            field.update(cx, |s, cx| s.set_value(value.clone(), window, cx));
        }
        self.form.reset(
            &self.settings,
            &self.preferences,
            &self.profiles,
            window,
            cx,
        );
        self.page = Page::Settings;
        self.notice = None;
        self.endpoint.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn draft(&self, cx: &App) -> Settings {
        Settings {
            endpoint: self.endpoint.read(cx).value().trim().into(),
            model: self.form.model_name(cx),
            api_key: self.api_key.read(cx).value().trim().into(),
            api_mode: self.form.api_mode(cx),
            reasoning: self.form.reasoning(),
        }
    }

    fn check_origin(&self, draft: &Settings) -> Result<(), String> {
        let mut matching = std::iter::once(&self.settings)
            .chain(self.profiles.others.values())
            .filter(|s| {
                !draft.api_key.trim().is_empty() && s.api_key.trim() == draft.api_key.trim()
            });
        let Some(first) = matching.next() else {
            return Ok(());
        };
        if matching.any(|s| preferences::validate_draft_origin(s, draft).is_ok()) {
            return Ok(());
        }
        preferences::validate_draft_origin(first, draft)
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.draft(cx);
        let prefs = self.form.preferences(cx);
        if prefs.source == prefs.target {
            self.notice = Some("默认来源语言和目标语言不能相同。".into());
            cx.notify();
            return;
        }
        let profiles = &self.form.drafts;
        if let Err(e) = self.check_origin(&settings).and_then(|_| {
            profiles
                .others
                .values()
                .try_for_each(|s| self.check_origin(s))
        }) {
            self.notice = Some(e);
            cx.notify();
            return;
        }
        match config::save(&settings, &prefs, profiles) {
            Ok(()) => {
                self.profiles = profiles.clone();
                if prefs.target != self.preferences.target
                    || prefs.source != self.preferences.source
                {
                    self.manual_target = false;
                    select(
                        &self.target,
                        language(&prefs.target).map_or(CHINESE, |l| l.0),
                        window,
                        cx,
                    );
                }
                preferences::apply(prefs.appearance, window, cx);
                typography::apply(&prefs, cx);
                self.preferences = prefs;
                self.settings = settings;
                self.back(window, cx);
            }
            Err(error) => {
                self.notice = Some(error);
                cx.notify();
            }
        }
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form.cancel_models();
        self.page = Page::Translator;
        self.notice = None;
        self.confirming_clear = false;
        self.entries.clear();
        self.editor.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn load_history(&mut self) {
        match history::load() {
            Ok(entries) => self.entries = entries,
            Err(e) => {
                self.entries.clear();
                self.notice = Some(e);
            }
        }
    }

    fn show_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.notice = None;
        self.page = Page::History;
        self.load_history();
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn restore(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(ix).cloned() else {
            return;
        };
        self.reset();
        let source = language(&entry.source)
            .map(|l| l.2.unwrap_or(l.0))
            .unwrap_or(AUTO.0);
        let target = language(&entry.target).map(|l| l.0).unwrap_or(CHINESE);
        select(&self.source, source, window, cx);
        select(&self.target, target, window, cx);
        self.manual_target = true;
        self.auto_source = source == AUTO.0;
        let detected = if source == AUTO.0 {
            detect(&entry.original)
        } else {
            Some(source)
        };
        self.editor
            .update(cx, |s, cx| s.set_value(entry.original.clone(), window, cx));
        self.request = Some(Request {
            source,
            target,
            detected,
            text: entry.original.into(),
        });
        self.output = entry.translated;
        self.dictionary_html.take();
        self.status = Status::Done;
        self.back(window, cx);
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.status == Status::Running;
        let available = !self.editor.read(cx).value().trim().is_empty();
        let choose = |state: &Picker, name: &'static str| {
            div().w(px(126.)).h(px(24.)).flex_shrink_0().child(
                Select::new(state)
                    .small()
                    .w_full()
                    .menu_width(px(196.))
                    .accessibility_label(name)
                    .search_placeholder("搜索语言")
                    .menu_max_h(px(300.)),
            )
        };
        h_flex()
            .w_full()
            .h(px(62.))
            .flex_shrink_0()
            .px(px(16.))
            .gap(px(9.))
            .items_center()
            .bg(cx.theme().background.opacity(0.56))
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.65))
            .child(choose(&self.source, "原文语言"))
            .child(
                icon_button("swap", IconName::ArrowLeftRight, "交换语言")
                    .disabled(running)
                    .on_click(cx.listener(|this, _, w, cx| this.swap(w, cx))),
            )
            .child(choose(&self.target, "译文语言"))
            .child(div().flex_1())
            .child(
                Button::new("translate")
                    .primary()
                    .small()
                    .icon(if running {
                        IconName::Square
                    } else {
                        IconName::Languages
                    })
                    .label(if running { "停止" } else { "翻译" })
                    .tooltip(if running {
                        "停止翻译 (Esc)"
                    } else {
                        "翻译 (Enter)"
                    })
                    .disabled(!available && !running)
                    .on_click(cx.listener(|this, _, w, cx| {
                        if this.status == Status::Running {
                            this.stop(cx);
                        } else {
                            this.submit(w, cx);
                        }
                    })),
            )
    }

    fn render_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self.editor.read(cx).value();
        let has_text = !text.is_empty();
        let draft = has_text && self.request.as_ref().is_none_or(|r| r.text != text);
        let muted = cx.theme().muted_foreground;
        v_flex()
            .size_full()
            .min_h_0()
            .px(px(18.))
            .pt(px(34.))
            .pb(px(18.))
            .gap(px(4.))
            .child(
                div().flex_1().min_h_0().overflow_hidden().child(
                    Textarea::new(&self.editor)
                        .aria_label("原文")
                        .bordered(false)
                        .border_1()
                        .border_color(cx.theme().input)
                        .h(relative(1.))
                        .w_full()
                        .rounded(px(8.))
                        .text_size(px(self.preferences.font_size.into()))
                        .bg(cx.theme().secondary.opacity(0.65)),
                ),
            )
            .child(
                h_flex()
                    .h(px(27.))
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(3.))
                    .when(draft, |row| {
                        row.child(
                            Label::new("Enter 翻译 · Shift+Enter 换行")
                                .text_size(px(11.))
                                .text_color(muted),
                        )
                    })
                    .child(div().flex_1())
                    .when(has_text, |row| {
                        row.child(
                            Clipboard::new("copy-source")
                                .small()
                                .value(text)
                                .tooltip("复制原文")
                                .accessibility_label("复制原文"),
                        )
                        .child(
                            icon_button("clear", IconName::Eraser, "清空原文和译文")
                                .on_click(cx.listener(|this, _, w, cx| this.clear(w, cx))),
                        )
                    }),
            )
    }

    fn render_result(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (label, icon) = match self.status {
            Status::Idle => ("", None),
            Status::Running if self.retrying => ("正在自动重试", None),
            Status::Running => ("正在翻译", None),
            Status::Done => ("已翻译", Some(IconName::Check)),
            Status::Stopped => ("已停止", Some(IconName::Square)),
            Status::Error(_) => ("翻译失败", Some(IconName::CircleAlert)),
        };
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border.opacity(0.6);
        let running = self.status == Status::Running;
        let is_error = matches!(self.status, Status::Error(_));
        let mut result = v_flex().size_full().min_h_0();
        if self.status == Status::Idle {
            return result;
        }
        result = result.child(
            h_flex()
                .w_full()
                .items_center()
                .flex_shrink_0()
                .mt(px(-11.))
                .child(div().flex_1().h(px(1.)).bg(border))
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(5.))
                        .px(px(14.))
                        .h(px(23.))
                        .rounded_full()
                        .bg(cx.theme().secondary.opacity(0.9))
                        .text_color(if is_error { cx.theme().danger } else { muted })
                        .text_size(px(11.))
                        .child(label)
                        .when(running, |chip| {
                            chip.child(Spinner::new().with_size(px(11.)))
                        })
                        .when_some(icon, |chip, icon| {
                            chip.child(gpui_kit::component::Icon::new(icon).size(px(11.)))
                        }),
                )
                .child(div().flex_1().h(px(1.)).bg(border)),
        );
        let mut body = v_flex().w_full().gap(px(8.)).px(px(18.)).py(px(12.));
        if !self.output.is_empty() {
            let dictionary = self
                .request
                .as_ref()
                .is_some_and(|r| engine::possible_word(&r.text));
            let text = if dictionary {
                let html = self
                    .dictionary_html
                    .get_or_init(|| dictionary::html(&self.output).into());
                TextView::html("dictionary-entry", html.clone())
                    .link(|_, _| None)
                    .image_source(|_| ImageSource::Custom(Arc::new(|_, _| None)))
                    .scrollable(false)
                    .into_any_element()
            } else {
                SelectableText::new("translation", self.output.clone()).into_any_element()
            };
            body = body.child(
                div()
                    .w_full()
                    .text_size(px(self.preferences.font_size.into()))
                    .line_height(px(f32::from(self.preferences.font_size) * 1.55))
                    .child(text),
            );
        }
        if let Status::Error(error) = &self.status {
            body = body.child(error_banner("translation-error", error, cx));
        }
        body = body.child(
            h_flex()
                .w_full()
                .gap_1()
                .justify_end()
                .when(!running, |bar| {
                    bar.child(
                        icon_button("retry", IconName::RotateCw, "重新翻译")
                            .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                    )
                })
                .when(!self.output.is_empty(), |bar| {
                    bar.child(
                        Clipboard::new("copy-target")
                            .small()
                            .value(self.output.clone())
                            .tooltip("复制译文")
                            .accessibility_label("复制译文"),
                    )
                }),
        );
        result.child(
            div()
                .id("result-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(body),
        )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut footer = h_flex()
            .w_full()
            .flex_shrink_0()
            .h(px(42.))
            .px(px(14.))
            .gap(px(9.))
            .items_center()
            .bg(cx.theme().background.opacity(0.60))
            .border_t_1()
            .border_color(cx.theme().border.opacity(0.65));
        if self.page == Page::Translator {
            footer = footer
                .child(
                    icon_button("settings", IconName::Settings, "设置 (Ctrl+,)")
                        .on_click(cx.listener(|this, _, w, cx| this.show_settings(w, cx))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Powered by  {}", self.settings.model)),
                )
                .child(div().when(self.update.actionable(), |el| {
                    el.child(
                        Button::new("update-notice")
                            .ghost()
                            .small()
                            .label("软件更新")
                            .on_click(cx.listener(|this, _, w, cx| this.show_settings(w, cx))),
                    )
                }))
                .child(
                    icon_button("history", IconName::RotateCcwClock, "翻译历史")
                        .on_click(cx.listener(|this, _, w, cx| this.show_history(w, cx))),
                );
        } else {
            footer = footer.child(
                Button::new("back")
                    .small()
                    .ghost()
                    .icon(IconName::ArrowLeft)
                    .label("返回")
                    .tooltip("返回翻译 (Esc)")
                    .on_click(cx.listener(|this, _, w, cx| this.back(w, cx))),
            );
            if self.page == Page::Settings {
                footer = footer.child(div().flex_1()).child(
                    Button::new("save")
                        .primary()
                        .small()
                        .label("保存")
                        .on_click(cx.listener(|this, _, w, cx| this.save(w, cx))),
                );
            }
        }
        footer
    }

    fn render_history(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut header = page_header("翻译历史", cx).child(div().flex_1());
        if !self.entries.is_empty() {
            if self.confirming_clear {
                header = header
                    .child(
                        Button::new("cancel-clear")
                            .small()
                            .ghost()
                            .label("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirming_clear = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("confirm-clear")
                            .danger()
                            .small()
                            .label("确认清空")
                            .on_click(cx.listener(|this, _, _, cx| {
                                match history::clear() {
                                    Ok(()) => {
                                        this.entries.clear();
                                        this.notice = None;
                                    }
                                    Err(e) => this.notice = Some(e),
                                }
                                this.confirming_clear = false;
                                cx.notify();
                            })),
                    );
            } else {
                header = header.child(
                    icon_button("clear-history", IconName::Trash, "清空历史记录").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.confirming_clear = true;
                            cx.notify();
                        }),
                    ),
                );
            }
        }
        let mut list = v_flex().w_full().px(px(18.));
        if self.entries.is_empty() {
            list = list.child(
                Label::new("暂无翻译记录")
                    .py(px(25.))
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            );
        }
        for (ix, entry) in self.entries.iter().enumerate() {
            let name = |s: &str| language(s).map(|l| l.1).unwrap_or(AUTO.1);
            let snippet = |s: &str| {
                s.chars()
                    .take(130)
                    .collect::<String>()
                    .replace(['\n', '\r'], " ")
            };
            list = list.child(
                v_flex()
                    .py(px(10.))
                    .border_b_1()
                    .border_color(cx.theme().border.opacity(0.65))
                    .gap(px(4.))
                    .child(
                        h_flex()
                            .items_center()
                            .child(
                                Label::new(format!(
                                    "{} → {}",
                                    name(&entry.source),
                                    name(&entry.target)
                                ))
                                .id(("history-language", ix))
                                .text_xs()
                                .text_color(cx.theme().muted_foreground),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new(("restore", ix))
                                    .small()
                                    .ghost()
                                    .label("恢复")
                                    .accessibility_label(format!("恢复第 {} 条翻译", ix + 1))
                                    .on_click(
                                        cx.listener(move |this, _, w, cx| this.restore(ix, w, cx)),
                                    ),
                            ),
                    )
                    .child(div().truncate().child(snippet(&entry.original)))
                    .child(
                        div()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(snippet(&entry.translated)),
                    ),
            );
        }
        v_flex().flex_1().min_h_0().child(header).child(
            div()
                .id("history-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(list),
        )
    }
}

fn error_banner(id: &'static str, message: &str, cx: &App) -> Div {
    div()
        .px(px(12.))
        .py(px(8.))
        .text_sm()
        .rounded(px(8.))
        .bg(cx.theme().danger.opacity(0.09))
        .text_color(cx.theme().danger)
        .child(SelectableText::new(id, message.to_owned()))
}

fn page_header(title: &'static str, cx: &App) -> Div {
    h_flex()
        .h(px(62.))
        .flex_shrink_0()
        .w_full()
        .px(px(18.))
        .items_center()
        .gap_2()
        .bg(cx.theme().background.opacity(0.56))
        .border_b_1()
        .border_color(cx.theme().border.opacity(0.65))
        .child(
            Label::new(title)
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD),
        )
}

impl Render for Translator {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = v_flex()
            .key_context("Translator")
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .text_color(cx.theme().foreground)
            .bg(cx
                .theme()
                .background
                .opacity(if cx.theme().is_dark() { 0.70 } else { 0.77 }))
            .on_action(cx.listener(|this, _: &OpenSettings, w, cx| this.show_settings(w, cx)))
            .on_action(cx.listener(|this, _: &GoBack, w, cx| {
                if this.page != Page::Translator {
                    this.back(w, cx)
                } else {
                    this.stop(cx)
                }
            }))
            .child(
                TitleBar::new()
                    .bg(cx.theme().background.opacity(0.36))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(img(self.app_icon.clone()).size(px(16.)))
                            .child(Label::new("NextAI Translator").text_sm()),
                    ),
            );
        root = match self.page {
            Page::Translator => root.child(self.render_header(cx)).child(
                div().flex_1().min_h_0().w_full().child(
                    v_resizable("translation-split")
                        .when(self.status != Status::Idle, |group| {
                            group.with_handle_appearance(std::rc::Rc::new(|handle, _, cx| {
                                Some(
                                    div()
                                        .w_full()
                                        .h(px(1.))
                                        .when(handle.is_active(), |line| {
                                            line.bg(cx.theme().muted_foreground)
                                        })
                                        .into_any_element(),
                                )
                            }))
                        })
                        .child(
                            resizable_panel()
                                .size(px(291.))
                                .size_range(px(118.)..Pixels::MAX)
                                .fixed()
                                .child(self.render_editor(cx)),
                        )
                        .child(
                            resizable_panel()
                                .size_range(px(82.)..Pixels::MAX)
                                .child(self.render_result(cx)),
                        ),
                ),
            ),
            Page::Settings => root.child(self.render_settings(cx)),
            Page::History => root.child(self.render_history(cx)),
        };
        if let Some(notice) = &self.notice {
            root = root.child(
                div()
                    .mx(px(18.))
                    .my(px(6.))
                    .child(error_banner("notice", notice, cx)),
            );
        }
        root.child(self.render_footer(cx))
    }
}

#[cfg(test)]
mod interaction_tests;

#[cfg(test)]
mod tests {
    use super::{CHINESE, ENGLISH, LANGUAGES, automatic_target, detect, language};
    #[test]
    fn languages_match_their_reverse_and_auto_directions() {
        for lang in LANGUAGES {
            assert!(language(lang.2.unwrap_or(lang.0)).unwrap().2.is_none());
            assert_eq!(
                LANGUAGES.iter().filter(|other| other.0 == lang.0).count(),
                1
            );
        }
        assert_eq!(
            automatic_target(detect("这是用于检测语言的中文句子。"), CHINESE, ENGLISH),
            ENGLISH
        );
        assert_eq!(
            automatic_target(detect("これは日本語です。"), CHINESE, ENGLISH),
            CHINESE
        );
        assert_eq!(detect("안녕하세요"), Some("Korean"));
        assert_eq!(
            detect("這是用於檢測語言的繁體中文句子。"),
            Some("Traditional Chinese")
        );
        let mixed = "## 我是谁？\n我叫 James，是一名电气工程专业的学生。我喜欢开源项目。\n{{< gh-repo-card repo=\"JamesFlare1212/FlareBlog\" >}}";
        assert_eq!(automatic_target(detect(mixed), CHINESE, "French"), "French");
        assert_eq!(automatic_target(Some(ENGLISH), ENGLISH, CHINESE), CHINESE);
        assert_eq!(detect(""), None);
        assert_eq!(detect("The cat is sleeping on the chair."), Some(ENGLISH));
        assert_eq!(
            automatic_target(Some("Spanish"), "German", ENGLISH),
            "German"
        );
    }
}
