//! Capabilities 4+6 POC: a static price-check card layout exercising mixed Latin+Cyrillic text
//! rendering.
//!
//! The default GPUI text style on Linux resolves to the sentinel family `.SystemUIFont`, which
//! `cosmic-text`'s fontdb maps to `"IBM Plex Sans"` (Zed's bundled UI font) -- not installed in
//! this POC's minimal runner image (only `fonts-dejavu-core` is). `.font_family("DejaVu Sans")`
//! is set explicitly on the root div so the whole subtree inherits it through GPUI's
//! `text_style_stack`; DejaVu Sans covers both Latin and Cyrillic in one family, unlike most
//! Latin-only fonts.

use gpui::{App, Bounds, Context, Render, Window, WindowBounds, div, prelude::*, px, rgb, size};
use gpui_platform::application;

struct PriceCheckCard;

/// One modifier line: Latin numeric roll + a Cyrillic description, exactly the mix a real
/// price-check overlay renders for an RU-localized client.
const MODIFIERS: &[&str] = &[
    "+38% к сопротивлению холоду",
    "Добавляет от 12 до 24 урона от огня к атакам",
    "16% увеличение скорости атаки",
    "+124 к максимальному запасу энергощита",
];

impl Render for PriceCheckCard {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .font_family("DejaVu Sans")
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x1a1a1a))
            .p_3()
            .child(
                // Card
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .rounded_md()
                    .border_2()
                    .border_color(rgb(0xaf6025))
                    .bg(rgb(0x0c0c0e))
                    .overflow_hidden()
                    .child(
                        // Header: rarity strip
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .py_2()
                            .bg(rgb(0x3a2410))
                            .child(
                                div()
                                    .text_lg()
                                    .text_color(rgb(0xd0913b))
                                    .child("Rime Gaze, Ornate Mask"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x8a8a8a))
                                    .child("Церемониальная маска"),
                            ),
                    )
                    .child(
                        // Modifier list
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap_1()
                            .p_3()
                            .children(MODIFIERS.iter().map(|line| {
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x8ea9e8))
                                    .child(*line)
                            })),
                    )
                    .child(
                        // Price footer
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .bg(rgb(0x161616))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x9a9a9a))
                                    .child("Цена продавца"),
                            )
                            .child(
                                div()
                                    .text_lg()
                                    .text_color(rgb(0xffffff))
                                    .child("7 divine orb"),
                            ),
                    ),
            )
    }
}

fn main() {
    env_logger::init();
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(400.), px(480.)), cx);
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Oracle POC — text_rendering");
                cx.new(|_cx| PriceCheckCard)
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
