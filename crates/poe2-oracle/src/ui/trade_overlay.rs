//! The trade overlay: buyers' requests (`crate::trade_requests`) as cards at the top of the game,
//! newest first, each with buttons that answer in the game's chat -- invite, trade, a word, kick.
//! PoE Trades Companion's feature, which no price checker has. Each button sends one message: one
//! press, one action, the rule GGG holds macros to. Below them, live search's cards
//! (`crate::live_search`): each new listing of a watched search -- its whisper to copy, the search
//! to open on the trade site -- and a watch the site ended.
//!
//! A task reads `Client.txt` every second through its own `platform::client_log::ClientLog`, opened
//! at the log's end so that old whispers never come back; with requests turned off it reads
//! nothing, and opens the log afresh when they are turned on again. The window is interactive and
//! shown only while it has cards, the game (or this app) is in front and the price panel is
//! closed. A click never activates it (`Win32Overlay::set_no_activate`): the game keeps the
//! keyboard, and a reply types straight into it through `game_chat`.

use std::time::Duration;

use async_channel::Receiver;
use gpui::{
    App, AsyncApp, Bounds, ClipboardItem, Context, Entity, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, Render, WeakEntity, Window, WindowBounds, WindowKind, WindowOptions, div,
    point, prelude::*, px, rgb, size,
};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;

use crate::game_chat;
use crate::live_search::{LiveCard, LiveListing, SHOWN_LISTINGS};
use crate::overlay_layout::PhysicalRect;
use crate::platform::client_log::ClientLog;
use crate::platform::game_window::{self, Foreground};
use crate::platform::win32::Win32Overlay;
use crate::price_check::PriceCheckApp;
use crate::settings::Settings;
use crate::trade_requests::{
    ChatEvent, RequestLanguage, TradeRequest, parse_chat_line, trade_request,
};
use crate::ui::hint::hint;
use crate::ui::panel::format::{currency_img, format_ru};
use crate::ui::theme::{
    BASE_REM_SIZE, BG_BUTTON_HOVER, BG_CLOSE_HOVER, BG_CONTROL, BG_NAMEPLATE, BG_PANEL, BORDER,
    BORDER_GOLD, GOLD, PRICE_RISE, TEXT, TEXT_DIM, TEXT_MUTED, TEXT_WARNING, rems_from_px,
};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// The window's logical width, a request card's and a live search card's height, the gap between
/// cards and the frame's padding, all at 100 % scale.
const WIDTH: f32 = 450.;
const CARD_HEIGHT: f32 = 116.;
const LIVE_CARD_HEIGHT: f32 = 92.;
const GAP: f32 = 6.;
const PADDING: f32 = 6.;
/// Cards kept: the oldest goes when a new request comes.
const MAX_CARDS: usize = 4;
/// Where the window's top sits below the game's, as a share of the game's height: under the top
/// edge's boss bar and area banner.
const TOP: f64 = 0.1;

/// What the overlay does, from the player's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TradeOverlayOptions {
    /// Buyers' requests are read and shown.
    pub enabled: bool,
    /// A sound for a new request or live search listing.
    pub sound: bool,
    pub ui_scale: f32,
}

impl TradeOverlayOptions {
    pub fn from_settings(settings: &Settings) -> TradeOverlayOptions {
        TradeOverlayOptions {
            enabled: settings.trade_requests,
            sound: settings.trade_sound,
            ui_scale: settings.ui_scale,
        }
    }
}

/// A request on screen.
struct Card {
    id: u64,
    request: TradeRequest,
    /// When it last came, as the log has it: `03:04`.
    time: String,
    /// The buyer joined the party or came to the area.
    arrived: bool,
    /// How many times the buyer asked for it again.
    repeats: u32,
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

/// An answer to a request: one message into the game's chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reply {
    Invite,
    Trade,
    Wait,
    Sold,
    Thanks,
    Kick,
}

const REPLIES: [Reply; 6] = [
    Reply::Invite,
    Reply::Trade,
    Reply::Wait,
    Reply::Sold,
    Reply::Thanks,
    Reply::Kick,
];

