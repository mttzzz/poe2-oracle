//! The trade overlay: live search's cards (`crate::live_search`) at the top of the game, newest
//! first -- each new listing of a watched search, with its whisper to copy and the search to open
//! on the trade site, and a watch the site ended.
//!
//! The window is interactive and shown only while it has cards, the game (or this app) is in
//! front -- a task looks every second -- and the price panel is closed. A click never activates it
//! (`Win32Overlay::set_no_activate`): the game keeps the keyboard.

use std::time::Duration;

use async_channel::Receiver;
use gpui::{
    App, AsyncApp, Bounds, ClipboardItem, Context, Entity, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, Render, WeakEntity, Window, WindowBounds, WindowKind, WindowOptions, div,
    point, prelude::*, px, rgb, size,
};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::live_search::{LiveCard, LiveListing, SHOWN_LISTINGS};
use crate::overlay_layout::PhysicalRect;
use crate::platform::game_window::{self, Foreground};
use crate::platform::win32::Win32Overlay;
use crate::price_check::PriceCheckApp;
use crate::settings::Settings;
use crate::ui::hint::hint;
use crate::ui::panel::format::{currency_img, format_ru};
use crate::ui::theme::{
    BASE_REM_SIZE, BG_BUTTON_HOVER, BG_CLOSE_HOVER, BG_CONTROL, BG_NAMEPLATE, BG_PANEL, BORDER,
    BORDER_GOLD, GOLD, PRICE_RISE, TEXT, TEXT_DIM, TEXT_MUTED, TEXT_WARNING, rems_from_px,
};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// The window's logical width, a card's height, the gap between cards and the frame's padding, all
/// at 100 % scale.
const WIDTH: f32 = 450.;
const LIVE_CARD_HEIGHT: f32 = 92.;
const GAP: f32 = 6.;
const PADDING: f32 = 6.;
/// Where the window's top sits below the game's, as a share of the game's height: under the top
/// edge's boss bar and area banner.
const TOP: f64 = 0.1;

/// The player's settings the overlay follows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TradeOverlayOptions {
    pub ui_scale: f32,
}

impl TradeOverlayOptions {
    pub fn from_settings(settings: &Settings) -> TradeOverlayOptions {
        TradeOverlayOptions {
            ui_scale: settings.ui_scale,
        }
    }
}

/// A live search card on screen.
struct LiveShown {
    id: u64,
    card: LiveCard,
    /// When it came: `03:04`.
    time: String,
    /// Its listing's whisper was copied.
    copied: bool,
}

/// The overlay window's root view.
pub struct TradeOverlay {
    /// Live search's cards, newest first.
    live: Vec<LiveShown>,
    next_id: u64,
    options: TradeOverlayOptions,
    /// For the price's currency icon, from the trade site's catalog.
    app: WeakEntity<PriceCheckApp>,
    suppressed: bool,
    /// The game or this app is in front: over any other program the overlay stays hidden.
    in_front: bool,
    overlay: Option<Win32Overlay>,
    last_bounds: Option<PhysicalRect>,
    last_shown: Option<bool>,
}

/// Opens the overlay window -- hidden until a card comes -- and starts taking `live_cards` and
/// looking at what is in front. The window owns the returned view; keep the handle only to call
/// [`TradeOverlay::set_suppressed`] and [`TradeOverlay::set_options`].
pub fn open(
    options: TradeOverlayOptions,
    app: WeakEntity<PriceCheckApp>,
    live_cards: Receiver<LiveCard>,
    cx: &mut App,
) -> anyhow::Result<Entity<TradeOverlay>> {
    let window = cx.open_window(window_options(), |window, cx| {
        window.set_window_title("PoE2 Oracle — торговля");
        cx.new(|_| TradeOverlay {
            live: Vec::new(),
            next_id: 0,
            options,
            app,
            suppressed: false,
            in_front: false,
            overlay: None,
            last_bounds: None,
            last_shown: None,
        })
    })?;
    let view = window.entity(cx)?;
    let weak = view.downgrade();
    cx.spawn(async move |cx| poll_forever(weak, cx).await)
        .detach();
    let weak = view.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(first) = live_cards.recv().await {
            let cards: Vec<LiveCard> = std::iter::once(first)
                .chain(std::iter::from_fn(|| live_cards.try_recv().ok()))
                .collect();
            let Some(view) = weak.upgrade() else {
                return;
            };
            view.update(cx, |view, cx| view.take_live(cards, cx));
        }
    })
    .detach();
    Ok(view)
}

