//! The settings window, in the game-styled look (`ui::style`): a title bar that drags the window,
//! a sidebar with the six sections -- Общие, Проверка цены, Быстрые действия, Оверлей опыта,
//! Аккаунт, Помощь -- and each section's page of cards. Its words are in the interface language
//! (`crate::i18n`) that Общие's «Язык интерфейса» picks, worded anew on every render, so a change
//! of language shows at once.
//!
//! Every change applies at once, through one handler ([`SettingsView::change`]): the app takes
//! the settings over (`PriceCheckApp::apply_settings`) and the file is written -- a write it
//! doesn't take is said over every section until one gets through; there is no Сохранить or
//! Отмена. A text field applies when the player leaves it (`ui::text_field`) or the app quits, a
//! hotkey recorder as soon as it takes a combination -- refused, with the reason under it and the
//! old hotkey kept, when another program holds that one. A quick action whose text is a denied
//! command (`quick_action::denied_command`) says so under its field and isn't saved: the file
//! keeps its last allowed text (`quick_action::kept_actions`). Resetting the waystone marks, the
//! one erasing action, asks first in a dialog over the window.
//!
//! Signing in and out of pathofexile.com (`crate::login`, `crate::session`) keeps its secret out
//! of the settings; Помощь's buttons write reports and open folders and pages.
//!
//! The window stays above the game (topmost), stepping down while the sign-in window -- which
//! isn't topmost -- is open over it. × and Esc (with no menu or dialog open) close it, and so does
//! anything that sends it `WM_CLOSE` -- Alt+F4, the taskbar -- routed through the same close
//! ([`SettingsView::close`]).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Context, Entity, FocusHandle, Focusable,
    IntoElement, KeyDownEvent, Keystroke, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, Render, SharedString, Window, WindowControlArea, div, linear_color_stop,
    linear_gradient, prelude::*, px, relative, rgb,
};
use serde_json::Value;
use trade_client::private_leagues;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_0, VK_9, VK_SHIFT};

use crate::bug_report;
use crate::diagnostics::{self, SetupProblem};
use crate::i18n::{self, Lang};
use crate::league_chip;
use crate::login::{self, Login, LoginProblem};
use crate::platform::autostart;
use crate::platform::win32::Win32Overlay;
use crate::price_check::{BootstrapState, PriceCheckApp};
use crate::quick_action::{self, ActionDraft};
use crate::session::{self, SessionStatus};
use crate::settings::{
    self, ClientLanguage, Hotkey, HotkeyProblem, InterfaceLanguage, KeyName, LeagueChoice,
    ListingStatusChoice, QuickActionKind, Settings,
};
use crate::tour::{Host, Stop};
use crate::tr;
use crate::ui::fonts::{self, NameFont};
use crate::ui::style::{
    ButtonKind, CARD_RADIUS, SCRIM_OPACITY, TRANSITION, alpha, appear, button, card, diamond, ease,
    ease_hover, ease_state, game_frame, heading, icon_button, keycaps, link, menu, modal_shadow,
    ornament_rule, recorder, section_heading, segmented, select, stepper, switch, switch_in,
    title_button, title_gradient,
};
use crate::ui::text_field::{Committed, TextField};
use crate::ui::theme::{
    BG_CARD, BG_PANEL, BG_SIDEBAR, BORDER_CARD, BORDER_GOLD, GOLD, GOLD_LIGHT, TEXT, TEXT_DIM,
    TEXT_MUTED, TEXT_WARNING, blend,
};
use crate::ui::tour;

const TITLE_HEIGHT: f32 = 40.;
const SIDEBAR_WIDTH: f32 = 216.;
/// The sidebar's list: where it starts, its rows and the gap between them.
const NAV_TOP: f32 = 18.;
const NAV_ITEM_HEIGHT: f32 = 38.;
const NAV_GAP: f32 = 2.;
/// Inset of a section's content from the sidebar and the window's right edge.
const CONTENT_INSET: f32 = 36.;
/// How much gold the top of a section's background takes.
const CONTENT_GLOW: f32 = 0.035;
/// The league menu's width, px: room for «Своя лига · » and a private league's name.
const LEAGUE_MENU_WIDTH: f32 = 280.;

/// What one click of the scale stepper moves it by, in percent.
const STEP_PERCENT: u16 = 5;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The app's repository: «GitHub ↗» in О программе.
const REPOSITORY_URL: &str = "https://github.com/mttzzz/poe2-oracle";
/// The third-party notices the installer puts next to the exe (`packaging/installer.nsi`).
const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.html";
/// Microsoft's WebView2 page, at its download section: the sign-in window needs the runtime.
const WEBVIEW2_DOWNLOAD: &str = "https://developer.microsoft.com/microsoft-edge/webview2/#download";

/// A choice's label, worded whenever it's shown, so it follows the interface language.
type Label = fn() -> &'static str;

/// The client languages, as the segmented choice lists them.
const CLIENT_LANGUAGES: [(ClientLanguage, Label); 3] = [
    (ClientLanguage::Auto, || tr!("Auto")),
    (ClientLanguage::Russian, || language_name(Lang::Russian)),
    (ClientLanguage::English, || language_name(Lang::English)),
];
/// The interface languages, as the segmented choice lists them ([`interface_language_labels`]).
const INTERFACE_LANGUAGES: [InterfaceLanguage; 3] = [
    InterfaceLanguage::Auto,
    InterfaceLanguage::Russian,
    InterfaceLanguage::English,
];
/// The default sellers, as the segmented choice lists them: instant buyout, the default, first.
const SELLERS: [(ListingStatusChoice, Label); 4] = [
    (ListingStatusChoice::Securable, || tr!("Instant Buyout")),
    (ListingStatusChoice::Available, || {
        tr!("Buyout or In Person")
    }),
    (ListingStatusChoice::Online, || tr!("In Person")),
    (ListingStatusChoice::Any, || tr!("Any")),
];
const ACTION_KINDS: [(QuickActionKind, Label); 2] = [
    (QuickActionKind::ChatCommand, || tr!("Chat")),
    (QuickActionKind::StashSearch, || tr!("Stash")),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    General,
    PriceCheck,
    QuickActions,
    XpOverlay,
    Account,
    Help,
}

impl Section {
    const ALL: [Section; 6] = [
        Section::General,
        Section::PriceCheck,
        Section::QuickActions,
        Section::XpOverlay,
        Section::Account,
        Section::Help,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::General => tr!("General"),
            Section::PriceCheck => tr!("Price check"),
            Section::QuickActions => tr!("Quick actions"),
            Section::XpOverlay => tr!("XP overlay"),
            Section::Account => tr!("Account"),
            Section::Help => tr!("Help"),
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Section::General => tr!("League, languages, scale, starting with Windows and updates"),
            Section::PriceCheck => tr!("Hotkey, sellers, results table and Waystones"),
            Section::QuickActions => tr!("Keys that type chat commands and searches into the game"),
            Section::XpOverlay => tr!("Experience rate and map timer, on top of the game's panels"),
            Section::Account => {
                tr!("Signing in to pathofexile.com: private leagues and “sum” rows")
            }
            Section::Help => tr!("Bug reports, logs and about the app"),
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    /// Where the sidebar's marker sits for this section.
    fn marker_top(self) -> f32 {
        NAV_TOP + self.index() as f32 * (NAV_ITEM_HEIGHT + NAV_GAP)
    }
}

/// What the window says above Общие: whatever in the player's setup keeps checks from working
/// (`diagnostics::setup_problems`). The first launch's welcome is the tour's (`ui::tour`).
pub struct Intro {
    pub problems: Vec<SetupProblem>,
}

/// Which hotkey recorder a key-down is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Recorder {
    PriceCheck,
    /// The quick action in this row.
    Action(usize),
}

/// The diagnostics report's progress, shown under its button.
enum ReportState {
    Idle,
    Writing,
    Written(PathBuf),
    Failed(String),
}

/// Where the lookup of a private league's typed name on pathofexile.com stands
/// (`SettingsView::apply_private_league`), said under its field.
enum LeagueCheck {
    Idle,
    Checking,
    /// The site has the league, now the private league: `corrected` when the name typed wasn't
    /// quite the site's -- its number missing or wrong, or, for one of the account's own leagues,
    /// its letter case off.
    Found {
        corrected: bool,
    },
    /// The site has no league by the name typed, which changed nothing.
    Missing(String),
    /// The site couldn't be asked: the name was taken as typed if it reads as a private league's
    /// (`taken`), and not otherwise.
    Unchecked {
        taken: bool,
    },
}

/// A quick action's row: what it will save (`quick_action::kept_actions`), the field its text is
/// typed in, and its hotkey recorder's focus.
struct ActionRow {
    draft: ActionDraft,
    field: Entity<TextField>,
    recorder: FocusHandle,
}

