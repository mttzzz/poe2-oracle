//! The onboarding tour, drawn: `crate::tour`'s course as a spotlight over the app's own windows.
//! At each stop the window it points into -- the settings window, the price panel, or, over the
//! game, the tour's own window -- dims all but a hole around the stop's target, edges the hole in
//! gold, and sets the stop's card beside it in the game's frame: which of the four steps it is,
//! what to look at, and Back, Next and «Пропустить обучение». The check's stop points at nothing
//! and has no Next: its card waits over the game for the player's first price check. What the
//! player does in the app moves the tour on by itself -- picking the league, the first check, a
//! search, closing the panel (`tour::Watched`).
//!
//! It starts by itself at launch until the player finishes or skips it (`Settings::tour_done`),
//! and again from «Помощь». The windows it points into only mark their targets ([`spot`]) and lay
//! the spotlight over themselves ([`layer`]); the tour opens and closes them as its stops need: the
//! settings window for the league, the settings window closed for the check (its hotkey waits
//! while the window is open), the panel shown for the panel's parts and hidden for the XP line,
//! which the panel hides.
//!
//! GPUI clips nothing out, so the dim is four strips around the hole. A target's place is known
//! only once its window has laid it out: each frame keeps where the targets and the card were
//! drawn, the next frame places the hole and the card by that, and a frame that finds them moved
//! asks for one more. The card rises in over `style::APPEAR` at each stop and the dim fades in
//! with the first; nothing else moves, so a tour left alone draws nothing.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Bounds, Context, Entity, Global, MouseDownEvent,
    Pixels, Render, SharedString, Size, Subscription, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, canvas, div, point, prelude::*, px,
    relative, rgb, size,
};

use crate::overlay_layout::{PhysicalRect, hud_rails};
use crate::platform::game_window::GameScreen;
use crate::platform::win32::Win32Overlay;
use crate::price_check::{BootstrapState, PriceCheckApp, SearchState};
use crate::settings::{self, Hotkey};
use crate::tour::{self, Action, Area, Host, Outcome, STEPS, Stop, Tour, Watched};
use crate::tr;
use crate::ui::fonts;
use crate::ui::settings_view::SettingsView;
use crate::ui::style::{
    APPEAR, ButtonKind, SCRIM_OPACITY, alpha, appear, diamond, ease, game_frame, heading, keycaps,
    link, modal_shadow, small_button,
};
use crate::ui::theme::{
    BASE_REM_SIZE, BG_CARD, BORDER_GOLD, GOLD, GOLD_LIGHT, TEXT, TEXT_DIM, TEXT_MUTED, rems_from_px,
};
use crate::ui::xp_overlay::XpLineOnScreen;

/// The card's width at 100 % scale, px.
const CARD_WIDTH: f32 = 360.;
/// The card's height before it was first drawn: about its usual height, so the first frame puts
/// it near its place.
const CARD_HEIGHT_GUESS: f32 = 200.;
/// How far the hole reaches past its target, and the gap between the hole, the card and the
/// window's edges, px at 100 % scale.
const HOLE_MARGIN: f32 = 6.;
const CARD_GAP: f32 = 12.;
/// The hole's corners, and how far outside its gold line the fainter second one runs, px.
const HOLE_RADIUS: f32 = 4.;
const HOLE_HALO: f32 = 3.;
/// The check's card over the game: its top this share of the game's height down, clear of the
/// inventory and the stash at the sides.
const CHECK_CARD_TOP: f32 = 0.12;

/// The running tour, for the windows that draw it.
struct Running(Entity<Guide>);

impl Global for Running {}

fn running(cx: &App) -> Option<Entity<Guide>> {
    cx.try_global::<Running>()
        .map(|Running(guide)| guide.clone())
}

/// Whether the tour stands at the XP line. The XP overlay stays on screen then, although the
/// tour's dim covers the bar the line reads and the bar goes unreadable: otherwise the line would
/// hide under its own spotlight.
pub fn holds_xp_line(cx: &App) -> bool {
    running(cx).is_some_and(|guide| {
        let tour = &guide.read(cx).tour;
        tour.ended().is_none() && tour.stop() == Stop::XpLine
    })
}