fn window_options() -> WindowOptions {
    WindowOptions {
        // Placeholder: the first card places the window over the game, then shows it.
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(WIDTH), px(LIVE_CARD_HEIGHT)),
        ))),
        titlebar: None,
        kind: WindowKind::PopUp,
        is_movable: false,
        focus: false,
        show: false,
        ..Default::default()
    }
}

/// The local time of day to the minute: `03:04`.
fn local_time() -> String {
    let now = unsafe { GetLocalTime() };
    format!("{:02}:{:02}", now.wHour, now.wMinute)
}

/// Tells the overlay whether the game or this app is in front, until its window closes.
async fn poll_forever(view: WeakEntity<TradeOverlay>, cx: &mut AsyncApp) {
    loop {
        cx.background_executor().timer(POLL_INTERVAL).await;
        let in_front = game_window::foreground() != Foreground::Other;
        let Some(view) = view.upgrade() else {
            return;
        };
        view.update(cx, |view, cx| view.set_in_front(in_front, cx));
    }
}

impl TradeOverlay {
    /// Hides the overlay while `suppressed` is true -- pass whether the price-check window is
    /// shown, whenever that changes: one would cover the other.
    pub fn set_suppressed(&mut self, suppressed: bool, cx: &mut Context<Self>) {
        self.suppressed = suppressed;
        self.sync_window(cx);
    }

    /// Takes over the player's saved options.
    pub fn set_options(&mut self, options: TradeOverlayOptions, cx: &mut Context<Self>) {
        if options == self.options {
            return;
        }
        self.options = options;
        cx.notify();
        self.sync_window(cx);
    }

    /// Takes in whether the game or this app is in front.
    fn set_in_front(&mut self, in_front: bool, cx: &mut Context<Self>) {
        if in_front != self.in_front {
            self.in_front = in_front;
            cx.notify();
            self.sync_window(cx);
        }
    }

    /// Puts live search's `cards` on top of its others, keeping `SHOWN_LISTINGS`.
    fn take_live(&mut self, cards: Vec<LiveCard>, cx: &mut Context<Self>) {
        let time = local_time();
        for card in cards {
            self.next_id += 1;
            self.live.insert(
                0,
                LiveShown {
                    id: self.next_id,
                    card,
                    time: time.clone(),
                    copied: false,
                },
            );
        }
        self.live.truncate(SHOWN_LISTINGS);
        log::info!("live search card ({} shown)", self.live.len());
        cx.notify();
        self.sync_window(cx);
    }

    fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.live.retain(|shown| shown.id != id);
        log::info!("overlay card dismissed ({} left)", self.live.len());
        cx.notify();
        self.sync_window(cx);
    }

    /// Puts the whisper of live search card `id`'s listing on the clipboard, for the game's chat:
    /// copying, not sending, as in the results table -- the player sends it.
    fn copy_whisper(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(shown) = self.live.iter_mut().find(|shown| shown.id == id) else {
            return;
        };
        let LiveCard::Listing(LiveListing {
            whisper: Some(whisper),
            ..
        }) = &shown.card
        else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(whisper.clone()));
        shown.copied = true;
        log::info!("live search: whisper copied");
        cx.notify();
    }

    /// The window's rect: the cards' size at the game's DPI and the player's scale, centred at
    /// the top of the game.
    fn placement(&self) -> Option<PhysicalRect> {
        let (game, dpi_scale) = game_window::game_client()?;
        let scale = dpi_scale * f64::from(self.options.ui_scale);
        // One card's height while there are none: the window is hidden then anyway.
        let shown = self.live.len().max(1) as f64;
        let height = shown * f64::from(LIVE_CARD_HEIGHT)
            + (shown - 1.) * f64::from(GAP)
            + 2. * f64::from(PADDING);
        let width = (f64::from(WIDTH) * scale).round() as i32;
        Some(PhysicalRect {
            x: game.x + (game.width - width) / 2,
            y: game.y + (f64::from(game.height) * TOP).round() as i32,
            width,
            height: (height * scale).round() as i32,
        })
    }

    fn ensure_overlay(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.overlay.is_some() {
            return;
        }
        match Win32Overlay::from_window(window) {
            Ok(overlay) => {
                if let Err(err) = overlay.disable_dwm_frame() {
                    log::warn!("{err:#}");
                }
                // Spawned before `sync_window`'s first task, so the window is frameless and never
                // takes the keyboard by the time it is first shown.
                cx.spawn(async move |_, _| {
                    if let Err(err) = overlay.remove_frame() {
                        log::warn!("{err:#}");
                    }
                    if let Err(err) = overlay.set_no_activate() {
                        log::warn!("{err:#}");
                    }
                })
                .detach();
                self.overlay = Some(overlay);
                self.sync_window(cx);
            }
            Err(err) => log::warn!("Win32Overlay::from_window failed: {err:?}"),
        }
    }

    /// Brings the platform window in line with the cards. Called from the overlay's tasks,
    /// `set_suppressed` and `set_options`, not only from `render`: a hidden GPUI window is never
    /// redrawn, so a render-only sync could never show it again.
    fn sync_window(&mut self, cx: &mut Context<Self>) {
        let Some(overlay) = self.overlay else {
            return;
        };
        let placement = self.placement();
        let want_shown =
            !self.live.is_empty() && !self.suppressed && self.in_front && placement.is_some();
        let bounds = placement.filter(|rect| self.last_bounds != Some(*rect));
        let shown = (self.last_shown != Some(want_shown)).then_some(want_shown);
        if bounds.is_none() && shown.is_none() {
            return;
        }
        if bounds.is_some() {
            self.last_bounds = bounds;
        }
        self.last_shown = Some(want_shown);
        // Deferred: `SetWindowPos`/`ShowWindow` send `WM_SIZE`/`WM_SHOWWINDOW` synchronously into
        // GPUI's own window state.
        cx.spawn(async move |_, _| {
            if let Some(rect) = bounds
                && let Err(err) = overlay.set_bounds(rect)
            {
                log::warn!("{err:#}");
            }
            if let Some(shown) = shown {
                overlay.set_shown(shown);
            }
        })
        .detach();
    }

    /// A live search card: a new listing -- what, for how much, from whom, with its whisper to
    /// copy and the search to open on the trade site -- or a watch the site ended, and why.
    fn render_live_card(
        &self,
        shown: &LiveShown,
        icon: Option<String>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let id = shown.id;
        let header = |title: String, color: u32| {
            div()
                .flex()
                .items_center()
                .gap(rems_from_px(6.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(rgb(color))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(rgb(TEXT_DIM))
                        .child(shown.time.clone()),
                )
                .child(close_button(id, cx))
        };
        match &shown.card {
            LiveCard::Listing(listing) => {
                let price = listing
                    .price
                    .as_ref()
                    .map(|(amount, currency)| price_tag(*amount, currency, icon));
                let whisper = if listing.whisper.is_none() {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(rgb(TEXT_DIM))
                        .child("мгновенный выкуп")
                        .into_any_element()
                } else if shown.copied {
                    card_button("Скопировано ✓", PRICE_RISE).into_any_element()
                } else {
                    card_button("Скопировать шёпот", TEXT)
                        .id(("copy-whisper", id))
                        .tooltip(hint(
                            "Сообщение продавцу — в буфер обмена: вставьте его в чат игры и \
                             отправьте сами",
                        ))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                                view.copy_whisper(id, cx);
                            }),
                        )
                        .into_any_element()
                };
                let trade_url = listing.trade_url.clone();
                card_frame(LIVE_CARD_HEIGHT, BORDER_GOLD)
                    .child(header(format!("◉ Слежение · {}", listing.search), GOLD))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(rems_from_px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(listing.item.clone()),
                            )
                            .children(price),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(rems_from_px(4.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(rgb(TEXT_DIM))
                                    .child(format!("продаёт {}", listing.seller)),
                            )
                            .child(whisper)
                            .child(card_button("Открыть на сайте", TEXT).on_mouse_down(
                                MouseButton::Left,
                                move |_event: &MouseDownEvent, _window, cx: &mut App| {
                                    cx.open_url(&trade_url);
                                },
                            )),
                    )
                    .into_any_element()
            }
            LiveCard::Ended { label, reason } => card_frame(LIVE_CARD_HEIGHT, TEXT_WARNING)
                .child(header(
                    format!("Слежение остановлено · {label}"),
                    TEXT_WARNING,
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(TEXT_DIM))
                        .child(format!("Причина: {reason}.")),
                )
                .into_any_element(),
        }
    }
}