pub struct SettingsView {
    /// Whose settings these are: read as they stand on every render, changed through
    /// [`Self::change`].
    app: Entity<PriceCheckApp>,
    intro: Intro,
    /// The window's own focus: Esc closes from here. A click anywhere lands here (GPUI focuses
    /// the innermost focusable element under the mouse), ending a capture; so does a finished
    /// one, and nothing left focused (`on_focus_lost`).
    focus_handle: FocusHandle,
    section: Section,
    /// The sidebar marker's slide between sections: from where to where, since when, and how
    /// many so far (each restarts its animation).
    marker_from: f32,
    marker_to: f32,
    marker_moved: Option<Instant>,
    marker_slides: usize,
    league_menu: bool,
    confirming_reset: bool,
    /// Focused while the price-check hotkey recorder captures.
    recorder_focus: FocusHandle,
    /// The modifiers held during a capture, shown until the key comes.
    held: Modifiers,
    /// Why the combination last pressed into the capturing recorder can't be a hotkey, said while
    /// it still captures.
    capture_error: Option<&'static str>,
    /// A recorded combination another program holds, refused: said under its recorder until the
    /// next capture -- the recorder, the combination and the hotkey kept in its place (`None` for
    /// an action left without one).
    refused: Option<(Recorder, Hotkey, Option<Hotkey>)>,
    actions: Vec<ActionRow>,
    /// The private league's name (`LeagueChoice::Custom`): its text while that is the league.
    league_field: Entity<TextField>,
    /// Where the lookup of the name last typed there stands, and which lookup is the latest: an
    /// older one's answer, for a name since changed, is dropped.
    league_check: LeagueCheck,
    league_checks: u64,
    report: ReportState,
    /// The notices file next to the exe; `None` for a copy that wasn't installed.
    notices: Option<PathBuf>,
    /// The window, for keeping it above the game; `None` if its handle couldn't be read.
    overlay: Option<Win32Overlay>,
    /// Last topmost state handed to the window; `None` until the first.
    topmost: Option<bool>,
    /// The language «Авто» stands for (`i18n::auto`), read when the window opens: its choice
    /// names it.
    auto_language: Lang,
}