impl Reply {
    fn label(self) -> &'static str {
        match self {
            Reply::Invite => "Пригласить",
            Reply::Trade => "Обмен",
            Reply::Wait => "Минуту",
            Reply::Sold => "Продано",
            Reply::Thanks => "Спасибо",
            Reply::Kick => "Выгнать",
        }
    }

    /// The chat line it sends; words to the buyer go in the language their request came in.
    fn text(self, request: &TradeRequest) -> String {
        let buyer = &request.buyer;
        let russian = request.language == RequestLanguage::Russian;
        let say = |ru: &str, en: &str| format!("@{buyer} {}", if russian { ru } else { en });
        match self {
            Reply::Invite => format!("/invite {buyer}"),
            Reply::Trade => format!("/tradewith {buyer}"),
            Reply::Kick => format!("/kick {buyer}"),
            Reply::Wait => say("минуту, пожалуйста", "one moment please"),
            Reply::Sold => say("извините, уже продано", "sorry, it's already sold"),
            Reply::Thanks => say("спасибо, удачи!", "thanks, good luck!"),
        }
    }

    /// Whether the request is done with once this is sent.
    fn closes(self) -> bool {
        matches!(self, Reply::Sold | Reply::Kick)
    }
}

/// The overlay window's root view.
pub struct TradeOverlay {
    cards: Vec<Card>,
    /// Live search's cards, newest first, below the requests.
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

/// Opens the overlay window -- hidden until a card comes -- and starts reading the log and
/// `live_cards`. The window owns the returned view; keep the handle only to call
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
            cards: Vec::new(),
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
            size(px(WIDTH), px(CARD_HEIGHT)),
        ))),
        titlebar: None,
        kind: WindowKind::PopUp,
        is_movable: false,
        focus: false,
        show: false,
        ..Default::default()
    }
}

/// A log line's event with its time of day to the minute: `2026/09/23 03:04:53 ...` -> `03:04`.
fn read_line(line: &str) -> Option<(String, ChatEvent)> {
    let event = parse_chat_line(line)?;
    Some((line.get(11..16).unwrap_or_default().to_owned(), event))
}

/// The local time of day to the minute, as the log's lines have it: `03:04`.
fn local_time() -> String {
    let now = unsafe { GetLocalTime() };
    format!("{:02}:{:02}", now.wHour, now.wMinute)
}

/// The new-request sound. Best effort: no sound is no reason to fail.
fn chime() {
    let _ = unsafe { MessageBeep(MB_ICONASTERISK) };
}

/// Feeds the overlay until its window closes.
async fn poll_forever(view: WeakEntity<TradeOverlay>, cx: &mut AsyncApp) {
    let mut log: Option<ClientLog> = None;
    loop {
        cx.background_executor().timer(POLL_INTERVAL).await;
        let Ok(enabled) = view.read_with(cx, |view, _| view.options.enabled) else {
            return;
        };
        let (lines, still_open) = cx
            .background_executor()
            .spawn(async move {
                // Requests off: nothing is read, and the log is opened afresh -- at its end --
                // once they are on again, so whispers from meanwhile never show.
                let mut log = log.filter(|_| enabled);
                // Retried every second until the game runs: the log is found through its process.
                if enabled && log.is_none() {
                    log = ClientLog::open(0, read_line).map(|(opened, _)| opened);
                }
                let lines = log
                    .as_mut()
                    .map(|log| log.poll(read_line))
                    .unwrap_or_default();
                (lines, log)
            })
            .await;
        log = still_open;
        let in_front = game_window::foreground() != Foreground::Other;
        let Some(view) = view.upgrade() else {
            return;
        };
        view.update(cx, |view, cx| view.take(lines, in_front, cx));
    }
}

impl TradeOverlay {
    /// Hides the overlay while `suppressed` is true -- pass whether the price-check window is
    /// shown, whenever that changes: one would cover the other.
    pub fn set_suppressed(&mut self, suppressed: bool, cx: &mut Context<Self>) {
        self.suppressed = suppressed;
        self.sync_window(cx);
    }

    /// Takes over the player's saved options. With requests turned off, the overlay drops their
    /// cards and takes no new ones; live search's stay.
    pub fn set_options(&mut self, options: TradeOverlayOptions, cx: &mut Context<Self>) {
        if options == self.options {
            return;
        }
        self.options = options;
        if !options.enabled {
            self.cards.clear();
        }
        cx.notify();
        self.sync_window(cx);
    }