/// The running tour: where its course stands, what the app last showed, the tour's own window,
/// and where the stops' targets and the card were last drawn.
struct Guide {
    app: Entity<PriceCheckApp>,
    tour: Tour,
    seen: Watched,
    /// What the cards say of the app, kept as it changes: a card over the panel can't read the
    /// app, which is the panel's view and busy drawing it. The hotkey and whether the XP overlay
    /// is on, from the settings; whether the panel prices its item by the exchange.
    hotkey: Hotkey,
    xp_overlay: bool,
    market: bool,
    screen: Option<WindowHandle<Screen>>,
    /// Where each stop's target was drawn in its window's last frame ([`spot`]) -- `None` for one
    /// drawn but scrolled out of sight. Written while the windows draw, so behind a `RefCell`: no
    /// entity update, no effect, mid-frame.
    spots: RefCell<HashMap<Stop, Option<Bounds<Pixels>>>>,
    /// The card's size in each host's last frame.
    cards: RefCell<HashMap<Host, Size<Pixels>>>,
    _watching: [Subscription; 2],
}

/// Starts the tour from its first stop -- over again if one is under way. Its first stop opens
/// the settings window at «Общие», so the settings' own «Пройти заново» should show «Общие» first.
pub fn start(app: &Entity<PriceCheckApp>, cx: &mut App) {
    if let Some(guide) = running(cx).filter(|guide| guide.read(cx).tour.ended().is_none()) {
        guide.update(cx, |guide, cx| guide.restart(cx));
        return;
    }
    log::info!("tour started");
    let guide = cx.new(|cx| Guide::new(app.clone(), cx));
    cx.set_global(Running(guide.clone()));
    guide.update(cx, |guide, cx| guide.arrive(None, cx));
}

/// The tour's spotlight over `host`'s window while the tour stands at one of its stops; `None`
/// otherwise. Lay it last in the window's root, which must be `relative`.
pub fn layer(host: Host, window: &Window, cx: &App) -> Option<AnyElement> {
    let guide = running(cx)?;
    let g = guide.read(cx);
    let stop = g.tour.stop();
    if g.tour.ended().is_some() || stop.host() != host || host == Host::Screen {
        return None;
    }
    let unit = rem_unit(window);
    let view = view_area(window);
    // Taken, not read: the target marks itself again as this frame draws, and `settle` sees
    // whether it's still where the hole is placed.
    let target = g.spots.borrow_mut().remove(&stop);
    let placement = Placement {
        host,
        view,
        hole: target
            .flatten()
            .and_then(|target| area(target).grown(HOLE_MARGIN * unit).within(view)),
        scrolled_out: target == Some(None),
        card: g.cards.borrow().get(&host).copied(),
        unit,
    };
    Some(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(spotlight(&guide, g, &placement, cx))
            .child(settle(host, Some((stop, target)), placement.card))
            .into_any_element(),
    )
}

/// `target`, what `stop` points at, marking where it's drawn for the tour's spotlight. It sits in
/// a flex column of its own, so it lays out as it would alone.
pub fn spot(stop: Stop, target: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .on_children_prepainted(move |bounds, window, cx| {
            // A target with nothing in it isn't there: no results yet, say.
            let (Some(guide), Some(drawn)) = (
                running(cx),
                bounds.first().filter(|drawn| !drawn.is_empty()),
            ) else {
                return;
            };
            // Only what shows: a target scrolled out of the panel gets no hole.
            let shown = drawn.intersect(&window.content_mask().bounds);
            guide
                .read(cx)
                .spots
                .borrow_mut()
                .insert(stop, (!shown.is_empty()).then_some(shown));
        })
        .child(target)
}

impl Guide {
    fn new(app: Entity<PriceCheckApp>, cx: &mut Context<Guide>) -> Guide {
        let watching = [
            cx.observe(&app, |guide, app, cx| guide.app_changed(&app, cx)),
            cx.observe_global::<XpLineOnScreen>(|_, cx| {
                let guide = cx.entity();
                cx.defer(move |cx| place_screen(&guide, cx));
            }),
        ];
        let state = app.read(cx);
        let (seen, hotkey, xp_overlay, market) = (
            watched(state),
            state.settings.hotkey,
            state.settings.xp_overlay,
            state.priced_by_market,
        );
        Guide {
            app,
            tour: Tour::new(),
            seen,
            hotkey,
            xp_overlay,
            market,
            screen: None,
            spots: RefCell::default(),
            cards: RefCell::default(),
            _watching: watching,
        }
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        let from = self.tour.stop();
        self.tour = Tour::new();
        log::info!("tour started over");
        self.arrive(Some(from), cx);
    }

    fn next(&mut self, cx: &mut Context<Self>) {
        self.go(cx, |tour, parts| tour.next(|stop| parts.show(stop)));
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        self.go(cx, |tour, parts| tour.back(|stop| parts.show(stop)));
    }

