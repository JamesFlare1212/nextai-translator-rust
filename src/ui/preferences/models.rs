use super::*;
use gpui_kit::component::{
    combobox::{Combobox, ComboboxState},
    select::{SelectDelegate, SelectItem},
};

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

#[derive(Clone)]
struct Choice {
    id: String,
    custom: bool,
}

impl SelectItem for Choice {
    type Value = String;
    fn title(&self) -> SharedString {
        self.id.clone().into()
    }
    fn value(&self) -> &String {
        &self.id
    }
    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        if self.custom {
            format!("使用「{}」", self.id)
        } else {
            self.id.clone()
        }
    }
}

struct Choices {
    ids: Vec<String>,
    rows: Vec<Choice>,
}

impl Choices {
    fn new(mut ids: Vec<String>, selected: &str, query: &str) -> Self {
        if !selected.is_empty() && !ids.iter().any(|s| s == selected) {
            ids.insert(0, selected.into());
        }
        let mut result = Self {
            ids,
            rows: Vec::new(),
        };
        result.filter(query);
        result
    }

    fn filter(&mut self, query: &str) {
        let query = query.trim();
        let needle = query.to_lowercase();
        self.rows = self
            .ids
            .iter()
            .filter(|id| id.to_lowercase().contains(&needle))
            .map(|id| Choice {
                id: id.clone(),
                custom: false,
            })
            .collect();
        if !query.is_empty() && !self.rows.iter().any(|r| r.id == query) {
            self.rows.push(Choice {
                id: query.into(),
                custom: true,
            });
        }
    }
}

impl SelectDelegate for Choices {
    type Item = Choice;
    fn items_count(&self, _: usize) -> usize {
        self.rows.len()
    }
    fn item(&self, ix: IndexPath) -> Option<&Choice> {
        self.rows.get(ix.row)
    }
    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Choice: SelectItem<Value = V>,
        V: PartialEq,
    {
        self.rows
            .iter()
            .position(|r| r.value() == value)
            .map(IndexPath::new)
    }
    fn perform_search(&mut self, query: &str, _: &mut Window, _: &mut App) -> Task<()> {
        self.filter(query);
        Task::ready(())
    }
    fn on_confirm(&mut self, selection: &[(IndexPath, Choice)]) {
        if let Some((_, row)) = selection.first()
            && row.custom
            && !self.ids.contains(&row.id)
        {
            self.ids.insert(0, row.id.clone());
        }
    }
}

pub(super) struct ModelPicker {
    menu: Entity<ComboboxState<Choices>>,
}

impl ModelPicker {
    pub(super) fn new(id: &str, window: &mut Window, cx: &mut Context<Translator>) -> Self {
        let items = Choices::new(Vec::new(), id, "");
        let selection = (!id.is_empty())
            .then(|| IndexPath::new(0))
            .into_iter()
            .collect();
        let menu = cx.new(|cx| ComboboxState::new(items, selection, window, cx).searchable(true));
        let trigger = menu.focus_handle(cx);
        let mut was_open = false;
        cx.observe_in(&menu, window, move |this, menu, w, cx| {
            let open = menu.focus_handle(cx) != trigger;
            if open == was_open {
                return;
            }
            was_open = open;
            if open {
                this.refresh_models(engine::list_models, w, cx);
            } else if !menu.read(cx).query(cx).is_empty() {
                menu.update(cx, |s, cx| s.set_query("", w, cx));
            }
        })
        .detach();
        Self { menu }
    }

    pub(super) fn value(&self, cx: &App) -> String {
        self.menu.read(cx).selected_value().unwrap_or_default()
    }

    pub(super) fn reset(&self, id: &str, w: &mut Window, cx: &mut App) {
        self.menu.update(cx, |s, cx| {
            s.set_items(Choices::new(vec![], id, ""), w, cx);
            s.set_selected_values(&[id.to_owned()], w, cx);
        });
    }

    fn replace_choices(&self, ids: Vec<String>, w: &mut Window, cx: &mut App) {
        self.menu.update(cx, |s, cx| {
            let query = s.query(cx);
            let selected = s.selected_value().unwrap_or_default();
            s.set_items(Choices::new(ids, &selected, &query), w, cx);
            s.set_query(query, w, cx);
            cx.notify();
        });
    }
}