    /// Takes in what the log said since the last poll, and whether the game or this app is in
    /// front.
    fn take(&mut self, lines: Vec<(String, ChatEvent)>, in_front: bool, cx: &mut Context<Self>) {
        let mut changed = in_front != self.in_front;
        self.in_front = in_front;
        let mut fresh = false;
        for (time, event) in lines {
            match event {
                ChatEvent::Whisper { from, body } => {
                    if !self.options.enabled {
                        continue;
                    }
                    let Some(request) = trade_request(&from, &body) else {
                        continue;
                    };
                    changed = true;
                    fresh |= self.add(request, time);
                }
                ChatEvent::Joined(name) => changed |= self.mark_arrived(&name, true),
                ChatEvent::Left(name) => changed |= self.mark_arrived(&name, false),
            }
        }
        if fresh && self.options.sound {
            chime();
        }
        if changed {
            cx.notify();
            self.sync_window(cx);
        }
    }

    /// Puts `request` on top. Asked again for the same item, the buyer's card comes back to the
    /// top, counted, rather than twice. Whether the request is a new one.
    fn add(&mut self, request: TradeRequest, time: String) -> bool {
        if let Some(at) = self.cards.iter().position(|card| {
            card.request.buyer == request.buyer && card.request.item == request.item
        }) {
            let mut card = self.cards.remove(at);
            card.repeats += 1;
            card.time = time;
            card.request = request;
            self.cards.insert(0, card);
            log::info!("trade request repeated ({} shown)", self.cards.len());
            return false;
        }
        self.next_id += 1;
        self.cards.insert(
            0,
            Card {
                id: self.next_id,
                request,
                time,
                arrived: false,
                repeats: 0,
            },
        );
        self.cards.truncate(MAX_CARDS);
        log::info!("trade request ({} shown)", self.cards.len());
        true
    }