    fn skip(&mut self, cx: &mut Context<Self>) {
        self.go(cx, |tour, _| tour.skip());
    }

    /// Moves the course by `step`, which sees what the panel shows, and follows it.
    fn go(&mut self, cx: &mut Context<Self>, step: impl FnOnce(&mut Tour, PanelParts)) {
        if self.tour.ended().is_some() {
            return;
        }
        let from = self.tour.stop();
        step(&mut self.tour, PanelParts::of(self.app.read(cx)));
        self.follow(from, cx);
    }

    /// What the player just did in the app, moving the tour on where a stop asks for it. The
    /// settings window closed at one of the panel's stops -- opened from the tray, say -- brings
    /// back the panel that stepped aside for it (`app::open_settings`), and the stop with it.
    fn app_changed(&mut self, app: &Entity<PriceCheckApp>, cx: &mut Context<Self>) {
        if self.tour.ended().is_some() {
            return;
        }
        let state = app.read(cx);
        let (now, parts) = (watched(state), PanelParts::of(state));
        self.hotkey = state.settings.hotkey;
        self.xp_overlay = state.settings.xp_overlay;
        self.market = state.priced_by_market;
        if now == self.seen {
            return;
        }
        let actions = self.seen.actions_to(&now);
        self.seen = now;
        let from = self.tour.stop();
        let settings_closed = actions.contains(&Action::SettingsClosed);
        for action in actions {
            self.tour.act(action, |stop| parts.show(stop));
        }
        if settings_closed && self.tour.ended().is_none() && self.tour.stop().host() == Host::Panel
        {
            let app = self.app.clone();
            cx.defer(move |cx| set_panel_shown(&app, true, cx));
        }
        self.follow(from, cx);
    }

    fn follow(&mut self, from: Stop, cx: &mut Context<Self>) {
        match self.tour.ended() {
            Some(outcome) => self.end(outcome, cx),
            None if self.tour.stop() != from => self.arrive(Some(from), cx),
            None => {}
        }
    }

    /// Brings the windows in line with the stop just reached, `from` the one before: the settings
    /// window open for the league and closed for the check, the panel shown again for its parts
    /// on the way back from the XP line and hidden for the line -- then every host draws the stop.
    fn arrive(&mut self, from: Option<Stop>, cx: &mut Context<Self>) {
        let stop = self.tour.stop();
        log::info!("tour: {stop:?}");
        let (app, guide) = (self.app.clone(), cx.entity());
        // Deferred: this runs inside updates -- of the view whose button was pressed, or of the
        // app, whose observer saw the player act -- and it opens, closes and updates windows.
        cx.defer(move |cx| {
            match stop {
                Stop::League => crate::app::open_settings(&app, cx),
                Stop::PriceCheck => close_settings(&app, cx),
                Stop::XpLine => set_panel_shown(&app, false, cx),
                Stop::Filters | Stop::Search | Stop::Listings | Stop::PanelLeague => {
                    if from == Some(Stop::XpLine) {
                        set_panel_shown(&app, true, cx);
                    }
                }
            }
            // The settings window draws what the app says, so it follows the app too.
            app.update(cx, |_, cx| cx.notify());
            // Once what that set off has run: a panel hidden for the XP line lets the line show
            // first, and the tour's window finds it.
            cx.defer(move |cx| place_screen(&guide, cx));
        });
    }

    /// Ends the tour: its own window closes, the hosts drop the spotlight, and the settings
    /// remember it's done, so it never starts by itself again.
    fn end(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        log::info!("tour ended: {outcome:?}");
        let (app, guide, screen) = (self.app.clone(), cx.entity(), self.screen.take());
        cx.defer(move |cx| {
            if let Some(screen) = screen {
                screen
                    .update(cx, |_, window, cx| crate::app::close_window(window, cx))
                    .ok();
            }
            // Unless a tour started over since, from «Помощь».
            if running(cx).is_some_and(|current| current == guide) {
                cx.remove_global::<Running>();
            }
            app.update(cx, |state, cx| {
                if !state.settings.tour_done {
                    state.settings.tour_done = true;
                    state.save_settings(cx);
                }
                cx.notify();
            });
        });
    }