pub(super) struct ModelLookup {
    draft: Settings,
    state: Models,
}
enum Models {
    Loading { _task: Task<()> },
    Ready,
    Error(String),
}
pub(in crate::ui) fn validate_draft_origin(
    saved: &Settings,
    draft: &Settings,
) -> Result<(), String> {
    let key = draft.api_key.trim();
    if key.is_empty() || key != saved.api_key.trim() {
        return Ok(());
    }
    let origin = |s: &str| reqwest::Url::parse(s.trim()).ok().map(|u| u.origin());
    if origin(&saved.endpoint).is_some_and(|original| Some(original) == origin(&draft.endpoint)) {
        Ok(())
    } else {
        Err("更换服务地址后，请清除原服务的密钥或使用新服务的密钥。".into())
    }
}

pub(in crate::ui) fn observe_model_inputs(
    endpoint: &Entity<InputState>,
    api_key: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<Translator>,
) {
    for input in [endpoint, api_key] {
        cx.observe_in(input, window, |this, _, w, cx| {
            this.discard_stale_models(w, cx)
        })
        .detach();
    }
}

impl Translator {
    fn current_models(&self, cx: &App) -> Option<&Models> {
        let lookup = self.form.models.as_ref()?;
        (self.page == Page::Settings
            && lookup.draft.api_mode == self.form.api_mode(cx)
            && self.endpoint.read(cx).text() == lookup.draft.endpoint.as_str()
            && self.api_key.read(cx).text() == lookup.draft.api_key.as_str())
        .then_some(&lookup.state)
    }

    pub(super) fn discard_stale_models(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.form.models.is_some() && self.current_models(cx).is_none() {
            self.form.cancel_models();
            self.form.model.replace_choices(vec![], w, cx);
            cx.notify();
        }
    }

    fn refresh_models(
        &mut self,
        fetch: impl FnOnce(&Settings) -> Result<Vec<String>, String> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != Page::Settings
            || matches!(self.current_models(cx), Some(Models::Loading { .. }))
        {
            return;
        }
        let draft = Settings {
            endpoint: self.endpoint.read(cx).value().to_string(),
            api_key: self.api_key.read(cx).value().to_string(),
            model: self.form.model.value(cx),
            api_mode: self.form.api_mode(cx),
            reasoning: self.form.reasoning(),
        };
        let state = match self.check_origin(&draft) {
            Err(error) => Models::Error(error),
            Ok(()) => {
                let settings = draft.clone();
                let worker = cx.background_spawn(async move { fetch(&settings) });
                let task = cx.spawn_in(window, async move |this, cx| {
                    let result = worker.await;
                    _ = this.update_in(cx, |this, w, cx| {
                        if matches!(this.current_models(cx), Some(Models::Loading { .. })) {
                            this.finish_models(result, w, cx);
                        }
                    });
                });
                Models::Loading { _task: task }
            }
        };
        self.form.models = Some(ModelLookup { draft, state });
        cx.notify();
    }

    fn finish_models(
        &mut self,
        result: Result<Vec<String>, String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = match result {
            Ok(ids) if !ids.is_empty() => {
                self.form.model.replace_choices(ids, w, cx);
                Models::Ready
            }
            Ok(_) => {
                self.form.model.replace_choices(vec![], w, cx);
                Models::Error("未找到适合翻译的模型，可输入自定义名称。".into())
            }
            Err(error) => Models::Error(error),
        };
        if let Some(lookup) = &mut self.form.models {
            lookup.state = state;
        }
        cx.notify();
    }

    pub(super) fn render_models(&self, cx: &mut Context<Self>) -> Div {
        let menu = Combobox::new(&self.form.model.menu)
            .w_full()
            .placeholder("选择或输入模型名")
            .search_placeholder("搜索或输入模型名")
            .menu_max_h(px(220.))
            .focus_ring(false);
        let menu = match self.current_models(cx) {
            Some(Models::Loading { .. }) => menu.footer(|_, cx| {
                Label::new("正在获取模型…")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
            }),
            Some(Models::Error(error)) => {
                let error = error.clone();
                menu.footer(move |_, cx| {
                    Label::new(error.clone())
                        .text_sm()
                        .text_color(cx.theme().danger)
                })
            }
            _ => menu,
        };
        let loading = matches!(self.current_models(cx), Some(Models::Loading { .. }));
        field(
            "模型",
            h_flex()
                .size_full()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .id("model-picker")
                        .test_support()
                        .aria_label("模型")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(menu),
                )
                .child(
                    icon_button("refresh-models", IconName::RotateCw, "刷新模型列表")
                        .disabled(loading)
                        .loading(loading)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.refresh_models(engine::list_models, window, cx)
                        })),
                ),
        )
    }
}