    /// Puts live search's `cards` on top of its others, keeping `SHOWN_LISTINGS`, with the
    /// new-request sound.
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
        if self.options.sound {
            chime();
        }
        cx.notify();
        self.sync_window(cx);
    }

    /// Marks the cards of `buyer` as arrived or gone; whether any changed.
    fn mark_arrived(&mut self, buyer: &str, arrived: bool) -> bool {
        let mut changed = false;
        for card in self
            .cards
            .iter_mut()
            .filter(|card| card.request.buyer == buyer && card.arrived != arrived)
        {
            card.arrived = arrived;
            changed = true;
        }
        changed
    }

    fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.cards.retain(|card| card.id != id);
        self.live.retain(|shown| shown.id != id);
        log::info!(
            "overlay card dismissed ({} left)",
            self.cards.len() + self.live.len()
        );
        cx.notify();
        self.sync_window(cx);
    }

    /// Sends `reply` to the buyer of card `id`; a closing one takes the card away once sent.
    fn reply(&mut self, id: u64, reply: Reply, cx: &mut Context<Self>) {
        let Some(card) = self.cards.iter().find(|card| card.id == id) else {
            return;
        };
        let text = reply.text(&card.request);
        log::info!("trade reply: {reply:?}");
        cx.spawn(async move |view, cx| {
            let sent = game_chat::send_chat(text, cx).await;
            if sent && reply.closes() {
                view.update(cx, |view, cx| view.dismiss(id, cx)).ok();
            }
        })
        .detach();
    }

    /// Searches the open stash for the item of card `id` -- by the name the buyer's site gave it,
    /// which the game finds when the site is in the client's language -- so the seller sees where
    /// it lies.
    fn find_in_stash(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(card) = self.cards.iter().find(|card| card.id == id) else {
            return;
        };
        let text = card.request.item.clone();
        log::info!("trade request: finding the item in the stash");
        cx.spawn(async move |_, cx| {
            game_chat::search_stash(text, cx).await;
        })
        .detach();
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
        let requests = self.cards.len() as f64;
        let live = self.live.len() as f64;
        let (shown, cards_height) = if requests + live == 0. {
            (1., f64::from(CARD_HEIGHT))
        } else {
            (
                requests + live,
                requests * f64::from(CARD_HEIGHT) + live * f64::from(LIVE_CARD_HEIGHT),
            )
        };
        let height = cards_height + (shown - 1.) * f64::from(GAP) + 2. * f64::from(PADDING);
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

    /// Brings the platform window in line with the cards. Called from the reading tasks,
    /// `set_suppressed` and `set_options`, not only from `render`: a hidden GPUI window is never
    /// redrawn, so a render-only sync could never show it again.
    fn sync_window(&mut self, cx: &mut Context<Self>) {
        let Some(overlay) = self.overlay else {
            return;
        };
        let placement = self.placement();
        // Requests' cards exist only while they are enabled (`set_options`).
        let want_shown = !(self.cards.is_empty() && self.live.is_empty())
            && !self.suppressed
            && self.in_front
            && placement.is_some();
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

    fn render_card(
        &self,
        card: &Card,
        icon: Option<String>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let request = &card.request;
        let id = card.id;
        let header = div()
            .flex()
            .items_center()
            .gap(rems_from_px(6.))
            .child(
                div()
                    .text_color(rgb(GOLD))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("✉ {}", request.buyer)),
            )
            .when(card.repeats > 0, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(rgb(TEXT_WARNING))
                        .child(format!("пишет {}-й раз", card.repeats + 1)),
                )
            })
            .when(card.arrived, |this| {
                this.child(div().text_xs().text_color(rgb(PRICE_RISE)).child("пришёл"))
            })
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(TEXT_DIM))
                    .child(card.time.clone()),
            )
            .child(close_button(id, cx));
        let price = request
            .price
            .as_ref()
            .map(|(amount, currency)| price_tag(*amount, currency, icon));
        let mut place = match &request.stash {
            Some(spot) => format!(
                "вкладка «{}» · {} столбец, {} ряд",
                spot.tab, spot.left, spot.top
            ),
            None => format!("лига {}", request.league),
        };
        if let Some(note) = &request.note {
            place += &format!(" · «{note}»");
        }
        card_frame(
            CARD_HEIGHT,
            if card.arrived {
                PRICE_RISE
            } else {
                BORDER_GOLD
            },
        )
        .child(header)
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
                        .child(request.item.clone()),
                )
                .children(price),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(rems_from_px(6.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(rgb(if request.note.is_some() {
                            TEXT_WARNING
                        } else {
                            TEXT_DIM
                        }))
                        .truncate()
                        .child(place),
                )
                .child(find_button(id, cx)),
        )
        .child(
            div()
                .flex()
                .gap(rems_from_px(4.))
                .children(REPLIES.map(|reply| reply_button(id, reply, cx))),
        )
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

fn reply_button(id: u64, reply: Reply, cx: &Context<TradeOverlay>) -> impl IntoElement {
    card_button(reply.label(), if reply.closes() { TEXT_DIM } else { TEXT }).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
            view.reply(id, reply, cx);
        }),
    )
}

/// "Найти": searches the stash for the card's item (`TradeOverlay::find_in_stash`).
fn find_button(id: u64, cx: &Context<TradeOverlay>) -> impl IntoElement {
    div()
        .id(("find", id))
        .flex_none()
        .px(rems_from_px(6.))
        .rounded_xs()
        .text_xs()
        .text_color(rgb(GOLD))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(BG_BUTTON_HOVER)))
        .tooltip(hint(
            "Вставить название вещи в поиск открытого тайника: в игре подсветится, где она лежит",
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _event: &MouseDownEvent, _window, cx| {
                view.find_in_stash(id, cx);
            }),
        )
        .child("Найти")
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
            .cards
            .iter()
            .map(|card| {
                let (_, currency) = card.request.price.as_ref()?;
                icon(currency)
            })
            .collect();
        let live_icons: Vec<Option<String>> = self
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
                self.cards
                    .iter()
                    .zip(icons)
                    .map(|(card, icon)| self.render_card(card, icon, cx)),
            )
            .children(
                self.live
                    .iter()
                    .zip(live_icons)
                    .map(|(shown, icon)| self.render_live_card(shown, icon, cx)),
            )
    }
}