    /// Where the tour's own window goes for the stop: the check's card over the top of the game;
    /// the XP line's spotlight over the game, or its card above the flask panel's rail while the
    /// line isn't on screen -- nowhere for a stop another window draws.
    fn screen_layout(&self, cx: &App) -> Option<Layout> {
        let stop = self.tour.stop();
        if self.tour.ended().is_some() || stop.host() != Host::Screen {
            return None;
        }
        let GameScreen {
            game, dpi_scale, ..
        } = GameScreen::at_cursor()?;
        let scale = dpi_scale as f32 * self.app.read(cx).settings.ui_scale;
        let centre_x = game.x + game.width / 2;
        let at = |share: f32| game.y + (game.height as f32 * share).round() as i32;
        Some(match stop {
            Stop::XpLine => match cx.try_global::<XpLineOnScreen>().and_then(|line| line.0) {
                Some(line) => Layout::Spotlight { area: game, line },
                // Just above where the line would be: the flask panel's rail.
                None => {
                    let plate = hud_rails(game).flask;
                    Layout::Card {
                        centre_x: plate.x + plate.width / 2,
                        y: plate.y - (CARD_GAP * scale).round() as i32,
                        rises: true,
                        scale,
                    }
                }
            },
            _ => Layout::Card {
                centre_x,
                y: at(CHECK_CARD_TOP),
                rises: false,
                scale,
            },
        })
    }
}

/// What the tour watches of `app`.
fn watched(app: &PriceCheckApp) -> Watched {
    Watched {
        settings_open: app.settings_window().is_some(),
        panel_shown: app.visible,
        checks: app.appearances,
        league: app.settings.league.clone(),
        searching: matches!(
            app.search,
            SearchState::Searching | SearchState::RateLimiting { .. }
        ),
    }
}

/// Which of the panel's parts it shows now, for the stops that point at them: its title bar once
/// the catalogs are in, the listings for an item, and the filters and Search for an item searched
/// by them rather than priced by the exchange.
#[derive(Clone, Copy)]
struct PanelParts {
    title_bar: bool,
    listings: bool,
    search: bool,
    filters: bool,
}

impl PanelParts {
    fn of(app: &PriceCheckApp) -> PanelParts {
        let ready = matches!(app.bootstrap, BootstrapState::Ready);
        let item = ready && app.problem.is_none() && app.item.is_some();
        let search = item && !app.priced_by_market;
        PanelParts {
            title_bar: ready,
            listings: item,
            search,
            filters: search && !app.filters.is_empty(),
        }
    }

    fn show(self, stop: Stop) -> bool {
        match stop {
            Stop::Filters => self.filters,
            Stop::Search => self.search,
            Stop::Listings => self.listings,
            Stop::PanelLeague => self.title_bar,
            Stop::League | Stop::PriceCheck | Stop::XpLine => true,
        }
    }
}

fn set_panel_shown(app: &Entity<PriceCheckApp>, shown: bool, cx: &mut App) {
    app.update(cx, |state, cx| {
        if state.visible != shown {
            state.visible = shown;
            cx.notify();
        }
    });
}

/// Closes the settings window, as its × would.
fn close_settings(app: &Entity<PriceCheckApp>, cx: &mut App) {
    let Some(settings) = app
        .read(cx)
        .settings_window()
        .and_then(|handle| handle.downcast::<SettingsView>())
    else {
        return;
    };
    if let Err(err) = settings.update(cx, |view, window, cx| view.close(window, cx)) {
        log::warn!("closing the settings window for the tour failed: {err:#}");
    }
}

/// Where a spotlight goes in its window.
struct Placement {
    host: Host,
    /// The window's whole area.
    view: Area,
    /// Around the target; `None` while the target isn't drawn or is out of sight.
    hole: Option<Area>,
    /// The target is drawn, but scrolled out of sight.
    scrolled_out: bool,
    /// The card's size in the last frame.
    card: Option<Size<Pixels>>,
    /// The window's pixels per pixel at 100 % scale.
    unit: f32,
}

/// A spotlight over a window: its area dimmed but for the hole (all of it when there's none),
/// the hole edged in gold, and the stop's card beside the hole -- in the middle without one.
fn spotlight(
    guide: &Entity<Guide>,
    g: &Guide,
    placement: &Placement,
    cx: &App,
) -> impl IntoElement {
    let &Placement {
        host,
        view,
        hole,
        scrolled_out,
        card,
        unit,
    } = placement;
    let card_size = card.map_or((CARD_WIDTH * unit, CARD_HEIGHT_GUESS * unit), |size| {
        (f32::from(size.width), f32::from(size.height))
    });
    let (card_x, card_y) = tour::card_origin(view, hole, card_size, CARD_GAP * unit);
    let strips = match hole {
        Some(hole) => tour::dim_around(view, hole).to_vec(),
        None => vec![view],
    };
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .children(
                    strips
                        .into_iter()
                        .filter(|strip| strip.width > 0. && strip.height > 0.)
                        .map(dim_strip),
                )
                .children(hole.map(hole_edge))
                // Keyed once per window: the dim stays put from stop to stop.
                .with_animation(
                    "tour-dim",
                    Animation::new(APPEAR).with_easing(ease),
                    |dim, t| dim.opacity(t),
                ),
        )
        .child(
            div()
                .absolute()
                .left(px(card_x))
                .top(px(card_y))
                .child(measured_card(host, stop_card(guide, g, scrolled_out, cx))),
        )
}

