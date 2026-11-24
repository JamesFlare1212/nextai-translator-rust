use super::*;
use crate::updater::{self, Update};
use tempfile::TempDir;

pub(super) enum State {
    Idle,
    Checking,
    Current,
    Available(Update),
    Downloading,
    Ready(Update, TempDir),
    Failed(String),
}
impl State {
    pub(super) fn actionable(&self) -> bool {
        matches!(self, Self::Available(_) | Self::Ready(..))
    }
}
impl Translator {
    pub(super) fn check_update(&mut self, cx: &mut Context<Self>) {
        if matches!(self.update, State::Checking | State::Downloading) {
            return;
        }
        self.update = State::Checking;
        let kind = self.package;
        let worker = cx.background_spawn(async move { updater::run(updater::check(kind)) });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.update = match result {
                    Ok(Some(u)) => State::Available(u),
                    Ok(None) => State::Current,
                    Err(e) => State::Failed(e),
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn update_action(&mut self, cx: &mut Context<Self>) {
        match &self.update {
            State::Available(u) => {
                let u = u.clone();
                self.update = State::Downloading;
                let worker = cx.background_spawn(async move {
                    updater::run(updater::download(&u)).map(|dir| (u, dir))
                });
                cx.spawn(async move |this, cx| {
                    let result = worker.await;
                    let _ = this.update(cx, |this, cx| {
                        this.update = match result {
                            Ok((u, dir)) => State::Ready(u, dir),
                            Err(e) => State::Failed(e),
                        };
                        cx.notify();
                    });
                })
                .detach();
            }
            State::Ready(u, dir) => {
                if self.status == Status::Running {
                    return;
                }
                match updater::restart(dir.path(), self.package, u) {
                    Ok(()) => {
                        if let State::Ready(_, dir) =
                            std::mem::replace(&mut self.update, State::Idle)
                        {
                            let _ = dir.keep();
                        }
                        cx.quit();
                    }
                    Err(e) => self.update = State::Failed(e),
                }
            }
            State::Checking | State::Downloading => return,
            _ => self.check_update(cx),
        }
        cx.notify();
    }
    pub(super) fn update_panel(&self, cx: &mut Context<Self>) -> Div {
        let (title, detail, action) = match &self.update {
            State::Idle => ("软件更新".into(), "启动时自动检查稳定版".into(), "检查更新"),
            State::Checking => (
                "正在检查更新".into(),
                "正在连接 GitHub Releases…".into(),
                "检查中",
            ),
            State::Current => (
                "已是最新版本".into(),
                "启动时自动检查稳定版".into(),
                "再次检查",
            ),
            State::Available(u) => (
                format!("新版本 {}", u.version),
                format!("更新包 {:.1} MB · 下载后可稍后重启", u.size as f64 / 1e6),
                "下载更新",
            ),
            State::Downloading => (
                "正在下载更新".into(),
                "下载后将校验文件，可继续翻译".into(),
                "下载中",
            ),
            State::Ready(u, _) => (
                format!("{} 已准备就绪", u.version),
                if self.status == Status::Running {
                    "请等待当前翻译结束"
                } else {
                    "重启前请保存设置，并复制需要保留的草稿"
                }
                .into(),
                "重启并更新",
            ),
            State::Failed(e) => ("更新未完成".into(), e.clone(), "重新检查"),
        };
        let busy = matches!(self.update, State::Checking | State::Downloading);
        let button = Button::new("software-update").small().label(action);
        let button = if self.update.actionable() {
            button.primary()
        } else {
            button.outline()
        };
        v_flex()
            .p(px(16.))
            .gap(px(12.))
            .rounded(px(12.))
            .bg(cx.theme().secondary.opacity(0.55))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(Label::new("软件更新").text_sm())
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{}  ·  v{}",
                                self.package.label(),
                                updater::CURRENT
                            )),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(4.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(detail),
                            ),
                    )
                    .when(busy, |el| el.child(Spinner::new().with_size(px(14.))))
                    .child(
                        button
                            .disabled(
                                busy || (matches!(self.update, State::Ready(..))
                                    && self.status == Status::Running),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.update_action(cx))),
                    ),
            )
    }
}
