use super::*;

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

type ModelChoices = SearchableVec<String>;
type ModelMenu = Entity<SelectState<ModelChoices>>;

pub(super) struct ModelLookup {
    draft: Settings,
    state: Models,
}

enum Models {
    Loading {
        _task: Task<()>,
    },
    Ready {
        menu: ModelMenu,
        _subscription: Subscription,
    },
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
        cx.observe_in(input, window, |this, _, _, cx| {
            this.discard_stale_models(cx)
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

    pub(super) fn discard_stale_models(&mut self, cx: &mut Context<Self>) {
        if self.form.models.is_some() && self.current_models(cx).is_none() {
            self.form.cancel_models();
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
            model: self.model.read(cx).value().to_string(),
            api_mode: self.form.api_mode(cx),
            reasoning: self.form.reasoning(),
        };
        let state = match validate_draft_origin(&self.settings, &draft) {
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = match result {
            Err(error) => Models::Error(error),
            Ok(ids) if ids.is_empty() => Models::Error("服务未列出可用模型，请手动填写。".into()),
            Ok(ids) => {
                let menu = cx.new(|cx| {
                    SelectState::new(ModelChoices::new(ids), None, window, cx).searchable(true)
                });
                let subscription = cx.subscribe_in(&menu, window, |this, source, event: &SelectEvent<ModelChoices>, w, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event
                        && matches!(this.current_models(cx), Some(Models::Ready { menu, .. }) if menu == source)
                    {
                        this.model.update(cx, |s, cx| s.set_value(id.clone(), w, cx));
                        source.update(cx, |s, cx| s.set_selected_index(None, w, cx));
                        cx.notify();
                    }
                });
                Models::Ready {
                    menu,
                    _subscription: subscription,
                }
            }
        };
        if let Some(lookup) = &mut self.form.models {
            lookup.state = state;
        }
        cx.notify();
    }

    pub(super) fn render_models(&self, cx: &mut Context<Self>) -> Div {
        let state = self.current_models(cx);
        let loading = matches!(state, Some(Models::Loading { .. }));
        let controls = h_flex()
            .w_full()
            .gap(px(8.))
            .items_center()
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.model)
                        .focus_bordered(false)
                        .aria_label("模型"),
                ),
            )
            .when_some(state, |row, state| match state {
                Models::Ready { menu, .. } => row.child(
                    div().w(px(100.)).h(px(28.)).flex_shrink_0().child(
                        Select::new(menu)
                            .id("model-picker")
                            .small()
                            .placeholder("选择模型")
                            .accessibility_label("选择模型")
                            .search_placeholder("搜索模型")
                            .focus_ring(false)
                            .menu_width(px(270.))
                            .menu_max_h(px(220.)),
                    ),
                ),
                _ => row,
            })
            .child(
                Button::new("fetch-models")
                    .small()
                    .label(if matches!(state, Some(Models::Ready { .. })) {
                        "刷新"
                    } else {
                        "获取模型"
                    })
                    .loading(loading)
                    .disabled(loading)
                    .on_click(cx.listener(|this, _, w, cx| {
                        this.refresh_models(engine::list_models, w, cx)
                    })),
            );
        let row = v_flex().w_full().gap(px(6.)).child(field("模型", controls));
        if let Some(Models::Error(message)) = state {
            row.child(error_banner("models-error", message, cx))
        } else {
            row
        }
    }
}