/// Asks for one more frame when this frame drew the target or the card elsewhere than the
/// spotlight was placed by: `placed`, a stop and where its target was (for a host whose targets
/// mark themselves, as `Guide::spots` keeps it), and the card's size, `card`. Laid after the
/// spotlight, so everything it compares has been drawn by then.
fn settle(
    host: Host,
    placed: Option<(Stop, Option<Option<Bounds<Pixels>>>)>,
    card: Option<Size<Pixels>>,
) -> impl IntoElement {
    canvas(
        move |_, window, cx| {
            let Some(guide) = running(cx) else {
                return;
            };
            let g = guide.read(cx);
            let target_moved =
                placed.is_some_and(|(stop, at)| g.spots.borrow().get(&stop).copied() != at);
            let card_resized = g.cards.borrow().get(&host).copied() != card;
            if target_moved || card_resized {
                window.on_next_frame(|window, _| window.refresh());
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .size_0()
}

/// `card`, its size kept for the next frame's placement.
fn measured_card(host: Host, card: impl IntoElement) -> impl IntoElement {
    div()
        .on_children_prepainted(move |bounds, _, cx| {
            if let (Some(card), Some(guide)) = (bounds.first(), running(cx)) {
                guide.read(cx).cards.borrow_mut().insert(host, card.size);
            }
        })
        .child(card)
}

fn dim_strip(strip: Area) -> impl IntoElement {
    div()
        .absolute()
        .left(px(strip.x))
        .top(px(strip.y))
        .w(px(strip.width))
        .h(px(strip.height))
        .bg(alpha(0x000000, SCRIM_OPACITY))
        // The dimmed part is out of play, the wheel excepted: a panel scrolls under it to bring
        // the target in.
        .block_mouse_except_scroll()
}

/// The hole's edge: a gold line with a fainter one just outside it -- the game frame's double
/// line turned outwards, so neither lies over the target.
fn hole_edge(hole: Area) -> impl IntoElement {
    div()
        .absolute()
        .left(px(hole.x))
        .top(px(hole.y))
        .w(px(hole.width))
        .h(px(hole.height))
        .rounded(px(HOLE_RADIUS))
        .border_1()
        .border_color(rgb(GOLD))
        .child(
            div()
                .absolute()
                .top(px(-HOLE_HALO))
                .left(px(-HOLE_HALO))
                .right(px(-HOLE_HALO))
                .bottom(px(-HOLE_HALO))
                .rounded(px(HOLE_RADIUS + HOLE_HALO))
                .border_1()
                .border_color(alpha(GOLD, 0.3)),
        )
}

/// What a stop's card says: its title, its text, and a note under the text.
struct Words {
    title: &'static str,
    text: SharedString,
    note: Option<&'static str>,
}

fn words(stop: Stop, g: &Guide, cx: &App) -> Words {
    let plain = |title, text: &'static str| Words {
        title,
        text: text.into(),
        note: None,
    };
    match stop {
        Stop::League => plain(
            tr!("Welcome to PoE2 Oracle"),
            tr!(
                "PoE2 Oracle runs in the background: its icon sits by the clock, sometimes under \
                 the “Show hidden icons” arrow. These settings open from its menu and from the ⚙ \
                 on the price panel. Start with your league — “Auto” follows the current one."
            ),
        ),
        Stop::PriceCheck => Words {
            title: tr!("Check a price"),
            text: tr!(
                "Hover over an item in the game and press {hotkey}. The price panel opens next \
                 to your inventory, and the tour carries on there.",
                hotkey = g.hotkey
            )
            .into(),
            note: None,
        },
        Stop::Filters => plain(
            tr!("Filters"),
            tr!(
                "Ticked rows are what the trade search looks for, starting from your item's \
                 values. Untick a row that doesn't matter, or change its minimum."
            ),
        ),
        Stop::Search => plain(
            tr!("Search"),
            tr!("After changing the filters, press “Search” to refresh the listings below."),
        ),
        Stop::Listings if g.market => plain(
            tr!("Exchange price"),
            tr!(
                "Currency and other items that trade on the Currency Exchange are priced by its \
                 trades, not by listings."
            ),
        ),
        Stop::Listings => plain(
            tr!("Listings"),
            tr!(
                "The cheapest matching listings. Hover over a row to see the item; click a row \
                 marked ✉ to copy a whisper to the seller."
            ),
        ),
        Stop::PanelLeague => plain(
            tr!("Search league"),
            tr!(
                "The league the search runs in. Click it to switch leagues right on the panel. \
                 Esc or × closes the panel."
            ),
        ),
        Stop::XpLine => Words {
            title: tr!("XP overlay"),
            text: tr!(
                "The line above the flask panel shows how fast you level — percent of a level per \
                 hour — and how much play is left to the next level; its ⚙ opens the settings. \
                 In town, in your hideout and after five minutes without experience it pauses: \
                 it dims and shows how long the pause has lasted. The map timer sits above the \
                 skill panel."
            )
            .into(),
            note: match cx.try_global::<XpLineOnScreen>().and_then(|line| line.0) {
                Some(_) => None,
                None if !g.xp_overlay => Some(tr!(
                    "It's off now: turn it on in the settings, “XP overlay” section."
                )),
                None => Some(tr!("It appears once the experience bar is on screen.")),
            },
        },
    }
}

/// The stop's card in the game's frame, rising in at each stop: the steps as diamonds, the title,
/// the words -- and, when the stop's target is `scrolled_out`, where it went -- and the buttons:
/// «Пропустить обучение», Back but on the first stop, and Next («Готово» on the last) but on the
/// stop that waits, which names the hotkey it waits for instead.
fn stop_card(guide: &Entity<Guide>, g: &Guide, scrolled_out: bool, cx: &App) -> impl IntoElement {
    let stop = g.tour.stop();
    let face = fonts::interface_font();
    let Words { title, text, note } = words(stop, g, cx);
    let note = note.or(scrolled_out
        .then(|| tr!("It's scrolled out of sight: scroll the panel with the mouse wheel.")));
    let guide = guide.downgrade();
    let press = move |go: fn(&mut Guide, &mut Context<Guide>)| {
        let guide = guide.clone();
        move |_: &MouseDownEvent, _: &mut Window, cx: &mut App| {
            guide.update(cx, go).ok();
        }
    };
    let card = div()
        .relative()
        .flex()
        .flex_col()
        .gap(rems_from_px(8.))
        .w(rems_from_px(CARD_WIDTH))
        .px(rems_from_px(18.))
        .pt(rems_from_px(14.))
        .pb(rems_from_px(14.))
        .bg(rgb(BG_CARD))
        .shadow(modal_shadow())
        .text_color(rgb(TEXT))
        .text_size(rems_from_px(13.))
        .line_height(relative(1.45))
        // The card's clicks stay on it: none reaches the dim or the target under it.
        .occlude()
        .child(progress(stop))
        .child(
            heading(face)
                .text_size(rems_from_px(17.))
                .text_color(rgb(GOLD_LIGHT))
                .child(title),
        )
        .child(div().child(text))
        .children(note.map(|note| {
            div()
                .text_size(rems_from_px(12.))
                .text_color(rgb(TEXT_DIM))
                .child(note)
        }))
        .children(stop.waits().then(|| waiting_for(g.hotkey)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(rems_from_px(8.))
                .pt(rems_from_px(4.))
                .child(link("tour-skip", tr!("Skip tour"), press(Guide::skip)))
                .child(div().flex_1())
                .when(!stop.is_first(), |this| {
                    this.child(small_button(
                        "tour-back",
                        tr!("Back"),
                        ButtonKind::Secondary,
                        face,
                        press(Guide::back),
                    ))
                })
                .when(!stop.waits(), |this| {
                    this.child(small_button(
                        "tour-next",
                        if stop.is_last() {
                            tr!("Done")
                        } else {
                            tr!("Next")
                        },
                        ButtonKind::Primary,
                        face,
                        press(Guide::next),
                    ))
                }),
        )
        .child(game_frame());
    appear(("tour-card", stop as usize), card)
}

/// The steps as diamonds -- those done gold, the current one larger and lighter, those to come
/// dull -- and «Шаг N из 4».
fn progress(stop: Stop) -> impl IntoElement {
    let step = stop.step();
    div()
        .flex()
        .items_center()
        .gap(rems_from_px(6.))
        .children((1..=STEPS).map(|each| match each.cmp(&step) {
            Ordering::Less => diamond(6., GOLD),
            Ordering::Equal => diamond(8., GOLD_LIGHT),
            Ordering::Greater => diamond(6., BORDER_GOLD),
        }))
        .child(
            div()
                .pl(rems_from_px(4.))
                .text_size(rems_from_px(11.5))
                .text_color(rgb(TEXT_MUTED))
                .child(tr!("Step {step} of {steps}", step = step, steps = STEPS)),
        )
}

/// What the check's stop waits for: the player's hotkey, as keycaps.
fn waiting_for(hotkey: Hotkey) -> impl IntoElement {
    let keys: Vec<SharedString> = settings::modifier_names(hotkey.ctrl, hotkey.shift, hotkey.alt)
        .map(SharedString::from)
        .chain(std::iter::once(hotkey.key.to_string().into()))
        .collect();
    div()
        .flex()
        .items_center()
        .gap(rems_from_px(8.))
        .text_size(rems_from_px(12.))
        .text_color(rgb(TEXT_DIM))
        .child(tr!("Waiting for"))
        .child(keycaps(&keys))
}

/// Where the tour's own window goes, physical px.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Layout {
    /// The card alone, centred on `centre_x` with its top at `y` -- its bottom, if it `rises`;
    /// until the card has been drawn, sized for it at `scale`, the monitor's DPI scale times the
    /// player's UI scale.
    Card {
        centre_x: i32,
        y: i32,
        rises: bool,
        scale: f32,
    },
    /// Over the game's `area`, dimmed but for a hole around the XP `line`.
    Spotlight {
        area: PhysicalRect,
        line: PhysicalRect,
    },
}

impl Layout {
    /// The window's rect, for a card `card` (width, height) in size as last drawn.
    fn rect(self, card: Option<(i32, i32)>) -> PhysicalRect {
        match self {
            Layout::Spotlight { area, .. } => area,
            Layout::Card {
                centre_x,
                y,
                rises,
                scale,
            } => {
                let (width, height) = card.unwrap_or((
                    (CARD_WIDTH * scale).round() as i32,
                    (CARD_HEIGHT_GUESS * scale).round() as i32,
                ));
                PhysicalRect {
                    x: centre_x - width / 2,
                    y: if rises { y - height } else { y },
                    width,
                    height,
                }
            }
        }
    }
}

/// The tour's own window over the game, for the stops no window of the app draws
/// (`Host::Screen`). It never takes the keyboard, so the game keeps it -- and with it the
/// price-check hotkey, held only while the game is in front.
struct Screen {
    layout: Option<Layout>,
    overlay: Option<Win32Overlay>,
    /// The card's size as last drawn, physical px: a card alone sizes the window by it.
    card: Rc<Cell<Option<(i32, i32)>>>,
    /// What was last applied to the platform window.
    applied: Option<PhysicalRect>,
    shown: bool,
}

/// Puts the tour's own window where the stop wants it (`Guide::screen_layout`), opening it the
/// first time.
fn place_screen(guide: &Entity<Guide>, cx: &mut App) {
    let (layout, screen) = {
        let g = guide.read(cx);
        (g.screen_layout(cx), g.screen)
    };
    match (screen, layout) {
        (Some(screen), layout) => {
            if let Err(err) = screen.update(cx, |screen, _, cx| screen.set_layout(layout, cx)) {
                log::warn!("the tour's window is gone: {err:#}");
            }
        }
        (None, Some(layout)) => {
            let screen = open_screen(layout, cx);
            guide.update(cx, |guide, _| guide.screen = screen);
        }
        (None, None) => {}
    }
}

fn open_screen(layout: Layout, cx: &mut App) -> Option<WindowHandle<Screen>> {
    let options = WindowOptions {
        // A placeholder: `Screen::place` moves it over the game, then shows it.
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(CARD_WIDTH), px(CARD_HEIGHT_GUESS)),
        ))),
        titlebar: None,
        // The game shows through the hole.
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        focus: false,
        show: false,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        window.set_window_title("PoE2 Oracle — tour");
        let overlay = Win32Overlay::from_window(window)
            .inspect_err(|err| log::warn!("the tour window's handle is unavailable: {err:#}"))
            .ok();
        cx.new(|cx| {
            if let Some(overlay) = overlay {
                if let Err(err) = overlay.disable_dwm_frame() {
                    log::warn!("{err:#}");
                }
                // Spawned before `place`'s first task: frameless, and never activated by a click,
                // by the time it first shows.
                cx.spawn(async move |_, _| {
                    if let Err(err) = overlay.remove_frame() {
                        log::warn!("{err:#}");
                    }
                    if let Err(err) = overlay.set_no_activate() {
                        log::warn!("{err:#}");
                    }
                })
                .detach();
            }
            let mut screen = Screen {
                layout: Some(layout),
                overlay,
                card: Rc::default(),
                applied: None,
                shown: false,
            };
            screen.place(cx);
            screen
        })
    });
    opened
        .inspect_err(|err| log::warn!("the tour's window is unavailable: {err:#}"))
        .ok()
}