/// A card's box, `height` tall at 100 % scale, edged in `border`.
fn card_frame(height: f32, border: u32) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .h(rems_from_px(height))
        .overflow_hidden()
        .gap(rems_from_px(4.))
        .px(rems_from_px(10.))
        .py(rems_from_px(7.))
        .rounded_xs()
        .bg(rgb(BG_NAMEPLATE))
        .border_1()
        .border_color(rgb(border))
}

/// A price: its amount, and its currency's icon -- or name, without one.
fn price_tag(amount: f64, currency: &str, icon: Option<String>) -> impl IntoElement {
    let name = icon.is_none().then(|| currency.to_owned());
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(rems_from_px(4.))
        .text_color(rgb(TEXT))
        .font_weight(FontWeight::SEMIBOLD)
        .child(format_ru(amount))
        .children(currency_img(icon.as_deref(), 18.))
        .children(name)
}

/// A card's ×, which takes card `id` away.
fn close_button(id: u64, cx: &Context<TradeOverlay>) -> impl IntoElement {
    div()
        .w(rems_from_px(20.))
        .h(rems_from_px(20.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_xs()
        .text_color(rgb(TEXT_MUTED))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(BG_CLOSE_HOVER)).text_color(rgb(TEXT)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                view.dismiss(id, cx);
            }),
        )
        .child("×")
}

/// A card's button, its label in `color`.
fn card_button(label: &'static str, color: u32) -> gpui::Div {
    div()
        .h(rems_from_px(24.))
        .px(rems_from_px(7.))
        .flex()
        .flex_none()
        .items_center()
        .rounded_xs()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(BG_CONTROL))
        .text_xs()
        .text_color(rgb(color))
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(rgb(BG_BUTTON_HOVER))
                .border_color(rgb(BORDER_GOLD))
        })
        .child(label)
}

impl Render for TradeOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_overlay(window, cx);
        window.set_rem_size(px(BASE_REM_SIZE * self.options.ui_scale));
        let app = self.app.upgrade();
        let icon = |currency: &str| {
            let app = app.as_ref()?;
            app.read(cx).currency_icon(currency).map(str::to_owned)
        };
        let icons: Vec<Option<String>> = self
            .live
            .iter()
            .map(|shown| match &shown.card {
                LiveCard::Listing(LiveListing {
                    price: Some((_, currency)),
                    ..
                }) => icon(currency),
                _ => None,
            })
            .collect();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(rems_from_px(GAP))
            .p(rems_from_px(PADDING))
            .bg(rgb(BG_PANEL))
            .border_1()
            .border_color(rgb(BORDER_GOLD))
            .text_sm()
            .text_color(rgb(TEXT))
            .children(
                self.live
                    .iter()
                    .zip(icons)
                    .map(|(shown, icon)| self.render_live_card(shown, icon, cx)),
            )
    }
}
