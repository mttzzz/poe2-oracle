//! A plain text tooltip: what a chip, marker or button means, shown on hover.

use gpui::{
    AnyView, App, AppContext as _, Context, IntoElement, Render, SharedString, Window, div,
    prelude::*, rgb,
};

use crate::ui::theme::{BG_PANEL, BORDER_GOLD, TEXT, rems_from_px};

struct Hint(SharedString);

impl Render for Hint {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(rems_from_px(320.))
            .px(rems_from_px(8.))
            .py(rems_from_px(5.))
            .rounded_xs()
            .bg(rgb(BG_PANEL))
            .border_1()
            .border_color(rgb(BORDER_GOLD))
            .text_xs()
            .text_color(rgb(TEXT))
            .child(self.0.clone())
    }
}

/// The builder `tooltip` takes, for a tooltip saying `text`.
pub fn hint(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_window, cx| cx.new(|_| Hint(text.clone())).into()
}
