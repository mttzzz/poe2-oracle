//! The welcome after an install. The installer's finish page starts the app with `--installed`
//! (`crate::launch`), and the settings window opens at «Общие» with this dialog over it: the
//! player's sign that the install worked. It says that PoE2 Oracle is installed and running; that
//! it works in the background, where its icon is -- the tray, the taskbar or both, as
//! `Settings::app_icon` has it -- and that Windows first hides a new tray icon under the ^ arrow;
//! that a click on the icon opens these settings; how a price check starts once they're closed,
//! with the player's hotkey, and where the XP overlay shows; and whether the app starts with
//! Windows. One button, «Понятно», dismisses it, as do Esc, Enter and the window's close.
//!
//! A modal in the style the tour's cards set: the window dimmed under the scrim, the card in the
//! game's frame with its heading in gold, rising in over `style::APPEAR`. It is a global while
//! it's up, since more than its window waits for it: the tour starts once it's gone, not under it
//! ([`after`]).

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Entity, Global, SharedString, div, prelude::*,
    relative, rgb,
};

use crate::platform::autostart;
use crate::price_check::PriceCheckApp;
use crate::settings::{AppIcon, Settings};
use crate::tr;
use crate::ui::fonts;
use crate::ui::style::{
    APPEAR, ButtonKind, SCRIM_OPACITY, alpha, appear, button, ease, game_frame, heading,
    modal_shadow,
};
use crate::ui::theme::{BG_CARD, GOLD_LIGHT, TEXT, TEXT_DIM, rems_from_px};

/// The card's width at 100 % scale, px.
const CARD_WIDTH: f32 = 470.;

/// Something that waits for the welcome to go ([`after`]).
type Then = Box<dyn FnOnce(&mut App)>;

/// The welcome while it's up, with what waits for it to go.
pub struct Welcome {
    after: Vec<Then>,
}

impl Global for Welcome {}

/// Opens the settings window at «Общие» -- or brings the open one forward, back to «Общие» -- with
/// the welcome over it. What it says of autostart comes from the registry, as the window's own
/// toggle does (`app::open_settings`).
pub fn show(app: &Entity<PriceCheckApp>, cx: &mut App) {
    let was_open = app.read(cx).settings_window().is_some();
    crate::app::open_settings(app, cx);
    if app.read(cx).settings_window().is_none() {
        // It didn't open, and `open_settings` said why: nothing to lay the welcome over, and the
        // tour mustn't wait for it.
        return;
    }
    if was_open {
        // A window opened just now has read it already.
        app.update(cx, |state, cx| {
            state.settings.autostart = autostart::autostart_enabled();
            cx.notify();
        });
    }
    if !cx.has_global::<Welcome>() {
        cx.set_global(Welcome { after: Vec::new() });
        log::info!("welcome shown");
    }
}

/// Whether the welcome is up.
pub fn is_showing(cx: &App) -> bool {
    cx.has_global::<Welcome>()
}

/// Runs `then` once the welcome is gone -- at once when it isn't up.
pub fn after(cx: &mut App, then: impl FnOnce(&mut App) + 'static) {
    if cx.has_global::<Welcome>() {
        cx.global_mut::<Welcome>().after.push(Box::new(then));
    } else {
        then(cx);
    }
}

/// Takes the welcome down, and what waited for it follows -- outside the press or the close that
/// dismissed it. Nothing when it isn't up.
pub fn dismiss(cx: &mut App) {
    if !cx.has_global::<Welcome>() {
        return;
    }
    log::info!("welcome dismissed");
    for then in cx.remove_global::<Welcome>().after {
        cx.defer(then);
    }
}

/// The welcome over the settings window while it's up, worded from `settings`; `None`
/// otherwise. Lay it last in the window's root, which must be `relative`: it covers the whole
/// window, and nothing under it takes the mouse.
pub fn layer(settings: &Settings, cx: &App) -> Option<AnyElement> {
    if !is_showing(cx) {
        return None;
    }
    Some(
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
                    .bg(alpha(0x000000, SCRIM_OPACITY))
                    .occlude()
                    .with_animation(
                        "welcome-scrim",
                        Animation::new(APPEAR).with_easing(ease),
                        |scrim, t| scrim.opacity(t),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(welcome_card(settings)),
            )
            .into_any_element(),
    )
}

/// The card: the heading, what the welcome says, and «Понятно».
fn welcome_card(settings: &Settings) -> impl IntoElement {
    let face = fonts::interface_font();
    let paragraph = |text: SharedString, color: u32| div().text_color(rgb(color)).child(text);
    let card = div()
        .relative()
        .flex()
        .flex_col()
        .gap(rems_from_px(10.))
        .w(rems_from_px(CARD_WIDTH))
        .px(rems_from_px(24.))
        .pt(rems_from_px(20.))
        .pb(rems_from_px(20.))
        .bg(rgb(BG_CARD))
        .shadow(modal_shadow())
        .text_size(rems_from_px(13.5))
        .line_height(relative(1.5))
        // The card's clicks stay on it.
        .occlude()
        .child(
            heading(face)
                .pb(rems_from_px(2.))
                .text_size(rems_from_px(19.))
                .text_color(rgb(GOLD_LIGHT))
                .child(tr!("PoE2 Oracle is installed and running")),
        )
        .child(paragraph(whereabouts(settings.app_icon).into(), TEXT))
        .children(settings.app_icon.tray().then(|| {
            paragraph(
                tr!(
                    "Windows first hides a new app's icon under the ^ arrow (“Show hidden \
                     icons”): drag it from there onto the taskbar to keep it in sight."
                )
                .into(),
                TEXT_DIM,
            )
        }))
        .child(paragraph(
            format!(
                "{} {}",
                // The hotkey waits while this window is open (`PriceCheckApp::pressed`).
                tr!(
                    "Close these settings, hover over an item in the game and press {hotkey}: \
                     the price panel opens next to your inventory.",
                    hotkey = settings.hotkey
                ),
                if settings.xp_overlay {
                    tr!("The XP overlay shows above the flask panel.")
                } else {
                    tr!(
                        "The XP overlay above the flask panel is off: turn it on in the “XP \
                         overlay” section."
                    )
                }
            )
            .into(),
            TEXT,
        ))
        .child(paragraph(
            if settings.autostart {
                tr!("It starts with Windows, so it's ready whenever you play.")
            } else {
                tr!(
                    "It doesn't start with Windows yet: turn on “Start with Windows” in the \
                     “General” section to have it ready whenever you play."
                )
            }
            .into(),
            TEXT,
        ))
        .child(
            div()
                .flex()
                .justify_end()
                .pt(rems_from_px(6.))
                .child(button(
                    "welcome-got-it",
                    tr!("Got it"),
                    ButtonKind::Primary,
                    face,
                    |_, _, cx| dismiss(cx),
                )),
        )
        .child(game_frame());
    appear("welcome-card", card)
}

/// Where the app lives now, by `icon`, and that a click there opens these settings.
fn whereabouts(icon: AppIcon) -> &'static str {
    match icon {
        AppIcon::Tray => tr!(
            "From now on it works in the background, with no main window: its icon sits in the \
             notification area by the clock, and a click on the icon opens these settings."
        ),
        AppIcon::Taskbar => tr!(
            "From now on it works in the background, with no main window: its button stays on \
             the taskbar while it runs, and a click on the button opens these settings."
        ),
        AppIcon::Both => tr!(
            "From now on it works in the background, with no main window: its button stays on \
             the taskbar, its icon sits by the clock, and a click on either opens these \
             settings."
        ),
    }
}