impl SettingsView {
    pub fn new(
        app: Entity<PriceCheckApp>,
        intro: Intro,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (actions, private_league) = {
            let settings = &app.read(cx).settings;
            (
                settings
                    .quick_actions
                    .iter()
                    .map(ActionDraft::saved)
                    .collect::<Vec<_>>(),
                settings.private_league.clone(),
            )
        };
        let actions = actions
            .into_iter()
            .map(|draft| Self::action_row(draft, window, cx))
            .collect();
        // A league name as the trade site writes one: the same in every interface language.
        let league_field =
            cx.new(|cx| TextField::new(private_league, || "My League (PL12345)", window, cx));
        cx.subscribe(&league_field, |view, _field, _: &Committed, cx| {
            view.apply_private_league(cx);
        })
        .detach();

        // Anything that closes the window through `WM_CLOSE` -- Alt+F4, the taskbar -- closes it
        // the way × does. Left to `DefWindowProc`, `WM_CLOSE` destroys the window before GPUI
        // lets go of it, and GPUI's teardown (`Drop for WindowsWindow`) then hides, un-registers
        // and destroys a handle that no longer exists -- «Недопустимый дескриптор окна» in the
        // log.
        let view = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            view.update(cx, |view, cx| view.close(window, cx)).is_err()
        });
        // Nothing focused -- a field or recorder done, a removed row's field -- is the window's
        // own focus again, where Esc closes.
        cx.on_focus_lost(window, |view, window, cx| {
            view.focus_handle.focus(window, cx)
        })
        .detach();
        cx.on_release(|view, cx| {
            log::info!("settings window closed");
            view.app.update(cx, |state, cx| {
                state.set_settings_window(None);
                cx.notify();
            });
        })
        .detach();
        // Everything shown comes from the app and these globals as they stand.
        cx.observe(&app, |_, _, cx| cx.notify()).detach();
        cx.observe_global::<SessionStatus>(|_, cx| cx.notify())
            .detach();
        cx.observe_global::<Login>(|view, cx| {
            view.sync_topmost(cx);
            cx.notify();
        })
        .detach();

        let overlay = match Win32Overlay::from_window(window) {
            Ok(overlay) => {
                // Windows 11's rounded corners and outline would cut the frame's corner diamonds.
                if let Err(err) = overlay.disable_dwm_frame() {
                    log::warn!("{err:#}");
                }
                Some(overlay)
            }
            Err(err) => {
                log::warn!("the settings window's handle is unavailable: {err:#}");
                None
            }
        };
        let top = Section::General.marker_top();
        let mut view = SettingsView {
            app,
            intro,
            focus_handle: cx.focus_handle(),
            section: Section::General,
            marker_from: top,
            marker_to: top,
            marker_moved: None,
            marker_slides: 0,
            league_menu: false,
            confirming_reset: false,
            recorder_focus: cx.focus_handle(),
            held: Modifiers::default(),
            capture_error: None,
            refused: None,
            actions,
            league_field,
            league_check: LeagueCheck::Idle,
            league_checks: 0,
            report: ReportState::Idle,
            notices: third_party_notices(),
            overlay,
            topmost: None,
            auto_language: i18n::auto(),
        };
        view.sync_topmost(cx);
        view
    }

    /// A row for `draft`, its field telling the view when the player leaves it.
    fn action_row(draft: ActionDraft, window: &mut Window, cx: &mut Context<Self>) -> ActionRow {
        let text = draft.text.clone();
        let placeholder = action_placeholder(draft.kind);
        let field = cx.new(|cx| TextField::new(text, placeholder, window, cx));
        cx.subscribe(&field, |view, field, _: &Committed, cx| {
            view.action_committed(&field, cx);
        })
        .detach();
        ActionRow {
            draft,
            field,
            recorder: cx.focus_handle(),
        }
    }

    /// Applies `edit` to the player's settings at once -- the one handler every control goes
    /// through. Autostart goes into the registry when it changed (that is where it lives), the app
    /// takes the settings over (`PriceCheckApp::apply_settings`), refusing a hotkey another
    /// program holds, and the file gets them as the app took them
    /// (`PriceCheckApp::save_settings`); what changed is logged.
    fn change(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Settings)) {
        self.app.update(cx, |state, cx| {
            let old = state.settings.clone();
            let mut new = old.clone();
            edit(&mut new);
            if new == old {
                return;
            }
            if new.autostart != old.autostart
                && let Err(err) = autostart::set_autostart(new.autostart)
            {
                log::warn!("{err:#}");
                new.autostart = autostart::autostart_enabled();
            }
            state.apply_settings(new, cx);
            state.save_settings(cx);
            let changed = changes(&old, &state.settings);
            if !changed.is_empty() {
                log::info!("settings changed: {changed}");
            }
        });
    }

    /// Saves the quick actions as the rows stand (`quick_action::kept_actions`) and applies them.
    /// Returns the rows whose hotkey the app refused -- another program holds it -- each with the
    /// combination refused; those rows now go without one, as the settings do.
    fn apply_actions(&mut self, cx: &mut Context<Self>) -> Vec<(usize, Hotkey)> {
        let actions = quick_action::kept_actions(self.actions.iter_mut().map(|row| &mut row.draft));
        self.change(cx, |settings| settings.quick_actions = actions);
        let taken: Vec<Option<Hotkey>> = self
            .app
            .read(cx)
            .settings
            .quick_actions
            .iter()
            .map(|action| action.hotkey)
            .collect();
        let saved_rows = self
            .actions
            .iter_mut()
            .enumerate()
            .filter(|(_, row)| row.draft.kept.is_some());
        let mut refused = Vec::new();
        for ((index, row), taken) in saved_rows.zip(taken) {
            if let Some(asked) = row.draft.hotkey
                && taken != Some(asked)
            {
                refused.push((index, asked));
                row.draft.hotkey = taken;
            }
        }
        refused
    }

    /// [`Self::apply_actions`], saying under each refused row why it went without its hotkey.
    fn apply_actions_noting(&mut self, cx: &mut Context<Self>) {
        for (index, asked) in self.apply_actions(cx) {
            self.refused = Some((Recorder::Action(index), asked, None));
        }
    }

    /// Closes the window: ×, Esc and `WM_CLOSE` all come here. What a field holds, typed but not
    /// left, applies first -- the window is gone before the field could report it. Hidden at once,
    /// then let go of by GPUI once what the hiding reported has run (`app::close_window`).
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_typed(cx);
        crate::app::close_window(window, cx);
    }

    /// Applies what the fields hold, typed but not yet left: the private league's name and the
    /// quick actions' texts. A field reports only when it's left, so the window's close does this
    /// first, and so does quitting the app (`app::quit`), which closes no window.
    pub fn apply_typed(&mut self, cx: &mut Context<Self>) {
        self.take_typed_private_league(cx);
        let mut typed = false;
        for row in &mut self.actions {
            let text = row.field.read(cx).text();
            if text != row.draft.text {
                row.draft.text = text.to_owned();
                typed = true;
            }
        }
        if typed {
            self.apply_actions_noting(cx);
        }
    }

    /// A quick action's field was left with a new text.
    fn action_committed(&mut self, field: &Entity<TextField>, cx: &mut Context<Self>) {
        let text = field.read(cx).text().to_owned();
        let Some(row) = self.actions.iter_mut().find(|row| row.field == *field) else {
            return;
        };
        row.draft.text = text;
        self.apply_actions_noting(cx);
        cx.notify();
    }

    /// Takes the private league's field as it reads, once the player changed it. Emptied, the
    /// private league is forgotten. A name of one of the account's own leagues, in any letter
    /// case, takes that league; any other is looked up on pathofexile.com first
    /// (`private_leagues::find`: a league's page is public) -- the league it names is taken as the
    /// site writes it, its number filled in or put right, and a name the site doesn't know
    /// changes nothing and is said to be wrong.
    fn apply_private_league(&mut self, cx: &mut Context<Self>) {
        let typed = self.league_field.read(cx).text().trim().to_owned();
        let app = self.app.read(cx);
        if typed == app.settings.private_league {
            return;
        }
        self.league_checks += 1;
        let known =
            private_leagues::among(app.private_leagues(), &typed).map(|league| league.id.clone());
        if typed.is_empty() || known.is_some() {
            let name = known.unwrap_or_default();
            self.league_check = if name == typed {
                LeagueCheck::Idle
            } else {
                self.league_field
                    .update(cx, |field, cx| field.set_text(name.clone(), cx));
                LeagueCheck::Found { corrected: true }
            };
            self.take_private_league(name, cx);
            return;
        }
        self.league_check = LeagueCheck::Checking;
        let check = self.league_checks;
        let client = cx.http_client();
        cx.spawn(async move |view, cx| {
            let found = private_leagues::find(&client, &typed).await;
            let _ = view.update(cx, |view, cx| {
                if view.league_checks != check {
                    return;
                }
                view.league_check = match found {
                    Ok(Some(league)) => {
                        let corrected = league.id != typed;
                        view.league_field
                            .update(cx, |field, cx| field.set_text(league.id.clone(), cx));
                        view.take_private_league(league.id, cx);
                        LeagueCheck::Found { corrected }
                    }
                    Ok(None) => LeagueCheck::Missing(typed),
                    Err(err) => {
                        log::warn!("looking the private league up failed: {err:#}");
                        let taken = private_leagues::is_private(&typed);
                        if taken {
                            view.take_private_league(typed, cx);
                        }
                        LeagueCheck::Unchecked { taken }
                    }
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The private league's field as it reads when the window closes, with no time left to look
    /// a name up: taken if it names one of the account's own leagues, or is written as the trade
    /// site writes one -- "<name> (PL<number>)" -- or is emptied; a bare name, unchecked, isn't.
    fn take_typed_private_league(&mut self, cx: &mut Context<Self>) {
        let typed = self.league_field.read(cx).text().trim().to_owned();
        let app = self.app.read(cx);
        if typed == app.settings.private_league {
            return;
        }
        let known =
            private_leagues::among(app.private_leagues(), &typed).map(|league| league.id.clone());
        if let Some(name) = known {
            self.take_private_league(name, cx);
        } else if typed.is_empty() || private_leagues::is_private(&typed) {
            self.take_private_league(typed, cx);
        }
    }

    /// Makes `name` the private league: remembered, and the league searched -- or, empty,
    /// forgotten, searches going back to «Авто» when it was the league searched.
    fn take_private_league(&mut self, name: String, cx: &mut Context<Self>) {
        let searched = matches!(self.app.read(cx).settings.league, LeagueChoice::Custom(_));
        self.change(cx, |settings| {
            if name.is_empty() {
                if searched {
                    settings.league = LeagueChoice::Auto;
                }
            } else {
                settings.league = LeagueChoice::Custom(name.clone());
            }
            settings.private_league = name;
        });
    }

    /// A league picked in Общие's menu. The private league's field goes back to the name
    /// remembered, dropping what was typed there and not taken yet: the pick is what counts.
    fn choose_league(&mut self, choice: LeagueChoice, cx: &mut Context<Self>) {
        self.league_menu = false;
        self.league_checks += 1;
        self.league_check = LeagueCheck::Idle;
        self.change(cx, |settings| settings.league = choice);
        let private_league = self.app.read(cx).settings.private_league.clone();
        self.league_field
            .update(cx, |field, cx| field.set_text(private_league, cx));
        cx.notify();
    }

    /// Keeps the window above the game -- except while the sign-in window it opened is up: that
    /// one isn't topmost, and must show in front of this one.
    fn sync_topmost(&mut self, cx: &mut Context<Self>) {
        let Some(overlay) = self.overlay else {
            return;
        };
        let topmost = !cx.try_global::<Login>().is_some_and(Login::is_open);
        if self.topmost == Some(topmost) {
            return;
        }
        self.topmost = Some(topmost);
        // `SetWindowPos` sends messages into GPUI's window procedure: not from inside an update.
        cx.spawn(async move |_, _| {
            if let Err(err) = overlay.set_topmost(topmost) {
                log::warn!("{err:#}");
            }
            // Stepping down puts this window first among those that aren't topmost -- over the
            // sign-in window, when that opened first (its task ran before this one): it comes
            // forward again.
            if !topmost {
                crate::platform::login_window::bring_forward();
            }
        })
        .detach();
    }

    /// The marker's position `now`, partway along its slide.
    fn marker_at(&self, now: Instant) -> f32 {
        match self.marker_moved {
            Some(moved) => {
                let progress = (now.saturating_duration_since(moved).as_secs_f32()
                    / TRANSITION.as_secs_f32())
                .min(1.);
                self.marker_from + (self.marker_to - self.marker_from) * ease(progress)
            }
            None => self.marker_to,
        }
    }

    fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        if section == self.section {
            return;
        }
        let now = Instant::now();
        self.marker_from = self.marker_at(now);
        self.marker_to = section.marker_top();
        self.marker_moved = Some(now);
        self.marker_slides += 1;
        self.section = section;
        self.league_menu = false;
        cx.notify();
    }

    /// Esc closes what is open, innermost first, then the window. A field or a capturing recorder
    /// takes its Esc itself.
    fn escape(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }
        if self.confirming_reset {
            self.confirming_reset = false;
        } else if self.league_menu {
            self.league_menu = false;
        } else {
            self.close(window, cx);
            return;
        }
        cx.notify();
    }

    /// The reset dialog closes, the marks kept.
    fn keep_marks(&mut self, cx: &mut Context<Self>) {
        self.confirming_reset = false;
        cx.notify();
    }

    fn reset_waystone_marks(&mut self, cx: &mut Context<Self>) {
        self.confirming_reset = false;
        self.change(cx, |settings| settings.waystone_marks.clear());
        cx.notify();
    }

    /// «Сообщить ↗»: the tray's «Сообщить об ошибке» (`bug_report::report_bug`).
    fn report_bug(&mut self, cx: &mut Context<Self>) {
        let (summary, language) = {
            let state = self.app.read(cx);
            (state.diagnostics_summary(), state.item_language())
        };
        bug_report::report_bug(summary, language, cx);
    }

    /// Writes the diagnostics report off the main thread, then shows it in Explorer.
    fn write_report(&mut self, cx: &mut Context<Self>) {
        if matches!(self.report, ReportState::Writing) {
            return;
        }
        let summary = self.app.read(cx).diagnostics_summary();
        self.report = ReportState::Writing;
        cx.notify();
        cx.spawn(async move |view, cx| {
            let written = cx
                .background_executor()
                .spawn(async move { diagnostics::write_report(&summary) })
                .await;
            let report = match written {
                Ok(path) => {
                    // The name only: the folder is the player's desktop, and the log goes into
                    // the next report.
                    let name = path.file_name().unwrap_or_default().display();
                    log::info!("diagnostics report written: {name}");
                    diagnostics::reveal(&path);
                    ReportState::Written(path)
                }
                Err(err) => {
                    log::warn!("writing the diagnostics report failed: {err:#}");
                    ReportState::Failed(format!("{err:#}"))
                }
            };
            view.update(cx, |view, cx| {
                view.report = report;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn recorder_focus(&self, recorder: Recorder) -> Option<&FocusHandle> {
        match recorder {
            Recorder::PriceCheck => Some(&self.recorder_focus),
            Recorder::Action(index) => self.actions.get(index).map(|row| &row.recorder),
        }
    }

    /// A press on a recorder: it starts capturing -- the same press focuses it (`track_focus`),
    /// which is what makes it capture -- or, while it captures, stops, the hotkey as it was.
    fn press_recorder(&mut self, recorder: Recorder, window: &mut Window, cx: &mut Context<Self>) {
        let capturing = self
            .recorder_focus(recorder)
            .is_some_and(|focus| focus.is_focused(window));
        if capturing {
            // The press must not focus the recorder again.
            window.prevent_default();
            self.stop_capture(window, cx);
        } else {
            self.held = window.modifiers();
            self.capture_error = None;
            self.refused = None;
            cx.notify();
        }
    }

    fn stop_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn capture_key(
        &mut self,
        recorder: Recorder,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Every key is the recorder's while it captures: Esc must not also close the window, nor
        // Alt+key reach the system menu.
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        let keystroke = &event.keystroke;
        if !keystroke.modifiers.modified() {
            match (keystroke.key.as_str(), recorder) {
                ("escape", _) => {
                    self.stop_capture(window, cx);
                    return;
                }
                // A quick action may go without a hotkey; the price check may not.
                ("backspace" | "delete", Recorder::Action(index)) => {
                    self.stop_capture(window, cx);
                    self.set_action_hotkey(index, None, cx);
                    return;
                }
                _ => {}
            }
        }
        match recorded_hotkey(keystroke).and_then(|hotkey| self.unused(hotkey, recorder, cx)) {
            Ok(hotkey) => {
                self.stop_capture(window, cx);
                match recorder {
                    Recorder::PriceCheck => self.set_price_check_hotkey(hotkey, cx),
                    Recorder::Action(index) => self.set_action_hotkey(index, Some(hotkey), cx),
                }
            }
            Err(error) => {
                self.capture_error = Some(error);
                cx.notify();
            }
        }
    }

    /// `hotkey`, unless a recorder other than `recorder` already has it: one combination does
    /// one thing.
    fn unused(&self, hotkey: Hotkey, recorder: Recorder, cx: &App) -> Result<Hotkey, &'static str> {
        if recorder != Recorder::PriceCheck && self.app.read(cx).settings.hotkey == hotkey {
            return Err(tr!("The price check already uses this combination"));
        }
        let taken = self.actions.iter().enumerate().any(|(index, row)| {
            recorder != Recorder::Action(index) && row.draft.hotkey == Some(hotkey)
        });
        if taken {
            return Err(tr!("Another quick action already uses this combination"));
        }
        Ok(hotkey)
    }

    /// Makes `hotkey` the price check's at once; the app refuses it when another program holds
    /// it, and the old one stays.
    fn set_price_check_hotkey(&mut self, hotkey: Hotkey, cx: &mut Context<Self>) {
        let previous = self.app.read(cx).settings.hotkey;
        self.refused = None;
        self.change(cx, |settings| settings.hotkey = hotkey);
        if self.app.read(cx).settings.hotkey != hotkey {
            self.refused = Some((Recorder::PriceCheck, hotkey, Some(previous)));
        }
        cx.notify();
    }

    /// Gives the quick action in row `index` `hotkey`, at once for a saved action. One another
    /// program holds is refused, and the action keeps the hotkey it had.
    fn set_action_hotkey(&mut self, index: usize, hotkey: Option<Hotkey>, cx: &mut Context<Self>) {
        let Some(row) = self.actions.get_mut(index) else {
            return;
        };
        let previous = std::mem::replace(&mut row.draft.hotkey, hotkey);
        if previous == hotkey {
            return;
        }
        self.refused = None;
        let refused = self.apply_actions(cx);
        if let Some(&(_, asked)) = refused.iter().find(|(refused, _)| *refused == index) {
            self.actions[index].draft.hotkey = previous;
            // The refusal left the action without one: its old one is held again.
            if previous.is_some() {
                self.apply_actions(cx);
            }
            self.refused = Some((Recorder::Action(index), asked, previous));
        }
        cx.notify();
    }

    fn add_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.actions.len() >= settings::MAX_QUICK_ACTIONS {
            return;
        }
        let row = Self::action_row(ActionDraft::default(), window, cx);
        // Ready for typing the command at once; it's saved once it has a text.
        window.focus(&row.field.focus_handle(cx), cx);
        self.actions.push(row);
        cx.notify();
    }

    fn remove_action(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.actions.len() {
            return;
        }
        let removed = self.actions.remove(index);
        // Notes name rows by place, and the places below this one just moved.
        self.refused = None;
        if removed.draft.kept.is_some() {
            self.apply_actions_noting(cx);
        }
        cx.notify();
    }

    /// Switches the action in row `index` between a chat command and a stash search; its
    /// field's placeholder follows.
    fn set_action_kind(&mut self, index: usize, kind: QuickActionKind, cx: &mut Context<Self>) {
        let Some(row) = self.actions.get_mut(index) else {
            return;
        };
        if row.draft.kind == kind {
            return;
        }
        row.draft.kind = kind;
        row.field.update(cx, |field, cx| {
            field.set_placeholder(action_placeholder(kind));
            cx.notify();
        });
        if row.draft.kept.is_some() {
            self.apply_actions_noting(cx);
        }
        cx.notify();
    }

    /// What a recorder's row says: while it captures, the rules -- or why the combination just
    /// pressed can't be one; otherwise why its last one was refused, or `idle`.
    fn recorder_line(
        &self,
        recorder: Recorder,
        capturing: bool,
        rules: &'static str,
        idle: Option<&'static str>,
    ) -> Option<AnyElement> {
        if capturing {
            return Some(match self.capture_error {
                Some(error) => note(error, TEXT_WARNING),
                None => note(rules, TEXT_DIM),
            });
        }
        match self.refused {
            Some((refused, asked, kept)) if refused == recorder => {
                Some(note(taken_note(asked, kept), TEXT_WARNING))
            }
            _ => idle.map(|idle| note(idle, TEXT_DIM)),
        }
    }

    /// A hotkey recorder: `current` as keycaps (or `empty` without one) until pressed, then the
    /// modifiers held so far until a combination comes (`capture_key`).
    fn render_recorder(
        &self,
        which: Recorder,
        current: Option<Hotkey>,
        empty: &'static str,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let focus = self
            .recorder_focus(which)
            .cloned()
            .unwrap_or_else(|| self.recorder_focus.clone());
        let capturing = focus.is_focused(window);
        let content: AnyElement = if capturing {
            let held: String =
                settings::modifier_names(self.held.control, self.held.shift, self.held.alt)
                    .map(|name| format!("{name}+"))
                    .collect();
            let prompt: SharedString = if held.is_empty() {
                tr!("Press a combination…").into()
            } else {
                format!("{held}…").into()
            };
            div()
                .text_color(rgb(GOLD_LIGHT))
                .child(prompt)
                .into_any_element()
        } else {
            match current {
                Some(hotkey) => keycaps(&hotkey_labels(hotkey)).into_any_element(),
                None => div()
                    .text_color(rgb(TEXT_MUTED))
                    .child(empty)
                    .into_any_element(),
            }
        };
        let key = match which {
            Recorder::PriceCheck => "price-check-hotkey",
            Recorder::Action(_) => "keys",
        };
        div()
            .track_focus(&focus)
            .flex_none()
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                view.capture_key(which, event, window, cx);
            }))
            .on_modifiers_changed(cx.listener(
                |view, event: &ModifiersChangedEvent, _window, cx| {
                    view.held = event.modifiers;
                    cx.notify();
                },
            ))
            .child(recorder(
                key,
                content,
                capturing,
                cx.listener(move |view, _: &MouseDownEvent, window, cx| {
                    view.press_recorder(which, window, cx);
                }),
            ))
    }

    fn render_title_bar(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(TITLE_HEIGHT))
            .bg(title_gradient())
            .border_b_1()
            .border_color(rgb(BORDER_GOLD))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .gap(px(10.))
                    .pl(px(20.))
                    .window_control_area(WindowControlArea::Drag)
                    .child(diamond(8., GOLD))
                    .child(
                        heading(face)
                            .text_size(px(15.))
                            .text_color(rgb(GOLD_LIGHT))
                            .child("PoE2 Oracle"),
                    )
                    .child(div().text_color(rgb(TEXT_MUTED)).child("·"))
                    .child(
                        heading(face)
                            .text_size(px(15.))
                            .text_color(rgb(TEXT))
                            .child(tr!("Settings")),
                    ),
            )
            .child(title_button(
                "close",
                "×",
                46.,
                true,
                cx.listener(|view, _: &MouseDownEvent, window, cx| view.close(window, cx)),
            ))
    }

    fn render_sidebar(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        let (from, to) = (self.marker_from, self.marker_to);
        let marker = div()
            .absolute()
            .left(px(10.))
            .right(px(10.))
            .h(px(NAV_ITEM_HEIGHT))
            .rounded(px(6.))
            .bg(linear_gradient(
                90.,
                linear_color_stop(alpha(GOLD, 0.16), 0.),
                linear_color_stop(alpha(GOLD, 0.02), 1.),
            ))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(10.))
                    .bottom(px(10.))
                    .w(px(2.))
                    .rounded_full()
                    .bg(rgb(GOLD)),
            )
            .with_animation(
                ("marker", self.marker_slides),
                Animation::new(TRANSITION).with_easing(ease),
                move |marker, t| marker.top(px(from + (to - from) * t)),
            );
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .bg(rgb(BG_SIDEBAR))
            .border_r_1()
            .border_color(rgb(BORDER_CARD))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(NAV_GAP))
                    .px(px(10.))
                    .pt(px(NAV_TOP))
                    .child(marker)
                    .children(Section::ALL.map(|section| self.render_nav_item(section, face, cx))),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(26.))
                    .pb(px(18.))
                    .child(
                        heading(face)
                            .text_size(px(13.))
                            .text_color(rgb(GOLD))
                            .child("PoE2 Oracle"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(TEXT_MUTED))
                            .child(tr!("version {version}", version = VERSION)),
                    ),
            )
    }

    /// A section in the sidebar, named in the heading face.
    fn render_nav_item(
        &self,
        section: Section,
        face: &'static NameFont,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let current = section == self.section;
        let item = div()
            .id(("nav", section.index()))
            .relative()
            .flex()
            .items_center()
            .h(px(NAV_ITEM_HEIGHT))
            .px(px(18.))
            .rounded(px(6.))
            .font_family(face.family)
            .font_weight(face.weight)
            .text_size(px(15.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| view.show(section, cx)),
            );
        ease_hover(("nav", section.index()), item, move |item, hover| {
            item.bg(alpha(GOLD, 0.05 * hover)).child(ease_state(
                "label",
                current,
                div(),
                move |label, on| {
                    label
                        .text_color(rgb(blend(blend(TEXT_DIM, TEXT, hover), GOLD_LIGHT, on)))
                        .child(section.title())
                },
            ))
        })
    }

    fn render_content(
        &self,
        settings: &Settings,
        face: &'static NameFont,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let section = self.section;
        let body: AnyElement = match section {
            Section::General => self
                .render_general(settings, face, window, cx)
                .into_any_element(),
            Section::PriceCheck => self
                .render_price_check(settings, face, window, cx)
                .into_any_element(),
            Section::QuickActions => self
                .render_quick_actions(face, window, cx)
                .into_any_element(),
            Section::XpOverlay => self
                .render_xp_overlay(settings, face, cx)
                .into_any_element(),
            Section::Account => self.render_account(settings, face, cx).into_any_element(),
            Section::Help => self.render_help(face, cx).into_any_element(),
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            // A faint warmth under the title bar, as if lit from it, gone a third of the way
            // down.
            .bg(linear_gradient(
                180.,
                linear_color_stop(rgb(blend(BG_PANEL, GOLD, CONTENT_GLOW)), 0.),
                linear_color_stop(rgb(BG_PANEL), 0.3),
            ))
            .child(switch_in(
                ("section", section.index()),
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .gap(px(4.))
                            .px(px(CONTENT_INSET))
                            .pt(px(26.))
                            .pb(px(16.))
                            .child(
                                heading(face)
                                    .text_size(px(24.))
                                    .text_color(rgb(GOLD_LIGHT))
                                    .child(section.title()),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(rgb(TEXT_DIM))
                                    .child(section.summary()),
                            )
                            .children(self.app.read(cx).save_failure().map(|error| {
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .pt(px(4.))
                                    .text_size(px(13.))
                                    .text_color(rgb(TEXT_WARNING))
                                    .child(div().flex_none().child("⚠"))
                                    // Wrapped in the content's width: the system's reason can be
                                    // long.
                                    .child(div().flex_1().min_w_0().child(tr!(
                                        "Couldn't save the settings: {error}",
                                        error = error
                                    )))
                            }))
                            .child(div().pt(px(12.)).child(ornament_rule(BORDER_GOLD))),
                    )
                    .child(
                        div()
                            .id(("section-body", section.index()))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(CONTENT_INSET))
                            .pt(px(4.))
                            .pb(px(28.))
                            .child(body),
                    ),
            ))
    }

    /// The setup problems, in a gold-edged card; nothing when there are none.
    fn render_intro(&self) -> Option<AnyElement> {
        let problems = &self.intro.problems;
        if problems.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .px(px(18.))
                .py(px(16.))
                .rounded(px(CARD_RADIUS))
                .bg(rgb(BG_CARD))
                .border_1()
                .border_color(rgb(BORDER_GOLD))
                .children(problems.iter().map(|problem| {
                    div()
                        .flex()
                        .gap(px(8.))
                        .text_size(px(13.))
                        .text_color(rgb(TEXT_WARNING))
                        .child(div().flex_none().child("⚠"))
                        .child(problem.text())
                }))
                .into_any_element(),
        )
    }

    fn render_general(
        &self,
        settings: &Settings,
        face: &'static NameFont,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let language = CLIENT_LANGUAGES
            .iter()
            .position(|&(language, _)| language == settings.client_language)
            .unwrap_or(0);
        let interface_language = INTERFACE_LANGUAGES
            .iter()
            .position(|&choice| choice == settings.interface_language)
            .unwrap_or(0);
        let scale = scale_percent(settings.ui_scale);
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .children(self.render_intro())
            .child(group(
                face,
                tr!("League and language"),
                [
                    self.render_league(settings, window, cx),
                    setting_row(
                        tr!("Game client language"),
                        [note(
                            tr!(
                                "The language the game copies items in. Auto tells it from the \
                                 item's text"
                            ),
                            TEXT_DIM,
                        )],
                        segmented(
                            "client-language",
                            CLIENT_LANGUAGES.map(|(_, label)| SharedString::from(label())),
                            language,
                            cx.listener(|view, index: &usize, _, cx| {
                                if let Some(&(language, _)) = CLIENT_LANGUAGES.get(*index) {
                                    view.change(cx, |settings| {
                                        settings.client_language = language;
                                    });
                                }
                            }),
                        ),
                    ),
                    setting_row(
                        tr!("Interface language"),
                        [note(
                            tr!(
                                "The app's own words; items and the trade site stay as they are. \
                                 Auto follows the game client, or Windows until the game has run"
                            ),
                            TEXT_DIM,
                        )],
                        segmented(
                            "interface-language",
                            interface_language_labels(self.auto_language),
                            interface_language,
                            cx.listener(|view, index: &usize, window, cx| {
                                if let Some(&choice) = INTERFACE_LANGUAGES.get(*index) {
                                    view.change(cx, |settings| {
                                        settings.interface_language = choice;
                                    });
                                    // What the taskbar and Alt+Tab show follows too.
                                    window.set_window_title(window_title());
                                }
                            }),
                        ),
                    ),
                ],
            ))
            .child(group(
                face,
                tr!("Window"),
                [setting_row(
                    tr!("Interface scale"),
                    [note(
                        tr!("Size of the text and controls on the price panel"),
                        TEXT_DIM,
                    )],
                    stepper(
                        "scale",
                        i18n::percent(scale),
                        (
                            scale > scale_percent(settings::MIN_UI_SCALE),
                            scale < scale_percent(settings::MAX_UI_SCALE),
                        ),
                        cx.listener(|view, up: &bool, _, cx| {
                            view.change(cx, |settings| step_scale(settings, *up));
                        }),
                    ),
                )],
            ))
            .child(group(
                face,
                tr!("System"),
                [
                    toggle_row(
                        "autostart",
                        tr!("Start with Windows"),
                        None,
                        settings.autostart,
                        |settings| &mut settings.autostart,
                        cx,
                    ),
                    toggle_row(
                        "check-updates",
                        tr!("Check for updates automatically"),
                        Some(tr!(
                            "Half a minute after launch; a new version shows up in the menu of \
                             the icon by the clock"
                        )),
                        settings.check_updates,
                        |settings| &mut settings.check_updates,
                        cx,
                    ),
                ],
            ))
    }

    /// The league searches go to: «Авто» (naming the league it stands for), every league the
    /// trade site lists -- named as the site names them in the interface language, as the panel's
    /// league chip does -- and the league chosen when it is neither: one the site no longer lists,
    /// or the private one (set in Аккаунт).
    fn render_league(
        &self,
        settings: &Settings,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let app = self.app.read(cx);
        let listed = app.leagues();
        let options = league_chip::menu(
            &settings.league,
            &settings.private_league,
            app.private_leagues(),
            listed,
            app.league_names(),
        );
        let picked = options
            .iter()
            .position(|(choice, _)| *choice == settings.league)
            .unwrap_or(0);
        let current: SharedString = options
            .get(picked)
            .map(|(_, label)| label.clone().into())
            .unwrap_or_default();
        let (about, color) = match &settings.league {
            _ if listed.is_empty() && matches!(app.bootstrap, BootstrapState::Loading) => {
                (tr!("Loading the league list from the trade site"), TEXT_DIM)
            }
            _ if listed.is_empty() => (
                tr!("The league list from the trade site didn't load"),
                TEXT_WARNING,
            ),
            LeagueChoice::Named(league) if !listed.contains(league) => (
                tr!("The trade site no longer lists this league — searches go to the current one"),
                TEXT_WARNING,
            ),
            LeagueChoice::Custom(_) => (
                tr!("Private league: its name is set in the “Account” section"),
                TEXT_DIM,
            ),
            LeagueChoice::Auto | LeagueChoice::Named(_) => (
                tr!(
                    "Where prices are searched. Auto is the trade site's current league; a \
                     private one is set in “Account”"
                ),
                TEXT_DIM,
            ),
        };
        let (choices, labels): (Vec<LeagueChoice>, Vec<SharedString>) = options
            .into_iter()
            .map(|(choice, label)| (choice, label.into()))
            .unzip();
        let control = div()
            .flex()
            .flex_col()
            .child(select(
                "league",
                current,
                false,
                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                    view.league_menu = !view.league_menu;
                    cx.notify();
                }),
            ))
            .when(self.league_menu, |this| {
                this.child(menu(
                    "league-menu",
                    labels,
                    picked,
                    LEAGUE_MENU_WIDTH,
                    window,
                    cx.listener(move |view, index: &usize, _, cx| {
                        if let Some(choice) = choices.get(*index) {
                            view.choose_league(choice.clone(), cx);
                        }
                    }),
                    cx.listener(|view, _: &MouseDownEvent, _, cx| {
                        view.league_menu = false;
                        cx.notify();
                    }),
                ))
            });
        // The tour's first stop points here (`ui::tour`).
        tour::spot(
            Stop::League,
            setting_row(tr!("League"), [note(about, color)], control),
        )
        .into_any_element()
    }

    fn render_price_check(
        &self,
        settings: &Settings,
        face: &'static NameFont,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let capturing = self.recorder_focus.is_focused(window);
        let sellers = SELLERS
            .iter()
            .position(|&(sellers, _)| sellers == settings.listing_status)
            .unwrap_or(0);
        let marks = settings.waystone_marks.len();
        let marks_note: SharedString = if marks == 0 {
            tr!("No marks. They're set on the price panel, next to a Waystone's modifiers").into()
        } else {
            tr!(
                "Marks: {count}. Danger, caution, wanted — set on the price panel",
                count = marks
            )
            .into()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(group(
                face,
                tr!("Hotkey"),
                [setting_row(
                    tr!("Price check"),
                    self.recorder_line(
                        Recorder::PriceCheck,
                        capturing,
                        tr!(
                            "Ctrl or Alt with a letter or digit, or F1–F12 · Esc keeps the old one"
                        ),
                        Some(tr!(
                            "Hover over an item in the game and press this combination"
                        )),
                    ),
                    self.render_recorder(
                        Recorder::PriceCheck,
                        Some(settings.hotkey),
                        "",
                        window,
                        cx,
                    ),
                )],
            ))
            .child(group(
                face,
                tr!("Search"),
                [
                    setting_row(
                        tr!("Default sellers"),
                        [note(
                            tr!(
                                "What every check starts with; the panel's “Sellers” chip changes \
                                 it for one item. Instant Buyout doesn't need the seller online: \
                                 you buy in the game"
                            ),
                            TEXT_DIM,
                        )],
                        segmented(
                            "sellers",
                            SELLERS.map(|(_, label)| SharedString::from(label())),
                            sellers,
                            cx.listener(|view, index: &usize, _, cx| {
                                if let Some(&(sellers, _)) = SELLERS.get(*index) {
                                    view.change(cx, |settings| settings.listing_status = sellers);
                                }
                            }),
                        ),
                    ),
                    toggle_row(
                        "seller-column",
                        tr!("Seller column"),
                        Some(tr!("The seller's name in the results table")),
                        settings.show_seller_column,
                        |settings| &mut settings.show_seller_column,
                        cx,
                    ),
                ],
            ))
            .child(group(
                face,
                tr!("Waystones"),
                [setting_row(
                    tr!("Modifier marks"),
                    [note(marks_note, TEXT_DIM)],
                    div().when(marks > 0, |this| {
                        this.child(button(
                            "reset-marks",
                            tr!("Reset…"),
                            ButtonKind::Danger,
                            face,
                            cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                view.confirming_reset = true;
                                cx.notify();
                            }),
                        ))
                    }),
                )],
            ))
    }

    fn render_quick_actions(
        &self,
        face: &'static NameFont,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let rows: Vec<AnyElement> = if self.actions.is_empty() {
            vec![
                div()
                    .px(px(14.))
                    .py(px(12.))
                    .text_color(rgb(TEXT_MUTED))
                    .child(tr!("No actions yet"))
                    .into_any_element(),
            ]
        } else {
            self.actions
                .iter()
                .enumerate()
                .map(|(index, row)| self.render_action(index, row, window, cx))
                .collect()
        };
        let can_add = self.actions.len() < settings::MAX_QUICK_ACTIONS;
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .text_size(px(13.))
                    .child(div().text_color(rgb(TEXT_DIM)).child(tr!(
                        "Chat: the command goes to the game's chat — /hideout, /exit, @last thanks \
                         (a reply to the last player who whispered you). Stash: a search in the \
                         open stash or at a vendor, e.g. from poe2.re. The keys work while the \
                         game is in front."
                    )))
                    .child(div().text_size(px(12.)).text_color(rgb(TEXT_MUTED)).child(tr!(
                        "Dangerous commands, such as /destroy and /clear_ignore_list, are never \
                         sent or saved."
                    ))),
            )
            .child(group(face, tr!("Actions"), rows))
            .when(can_add, |this| {
                this.child(div().flex().child(button(
                    "add-action",
                    tr!("+ Add action"),
                    ButtonKind::Secondary,
                    face,
                    cx.listener(|view, _: &MouseDownEvent, window, cx| {
                        view.add_action(window, cx);
                    }),
                )))
            })
    }

    /// One quick action: its kind, text, hotkey and ×, and under them why its text won't be
    /// sent, or what its recorder says.
    fn render_action(
        &self,
        index: usize,
        row: &ActionRow,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let capturing = row.recorder.is_focused(window);
        let line = match quick_action::denied_command(row.field.read(cx).text()) {
            Some(denied) => Some(note(denied.warning(), TEXT_WARNING)),
            None => self.recorder_line(
                Recorder::Action(index),
                capturing,
                tr!(
                    "F1–F12, or Ctrl or Alt with a letter or digit · Backspace for no key · Esc \
                     keeps the old one"
                ),
                None,
            ),
        };
        let kind = ACTION_KINDS
            .iter()
            .position(|&(kind, _)| kind == row.draft.kind)
            .unwrap_or(0);
        div()
            .id(("action", index))
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(segmented(
                        "kind",
                        ACTION_KINDS.map(|(_, label)| SharedString::from(label())),
                        kind,
                        cx.listener(move |view, pick: &usize, _, cx| {
                            if let Some(&(kind, _)) = ACTION_KINDS.get(*pick) {
                                view.set_action_kind(index, kind, cx);
                            }
                        }),
                    ))
                    .child(div().flex().flex_1().min_w_0().child(row.field.clone()))
                    .child(self.render_recorder(
                        Recorder::Action(index),
                        row.draft.hotkey,
                        tr!("no key"),
                        window,
                        cx,
                    ))
                    .child(icon_button(
                        "remove",
                        "×",
                        true,
                        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                            view.remove_action(index, cx);
                        }),
                    )),
            )
            .children(line)
            .into_any_element()
    }

    fn render_xp_overlay(
        &self,
        settings: &Settings,
        face: &'static NameFont,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let window = settings::XP_RATE_WINDOWS
            .iter()
            .position(|&minutes| minutes == settings.xp_rate_window_minutes)
            .unwrap_or(0);
        let windows = settings::XP_RATE_WINDOWS.map(|minutes| {
            SharedString::from(i18n::duration(Duration::from_secs(u64::from(minutes) * 60)))
        });
        group(
            face,
            tr!("XP line"),
            [
                toggle_row(
                    "xp-overlay",
                    tr!("Show the XP overlay"),
                    Some(tr!(
                        "Experience rate and time to the next level, above the flask panel"
                    )),
                    settings.xp_overlay,
                    |settings| &mut settings.xp_overlay,
                    cx,
                ),
                toggle_row(
                    "xp-percent",
                    tr!("Level percentage"),
                    Some(tr!(
                        "How much of the level is done; a pause always shows it"
                    )),
                    settings.xp_show_percent,
                    |settings| &mut settings.xp_show_percent,
                    cx,
                ),
                toggle_row(
                    "xp-map-timer",
                    tr!("Map timer"),
                    Some(tr!(
                        "Time in the current map, the experience it gave and the session's \
                         average map time, above the skill panel"
                    )),
                    settings.xp_map_timer,
                    |settings| &mut settings.xp_map_timer,
                    cx,
                ),
                setting_row(
                    tr!("Rate smoothing"),
                    [note(
                        tr!(
                            "The rate weighs mostly the last few minutes: shorter shows a change \
                             of farm sooner, longer reads steadier"
                        ),
                        TEXT_DIM,
                    )],
                    segmented(
                        "rate-window",
                        windows,
                        window,
                        cx.listener(|view, index: &usize, _, cx| {
                            if let Some(&minutes) = settings::XP_RATE_WINDOWS.get(*index) {
                                view.change(cx, |settings| {
                                    settings.xp_rate_window_minutes = minutes;
                                });
                            }
                        }),
                    ),
                ),
            ],
        )
    }

    /// What the lookup of the private league's typed name found, under its field.
    fn league_check_note(&self) -> Option<AnyElement> {
        let (text, color) = match &self.league_check {
            LeagueCheck::Idle => return None,
            LeagueCheck::Checking => (
                tr!("Looking the league up on pathofexile.com…").to_owned(),
                TEXT_DIM,
            ),
            LeagueCheck::Found { corrected: false } => {
                (tr!("The league is on pathofexile.com").to_owned(), TEXT_DIM)
            }
            LeagueCheck::Found { corrected: true } => (
                tr!("Found on pathofexile.com: the name is now as the site writes it").to_owned(),
                TEXT_DIM,
            ),
            LeagueCheck::Missing(name) => (
                tr!(
                    "pathofexile.com has no private league “{name}”: check the name, capital \
                     letters too",
                    name = name
                ),
                TEXT_WARNING,
            ),
            LeagueCheck::Unchecked { taken: true } => (
                tr!("pathofexile.com couldn't be reached to check the name: it's taken as typed")
                    .to_owned(),
                TEXT_WARNING,
            ),
            LeagueCheck::Unchecked { taken: false } => (
                tr!(
                    "pathofexile.com couldn't be reached, and without its number (PL…) the trade \
                     site won't find the league: try again"
                )
                .to_owned(),
                TEXT_WARNING,
            ),
        };
        Some(note(text, color))
    }

    /// The pathofexile.com session: what the site says of it (`session`'s check), «Войти» (the
    /// sign-in window, `crate::login`) or «Выйти»; the private league -- the account's own, as
    /// pathofexile.com lists them, and the name typed in, looked up there -- and the public league
    /// its exchange prices come from.
    fn render_account(
        &self,
        settings: &Settings,
        face: &'static NameFont,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let status = cx
            .try_global::<SessionStatus>()
            .cloned()
            .unwrap_or_default();
        let login = cx.try_global::<Login>();
        let signed_in = status.signed_in();
        let signing_in = login.is_some_and(Login::is_open) && !signed_in;
        let (label, label_color, about) = session_line(&status, signing_in);
        let problem = login.and_then(Login::problem).map(login_problem);
        let buttons = div()
            .flex()
            .gap(px(8.))
            .when(!signed_in, |this| {
                this.child(button(
                    "sign-in",
                    tr!("Sign in"),
                    ButtonKind::Primary,
                    face,
                    |_: &MouseDownEvent, _: &mut Window, cx: &mut App| login::open(cx),
                ))
            })
            // «Выйти» forgets the app's copy only: the site stays signed in elsewhere.
            .when(status != SessionStatus::SignedOut, |this| {
                this.child(button(
                    "sign-out",
                    tr!("Sign out"),
                    ButtonKind::Secondary,
                    face,
                    |_: &MouseDownEvent, _: &mut Window, cx: &mut App| session::sign_out(cx),
                ))
            });
        let private = matches!(settings.league, LeagueChoice::Custom(_));
        let mine = {
            let leagues = self.app.read(cx).private_leagues();
            (signed_in && !leagues.is_empty()).then(|| {
                leagues
                    .iter()
                    .map(|league| league.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
        };
        let reference = match &settings.league {
            LeagueChoice::Custom(name) => {
                let app = self.app.read(cx);
                let market = league_chip::market_league(name, app.leagues(), app.private_leagues());
                (market != name.as_str())
                    .then(|| league_chip::league_name(market, app.league_names()).to_owned())
            }
            LeagueChoice::Auto | LeagueChoice::Named(_) => None,
        };
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(group(
                face,
                "pathofexile.com",
                [setting_row(
                    div().text_color(rgb(label_color)).child(label),
                    [Some(note(about, TEXT_DIM)), problem].into_iter().flatten(),
                    buttons,
                )],
            ))
            .child(group(
                face,
                tr!("Private league"),
                [setting_row(
                    tr!("League name"),
                    [
                        Some(note(
                            tr!(
                                "As on pathofexile.com; the number in brackets comes from there. \
                                 Enter looks the league up and makes it the league searched, and \
                                 the league menus keep it; an empty box forgets it"
                            ),
                            TEXT_DIM,
                        )),
                        self.league_check_note(),
                        mine.map(|leagues| {
                            note(
                                tr!(
                                    "Your leagues on pathofexile.com are in the league menus: \
                                     {leagues}",
                                    leagues = leagues
                                ),
                                TEXT_DIM,
                            )
                        }),
                        reference.map(|league| {
                            note(
                                tr!(
                                    "Exchange and poe2scout prices come from {league}: a private \
                                     league trades too little on the exchange",
                                    league = league
                                ),
                                TEXT_DIM,
                            )
                        }),
                        (private && !signed_in).then(|| {
                            note(
                                tr!("Without a sign-in the site won't answer searches in a \
                                     private league — sign in above"),
                                TEXT_WARNING,
                            )
                        }),
                    ]
                    .into_iter()
                    .flatten(),
                    div().flex().w(px(250.)).child(self.league_field.clone()),
                )],
            ))
    }

    fn render_help(&self, face: &'static NameFont, cx: &Context<Self>) -> impl IntoElement {
        let report = match &self.report {
            ReportState::Idle => None,
            ReportState::Writing => Some(note(tr!("Collecting the report…"), TEXT_DIM)),
            ReportState::Written(path) => Some(note(
                tr!(
                    "Saved: {path} — attach it to your bug report",
                    path = path.display()
                ),
                TEXT,
            )),
            ReportState::Failed(error) => Some(note(
                tr!("Couldn't collect the report: {error}", error = error),
                TEXT_WARNING,
            )),
        };
        let notices = self.notices.clone();
        group(
            face,
            tr!("Help"),
            [
                setting_row(
                    tr!("Tutorial"),
                    [note(
                        tr!("A short tour of the settings, the price panel and the XP overlay"),
                        TEXT_DIM,
                    )],
                    button(
                        "tour",
                        tr!("Replay"),
                        ButtonKind::Secondary,
                        face,
                        cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            // Its first stop is the league, in Общие.
                            view.show(Section::General, cx);
                            tour::start(&view.app, cx);
                        }),
                    ),
                ),
                setting_row(
                    tr!("Report a bug"),
                    [
                        Some(note(
                            tr!("A form on GitHub. The report is the logs, settings and \
                                 unrecognised items in one archive on your desktop, with your \
                                 Windows user name hidden"),
                            TEXT_DIM,
                        )),
                        report,
                    ]
                    .into_iter()
                    .flatten(),
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(button(
                            "report",
                            tr!("Report ↗"),
                            ButtonKind::Secondary,
                            face,
                            cx.listener(|view, _: &MouseDownEvent, _, cx| view.report_bug(cx)),
                        ))
                        .child(button(
                            "diagnostics",
                            tr!("Collect report"),
                            ButtonKind::Secondary,
                            face,
                            cx.listener(|view, _: &MouseDownEvent, _, cx| view.write_report(cx)),
                        )),
                ),
                setting_row(
                    tr!("Logs folder"),
                    [note(
                        tr!("The app's log for this run and the one before"),
                        TEXT_DIM,
                    )],
                    button(
                        "logs",
                        tr!("Open"),
                        ButtonKind::Secondary,
                        face,
                        |_: &MouseDownEvent, _: &mut Window, _: &mut App| {
                            diagnostics::open_logs_folder();
                        },
                    ),
                ),
                setting_row(
                    tr!("About"),
                    [note(
                        tr!(
                            "PoE2 Oracle {version} · MIT or Apache-2.0 license",
                            version = VERSION
                        ),
                        TEXT_DIM,
                    )],
                    div()
                        .flex()
                        .gap(px(8.))
                        .children(notices.map(|notices| {
                            button(
                                "licenses",
                                tr!("Licenses"),
                                ButtonKind::Secondary,
                                face,
                                move |_: &MouseDownEvent, _: &mut Window, cx: &mut App| {
                                    cx.open_with_system(&notices);
                                },
                            )
                        }))
                        .child(button(
                            "github",
                            "GitHub ↗",
                            ButtonKind::Secondary,
                            face,
                            |_: &MouseDownEvent, _: &mut Window, cx: &mut App| {
                                cx.open_url(REPOSITORY_URL);
                            },
                        )),
                ),
            ],
        )
    }

    /// Resetting the waystone marks asks first: a dialog over the dimmed window.
    fn render_confirm(
        &self,
        marks: usize,
        face: &'static NameFont,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(alpha(0x000000, SCRIM_OPACITY))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _: &MouseDownEvent, _, cx| view.keep_marks(cx)),
            )
            .child(appear(
                "confirm",
                div()
                    .id("confirm")
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .w(px(420.))
                    .px(px(24.))
                    .pt(px(22.))
                    .pb(px(20.))
                    .bg(rgb(BG_CARD))
                    .shadow(modal_shadow())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        heading(face)
                            .text_size(px(18.))
                            .text_color(rgb(GOLD_LIGHT))
                            .child(tr!("Reset the marks?")),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(TEXT_DIM))
                            .child(tr!(
                                "All Waystone modifier marks ({count}) will be deleted. This \
                                 can't be undone.",
                                count = marks
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .pt(px(10.))
                            .child(button(
                                "keep",
                                tr!("Cancel"),
                                ButtonKind::Secondary,
                                face,
                                cx.listener(|view, _: &MouseDownEvent, _, cx| view.keep_marks(cx)),
                            ))
                            .child(button(
                                "reset",
                                tr!("Reset"),
                                ButtonKind::Danger,
                                face,
                                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                                    view.reset_waystone_marks(cx);
                                }),
                            )),
                    )
                    .child(game_frame()),
            ))
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let face = fonts::interface_font();
        let settings = &self.app.read(cx).settings;
        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                view.escape(event, window, cx);
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG_PANEL))
            .text_color(rgb(TEXT))
            .text_size(px(14.))
            .line_height(relative(1.4))
            .child(self.render_title_bar(face, cx))
            .child(appear(
                "open",
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(face, cx))
                    .child(self.render_content(settings, face, window, cx)),
            ))
            .when(self.confirming_reset, |this| {
                this.child(self.render_confirm(settings.waystone_marks.len(), face, cx))
            })
            .child(game_frame())
            .children(tour::layer(Host::Settings, window, cx))
    }
}