impl Screen {
    fn set_layout(&mut self, layout: Option<Layout>, cx: &mut Context<Self>) {
        self.layout = layout;
        self.place(cx);
        cx.notify();
    }

    /// Brings the platform window in line with the layout: its rect, and shown or hidden.
    /// Deferred like every such call (`Win32Overlay::set_bounds`). Shown again, it gets its rect
    /// again too, which puts it back above any topmost window that rose meanwhile.
    fn place(&mut self, cx: &mut Context<Self>) {
        let Some(overlay) = self.overlay else {
            return;
        };
        let rect = self.layout.map(|layout| layout.rect(self.card.get()));
        let bounds = rect.filter(|rect| !self.shown || self.applied != Some(*rect));
        let shown = rect.is_some();
        let show = (self.shown != shown).then_some(shown);
        if bounds.is_some() {
            self.applied = bounds;
        }
        self.shown = shown;
        if bounds.is_none() && show.is_none() {
            return;
        }
        cx.spawn(async move |_, _| {
            if let Some(rect) = bounds
                && let Err(err) = overlay.set_bounds(rect)
            {
                log::warn!("{err:#}");
            }
            if let Some(shown) = show {
                overlay.set_shown(shown);
            }
        })
        .detach();
    }
}

impl Render for Screen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (Some(guide), Some(layout)) = (running(cx), self.layout) else {
            return div().into_any_element();
        };
        let g = guide.read(cx);
        // Over the game, the tour follows the interface scale as the overlays do.
        window.set_rem_size(px(BASE_REM_SIZE * g.app.read(cx).settings.ui_scale));
        let unit = rem_unit(window);
        match layout {
            Layout::Card { .. } => {
                let (drawn, screen) = (self.card.clone(), cx.weak_entity());
                div()
                    .on_children_prepainted(move |bounds, window, _| {
                        let Some(card) = bounds.first() else {
                            return;
                        };
                        let scale = window.scale_factor();
                        let physical = |length: Pixels| (f32::from(length) * scale).round() as i32;
                        let size = (physical(card.size.width), physical(card.size.height));
                        if drawn.replace(Some(size)) != Some(size) {
                            let screen = screen.clone();
                            window.on_next_frame(move |_, cx| {
                                screen.update(cx, |screen, cx| screen.place(cx)).ok();
                            });
                        }
                    })
                    .child(stop_card(&guide, g, false, cx))
                    .into_any_element()
            }
            Layout::Spotlight { area, line } => {
                let scale = window.scale_factor();
                let view = view_area(window);
                let line = Area {
                    x: (line.x - area.x) as f32 / scale,
                    y: (line.y - area.y) as f32 / scale,
                    width: line.width as f32 / scale,
                    height: line.height as f32 / scale,
                };
                let placement = Placement {
                    host: Host::Screen,
                    view,
                    hole: line.grown(HOLE_MARGIN * unit).within(view),
                    scrolled_out: false,
                    card: g.cards.borrow().get(&Host::Screen).copied(),
                    unit,
                };
                div()
                    .relative()
                    .size_full()
                    .child(spotlight(&guide, g, &placement, cx))
                    .child(settle(Host::Screen, None, placement.card))
                    .into_any_element()
            }
        }
    }
}

/// The window's pixels per pixel at 100 % scale: its rem size over the base one.
fn rem_unit(window: &Window) -> f32 {
    f32::from(window.rem_size()) / BASE_REM_SIZE
}

/// The window's whole area.
fn view_area(window: &Window) -> Area {
    let size = window.viewport_size();
    Area {
        x: 0.,
        y: 0.,
        width: f32::from(size.width),
        height: f32::from(size.height),
    }
}

fn area(bounds: Bounds<Pixels>) -> Area {
    Area {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
    }
}
