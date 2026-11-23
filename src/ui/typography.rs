use super::*;
use crate::config::{DEFAULT_FONT, DEFAULT_SIZE, FONT_SIZES, Preferences};
use gpui_kit::component::Theme;

type FontList = Entity<SelectState<SearchableVec<String>>>;

pub(super) struct FontControls {
    family: FontList,
    size: Entity<SelectState<Vec<String>>>,
}

impl FontControls {
    pub(super) fn new(prefs: &Preferences, window: &mut Window, cx: &mut App) -> Self {
        let mut names = cx.text_system().all_font_names();
        names.retain(|name| !name.is_empty() && !name.starts_with(['@', '.']));
        names.extend([DEFAULT_FONT.to_owned(), prefs.font_family.clone()]);
        names.sort_unstable_by_key(|s| s.to_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let selected = names
            .iter()
            .position(|s| s.eq_ignore_ascii_case(&prefs.font_family))
            .unwrap_or_default();
        let family = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(names),
                Some(IndexPath::default().row(selected)),
                window,
                cx,
            )
            .searchable(true)
        });
        let sizes = FONT_SIZES.map(|n| n.to_string()).collect::<Vec<_>>();
        let selected = sizes
            .iter()
            .position(|s| s == &prefs.font_size.to_string())
            .unwrap_or_default();
        let size = cx.new(|cx| {
            SelectState::new(sizes, Some(IndexPath::default().row(selected)), window, cx)
        });
        Self { family, size }
    }

    pub(super) fn reset(&self, prefs: &Preferences, window: &mut Window, cx: &mut App) {
        self.family.update(cx, |s, cx| {
            s.set_selected_value(&prefs.font_family, window, cx)
        });
        self.size.update(cx, |s, cx| {
            s.set_selected_value(&prefs.font_size.to_string(), window, cx)
        });
    }

    pub(super) fn write(&self, prefs: &mut Preferences, cx: &App) {
        prefs.font_family = self
            .family
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_else(|| DEFAULT_FONT.into());
        prefs.font_size = self
            .size
            .read(cx)
            .selected_value()
            .and_then(|s| s.parse().ok())
            .filter(|n| FONT_SIZES.contains(n))
            .unwrap_or(DEFAULT_SIZE);
    }

    pub(super) fn render(&self) -> Div {
        h_flex()
            .gap(px(14.))
            .w_full()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(7.))
                    .child(
                        Label::new("字体")
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        div().w_full().h(px(32.)).child(
                            Select::new(&self.family)
                                .w_full()
                                .menu_max_h(px(230.))
                                .search_placeholder("搜索字体")
                                .accessibility_label("字体"),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .w(px(116.))
                    .gap(px(7.))
                    .child(
                        Label::new("正文字号 (px)")
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        div().w_full().h(px(32.)).child(
                            Select::new(&self.size)
                                .w_full()
                                .menu_max_h(px(220.))
                                .accessibility_label("正文字号"),
                        ),
                    ),
            )
    }
}

pub(super) fn apply(prefs: &Preferences, cx: &mut App) {
    let installed = cx.text_system().all_font_names();
    let family = installed
        .iter()
        .find(|s| s.eq_ignore_ascii_case(&prefs.font_family))
        .map(String::as_str)
        .unwrap_or(".SystemUIFont");
    Theme::update(cx, |theme| theme.font_family = family.to_owned().into());
}