/// The settings window's title, as the taskbar and Alt+Tab show it.
pub fn window_title() -> &'static str {
    tr!("PoE2 Oracle — settings")
}

/// A group of rows under its heading.
fn group(
    face: &'static NameFont,
    title: &'static str,
    rows: impl IntoIterator<Item = AnyElement>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(section_heading(face, title))
        .child(card(rows))
}

/// A setting: its name and, under it, what it does on the left; its control on the right.
fn setting_row(
    label: impl IntoElement,
    lines: impl IntoIterator<Item = AnyElement>,
    control: impl IntoElement,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(24.))
        .px(px(14.))
        .py(px(12.))
        .child(label_block(label, lines))
        .child(div().flex_none().child(control))
        .into_any_element()
}

fn label_block(label: impl IntoElement, lines: impl IntoIterator<Item = AnyElement>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .flex_1()
        .min_w_0()
        .child(div().text_color(rgb(TEXT)).child(label))
        .children(lines)
}

/// A line under a setting's name: what it does, or what just happened, in its colour.
fn note(text: impl Into<SharedString>, color: u32) -> AnyElement {
    div()
        .text_size(px(12.))
        .text_color(rgb(color))
        .child(text.into())
        .into_any_element()
}

/// A setting that is on or off: the whole row flips it -- at once, like every control here --
/// lighting up under the pointer.
fn toggle_row(
    key: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    on: bool,
    flag: fn(&mut Settings) -> &mut bool,
    cx: &Context<SettingsView>,
) -> AnyElement {
    let row = div()
        .id(key)
        .flex()
        .items_center()
        .gap(px(24.))
        .px(px(14.))
        .py(px(12.))
        .rounded(px(6.))
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                view.change(cx, |settings| {
                    let value = flag(settings);
                    *value = !*value;
                });
            }),
        )
        .child(label_block(
            label,
            description.map(|description| note(description, TEXT_DIM)),
        ))
        .child(switch("switch", on));
    ease_hover(key, row, |row, hover| row.bg(alpha(GOLD, 0.05 * hover))).into_any_element()
}

/// What the account's row says of the session -- its label, the label's colour and the line
/// under it -- as the site's last answer has it, or while the sign-in window is up.
fn session_line(status: &SessionStatus, signing_in: bool) -> (SharedString, u32, SharedString) {
    if signing_in {
        return (
            tr!("Sign in through the window that opened").into(),
            TEXT,
            tr!("pathofexile.com's sign-in page; the window closes by itself once you're in")
                .into(),
        );
    }
    let kept =
        tr!("The session is kept in Windows' Credential Manager and sent to pathofexile.com only");
    match status {
        SessionStatus::SignedOut => (
            tr!("Not signed in").into(),
            TEXT,
            tr!("“Sign in” opens pathofexile.com's sign-in page — signing in through Steam works too")
                .into(),
        ),
        SessionStatus::Checking => (tr!("Checking the sign-in…").into(), TEXT_DIM, kept.into()),
        SessionStatus::SignedIn {
            account: Some(name),
        } => (
            tr!("Signed in as {name}", name = name).into(),
            TEXT,
            kept.into(),
        ),
        SessionStatus::SignedIn { account: None } => (tr!("Signed in").into(), TEXT, kept.into()),
        SessionStatus::Invalid => (
            tr!("Session expired").into(),
            TEXT_WARNING,
            tr!("The site no longer accepts this sign-in — sign in again").into(),
        ),
        SessionStatus::Unchecked(err) => (
            tr!("Couldn't check the sign-in").into(),
            TEXT_WARNING,
            tr!("Using the session as it is: {error}", error = err).into(),
        ),
    }
}

/// Why the last «Войти» didn't finish, under the account's status.
fn login_problem(problem: &LoginProblem) -> AnyElement {
    match problem {
        LoginProblem::NoRuntime => div()
            .flex()
            .flex_wrap()
            .gap_x(px(6.))
            .text_size(px(12.))
            .text_color(rgb(TEXT_WARNING))
            .child(tr!(
                "Signing in needs the Microsoft Edge WebView2 Runtime, and this computer doesn't \
                 have it."
            ))
            .child(link(
                "webview2",
                tr!("Download from Microsoft"),
                |_: &MouseDownEvent, _: &mut Window, cx: &mut App| cx.open_url(WEBVIEW2_DOWNLOAD),
            ))
            .into_any_element(),
        LoginProblem::Failed(err) => note(
            tr!("Couldn't open the sign-in window: {error}", error = err),
            TEXT_WARNING,
        ),
        LoginProblem::NotSaved(err) => note(
            tr!("Couldn't save the sign-in: {error}", error = err),
            TEXT_WARNING,
        ),
    }
}

/// Why a recorded combination didn't take: another program holds it. `kept` is the hotkey that
/// stays -- `None` for an action left without one.
fn taken_note(hotkey: Hotkey, kept: Option<Hotkey>) -> String {
    match kept {
        Some(kept) => tr!(
            "{hotkey} is taken by another program — {kept} stays",
            hotkey = hotkey,
            kept = kept
        ),
        None => tr!(
            "{hotkey} is taken by another program — the action has no key now",
            hotkey = hotkey
        ),
    }
}

/// A hotkey's keys as its keycaps name them: `Ctrl`, `E`.
fn hotkey_labels(hotkey: Hotkey) -> Vec<SharedString> {
    settings::modifier_names(hotkey.ctrl, hotkey.shift, hotkey.alt)
        .map(SharedString::new_static)
        .chain(std::iter::once(hotkey.key.to_string().into()))
        .collect()
}

/// A quick action's placeholder for its kind -- an example of what to type -- worded whenever the
/// field shows it, so it follows the interface language.
fn action_placeholder(kind: QuickActionKind) -> fn() -> &'static str {
    match kind {
        QuickActionKind::ChatCommand => || tr!("/hideout, /exit, @last thanks…"),
        QuickActionKind::StashSearch => || tr!("Search text, e.g. from poe2.re"),
    }
}

/// A language as the language choices name it: in its own words, whatever the interface's.
fn language_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Russian => "Русский",
        Lang::English => "English",
    }
}

/// The interface languages' choices: «Авто» naming the language it stands for (`auto`), then
/// each language in its own words.
fn interface_language_labels(auto: Lang) -> [SharedString; 3] {
    INTERFACE_LANGUAGES.map(|choice| match choice {
        InterfaceLanguage::Auto => tr!("Auto · {language}", language = language_name(auto)).into(),
        InterfaceLanguage::Russian => language_name(Lang::Russian).into(),
        InterfaceLanguage::English => language_name(Lang::English).into(),
    })
}

/// The notices file next to the running exe, where the installer puts it; `None` for a copy
/// that wasn't installed, such as a development build.
fn third_party_notices() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join(NOTICES_FILE);
    path.is_file().then_some(path)
}

/// What `new` changes from `old`, field by field as the settings file names them, for the log:
/// `ui_scale 1.0 -> 1.1, show_seller_column true -> false`.
fn changes(old: &Settings, new: &Settings) -> String {
    let (Ok(Value::Object(old)), Ok(Value::Object(new))) =
        (serde_json::to_value(old), serde_json::to_value(new))
    else {
        return String::new();
    };
    new.iter()
        .filter(|(field, value)| old.get(*field) != Some(*value))
        .map(|(field, value)| {
            let was = old.get(field).map_or_else(|| "-".to_owned(), shortened);
            format!("{field} {was} -> {}", shortened(value))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `value` as JSON, cut short: a long list of marks or actions would flood the log.
fn shortened(value: &Value) -> String {
    const LONGEST: usize = 160;
    let text = value.to_string();
    match text.char_indices().nth(LONGEST) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text,
    }
}

/// The hotkey a key-down in the recorder makes, or why it can't be one. Letters arrive as `a`-`z`
/// whatever the keyboard layout: gpui_windows names keys through
/// `MapVirtualKeyW(MAPVK_VK_TO_CHAR)`, which returns 'A'-'Z' for those keys "regardless of current
/// keyboard layout" (Microsoft's `MapVirtualKeyW` reference) -- so Ctrl+E pressed with the Russian
/// layout on is still the physical E key, the one `RegisterHotKey` will get.
fn recorded_hotkey(keystroke: &Keystroke) -> Result<Hotkey, &'static str> {
    let modifiers = keystroke.modifiers;
    if modifiers.platform {
        return Err(tr!("Combinations with the Windows key aren't supported"));
    }
    let (key, shift) = match KeyName::from_label(&keystroke.key) {
        Some(key) => (key, modifiers.shift),
        None => (
            shifted_digit().ok_or_else(|| tr!("Only letters A–Z, digits 0–9 and F1–F12 work"))?,
            true,
        ),
    };
    let hotkey = Hotkey {
        ctrl: modifiers.control,
        shift,
        alt: modifiers.alt,
        key,
    };
    hotkey
        .check()
        .map(|()| hotkey)
        .map_err(|problem| match problem {
            HotkeyProblem::NeedsModifier => {
                tr!("Letters and digits need Ctrl or Alt — otherwise the key would stop typing")
            }
            HotkeyProblem::CopyCombo => tr!("Ctrl+C is taken: the app copies items with it"),
            HotkeyProblem::CloseCombo => tr!("Alt+F4 closes windows, the game included"),
        })
}

/// The digit key held with Shift during the key-down being handled. gpui_windows reports
/// Shift+digit as the character it types, with Shift cleared -- "!" for Shift+1 on the US and
/// Russian layouts (`keyboard.rs`, `get_keystroke_key`) -- so the digit is read back from this
/// thread's key state, which `GetKeyState` keeps in step with the message being handled.
fn shifted_digit() -> Option<KeyName> {
    // SAFETY: `GetKeyState` only reads the calling thread's keyboard state; negative means down.
    let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
    if !down(VK_SHIFT.0) {
        return None;
    }
    let mut digits = (VK_0.0..=VK_9.0).filter(|&vk| down(vk));
    let digit = digits.next()?;
    // Two digits down at once name no single key.
    if digits.next().is_some() {
        return None;
    }
    KeyName::from_virtual_key(digit)
}

fn step_scale(settings: &mut Settings, up: bool) {
    let stepped = step_percent(
        scale_percent(settings.ui_scale),
        up,
        scale_percent(settings::MIN_UI_SCALE),
        scale_percent(settings::MAX_UI_SCALE),
    );
    settings.ui_scale = f32::from(stepped) / 100.;
}

fn scale_percent(scale: f32) -> u16 {
    (scale * 100.).round() as u16
}

/// The next multiple of [`STEP_PERCENT`] up or down from `value`, kept within `min..=max`: an
/// off-step value (a hand-edited 12) lands on the step either side of it (15 or 10).
fn step_percent(value: u16, up: bool, min: u16, max: u16) -> u16 {
    let stepped = if up {
        (value / STEP_PERCENT + 1) * STEP_PERCENT
    } else {
        value.saturating_sub(1) / STEP_PERCENT * STEP_PERCENT
    };
    stepped.clamp(min, max)
}
