//! End-to-end Price Check wire-up: the `Ctrl+E` global hotkey drives
//! `platform::clipboard_poll` + `platform::synth_input` to read a hovered item's clipboard text,
//! `item_parser::parse_clipboard` to parse it, `stat_filters::build_filters` to build a search
//! panel, and `trade_client::route_search` + the matching search/fetch/exchange call to populate
//! results. Windows-only (see `lib.rs`'s `#[cfg(target_os = "windows")]` gate on this module) --
//! every native dependency below (`platform::*`) only exists on that target.
//!
//! State lives on `PriceCheckApp`, the root view `ui::panel` renders. Orchestration (this
//! module) and presentation (`ui::panel`) are deliberately split, matching the
//! plan's own step 9/step 10 module boundary.
//!
//! Every async helper here takes `&Entity<PriceCheckApp>` (a STRONG reference), not
//! `WeakEntity` -- holding a strong `Entity<T>` for the duration of one pipeline run keeps it
//! alive by construction, so `Entity::update`/`read_with` are infallible plain-value calls here,
//! never `Result`-wrapped (unlike the `WeakEntity::upgrade()` the long-lived event tasks in
//! `register_hotkeys` still need, since THEY genuinely must tolerate the window closing).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, ensure};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, ClipboardItem, Context, Entity, FocusHandle,
    KeyDownEvent,
};
use http_client::HttpClient;
use item_parser::{ItemLanguage, ParseError};
use poe2_domain::{ItemRarity, ParsedItem, StatCatalog};
use trade_client::catalog::{ItemTypeEntry, StaticCurrency};
use trade_client::cx::{Market, MarketPrice};
use trade_client::private_leagues::{self, PrivateLeague};
use trade_client::rate_limit::RateLimiter;
use trade_client::rates::PriceUnit;
use trade_client::scout::ScoutPrices;
use trade_client::{
    AccountStatus, GroupedListing, League, ListedItem, ListingStatus, PriceCurrency, RarityFilter,
    SearchOutcome, SearchRoute, SearchScope, StatMatch, TradeApiError, TradeSite,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_SHIFT,
};

use crate::bound_input;
use crate::bug_report;
use crate::game_chat;
use crate::i18n;
use crate::item_refs::{self, RefKind};
use crate::league_chip;
use crate::overlay_layout::{self, PanelSide, PhysicalRect};
use crate::paths;
use crate::platform::game_window::{Foreground, GameScreen};
use crate::platform::{clipboard_poll, esc_hook, game_config, game_window, synth_input};
use crate::roll_slider::{self, Handle, Slider};
use crate::session::SessionStatus;
use crate::settings::{self, Hotkey, LeagueChoice, QuickAction, Settings, WaystoneMark};
use crate::tr;

/// Defaults matching `HostClipboard.ts`'s real, working constants (see `clipboard_poll`'s own
/// doc comment): 48ms initial delay and poll interval, 500ms total budget.
const CLIPBOARD_INITIAL_DELAY: Duration = Duration::from_millis(48);
const CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(48);
const CLIPBOARD_TIMEOUT: Duration = Duration::from_millis(500);

/// How long after a burst of foreground changes the hotkey re-checks where the foreground settled.
const FOREGROUND_SETTLE: Duration = Duration::from_millis(250);

/// How long a listing row says its whisper was copied.
const WHISPER_COPIED_SHOWN: Duration = Duration::from_secs(4);

/// How often a drag of the panel by its title bar looks at the pointer: each frame at 120 Hz.
const PANEL_DRAG_POLL: Duration = Duration::from_millis(8);

/// Up to 10 listings per `fetch` request -- the trade API's own per-request limit -- and one
/// request per search. EE2 fetches a second page (listings 10-20, `trade-api.ts`), but two
/// fetches a check made fetches the tighter budget (50 per 5 minutes is 25 checks, against 30
/// searches), shared with the player's browser on the same IP: a refused fetch locked the owner
/// out for ten minutes on 2026-09-23. The ten cheapest listings already show the price.
const FETCH_PAGE_SIZE: usize = 10;
const FETCH_PAGES: usize = 1;

/// Longest trade API rate-limit wait a search sits out before sending. A longer one means the API
/// has restricted this IP (a live 429 carried `Retry-After: 259`), and the search reports that
/// instead of silently hanging for minutes.
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(15);

/// How long a loaded exchange market is used before `fetch_market` is asked again: GGG's record
/// gains an hour a few minutes after each hour ends, and asking again costs at most that hour's
/// download (the hours already kept aren't fetched again; poe2scout's copy keeps half an hour).
const MARKET_MAX_AGE: Duration = Duration::from_secs(10 * 60);

/// How often the leagues and catalogs are looked at again while the app runs: a league starts and
/// a patch changes the trade site's data while the app keeps running.
const CATALOG_REFRESH: Duration = Duration::from_secs(3600);
/// How long a cached league list is used before the trade site is asked again.
const LEAGUES_MAX_AGE: Duration = Duration::from_secs(3600);
/// How long cached stat and item catalogs are used before the trade site is asked again.
const CATALOG_MAX_AGE: Duration = Duration::from_secs(6 * 3600);
/// Waits before retrying a first load that failed -- at sign-in the network may not be up yet
/// when autostart runs the app -- the last one repeating.
const BOOTSTRAP_RETRY: [Duration; 4] = [
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
];

/// One results-table row: a seller's listing (repeats at the same price folded in, EE2-style).
/// Presentation formatting (currency icons, relative "N days ago" time) is left to
/// `ui::panel` -- this is parsed data.
#[derive(Debug, Clone)]
pub struct ListingRow {
    pub price_amount: f64,
    pub price_currency: String,
    pub account_name: String,
    pub indexed: String,
    pub account_status: AccountStatus,
    pub instant_buyout: bool,
    /// How many times this seller listed it at this price (EE2's "× N").
    pub listed_times: u32,
    /// The message to the seller, ready for the game's chat; `None` for instant buyout.
    pub whisper: Option<String>,
    /// The listed item, drawn the way the game's tooltip draws it when the row is hovered --
    /// shared, so a hover hands it to its tooltip without a copy.
    pub item: Arc<ListedItem>,
}

impl From<GroupedListing> for ListingRow {
    fn from(group: GroupedListing) -> Self {
        let listing = group.listing;
        let (price_amount, price_currency) = listing.price.unwrap_or((0.0, String::new()));
        ListingRow {
            price_amount,
            price_currency,
            account_name: listing.account_name,
            indexed: listing.indexed,
            account_status: listing.account_status,
            instant_buyout: listing.instant_buyout,
            listed_times: group.listed_times,
            whisper: listing.whisper,
            item: Arc::new(listing.item),
        }
    }
}

/// Catalog/league bootstrap outcome -- gates whether the hotkey pipeline can do anything useful
/// at all.
pub enum BootstrapState {
    Loading,
    Ready,
    /// No usable catalog at all (first-ever run, offline) -- a blocking error state; the hotkey
    /// pipeline no-ops while in this state rather than attempting a parse against an empty
    /// catalog.
    Failed(String),
}

/// The current pricing outcome for whatever item is currently parsed.
pub enum SearchState {
    NotSearched,
    /// Sitting out the trade API's rate limit before the next request.
    RateLimiting {
        wait_secs: u64,
    },
    Searching,
    Failed(SearchFailure),
    /// Nothing matched; `relaxed` as in `Matched`, when that was the player's relaxed search.
    Empty {
        relaxed: Option<(u32, u32)>,
    },
    Matched {
        /// The search's own match count, listings beyond the fetched page included.
        total: u64,
        rows: Vec<ListingRow>,
        /// The trade site's own results page for this exact search, for the "trade ↗" link.
        trade_url: String,
        /// Found by the relaxed search the player asked for after nothing matched
        /// (`PriceCheckApp::search_one_fewer`): listings with at least `.0` of the `.1` stat rows
        /// asked for.
        relaxed: Option<(u32, u32)>,
    },
    /// A Currency Exchange item, priced by GGG's record of the exchange: the exchange is an
    /// auction the trade site's listings don't reflect (`trade_client::SearchRoute::Market`).
    Market(MarketPrice),
    /// An exchange item GGG's record doesn't price -- it didn't trade in the last hours, or the
    /// record is out of reach -- priced by poe2scout instead; its trade listings only when the
    /// player asks (`PriceCheckApp::search_listings`).
    Scouted {
        value: f64,
        unit: PriceUnit,
    },
}

/// What went wrong with a check, shown in place of an item. Kept as what happened rather than as
/// words: the panel says it in the interface language as it draws it ([`Problem::message`]), so a
/// change of language reaches it too.
#[derive(Debug)]
pub enum Problem {
    /// No item text came, and another program holds the copy combo named.
    ComboTaken(String),
    /// The parser rejected the item text -- a gamble offer too -- and where the text was kept
    /// for a report, once it is.
    Unparsed {
        error: ParseError,
        saved: Option<PathBuf>,
    },
}

impl Problem {
    /// The problem in the interface language.
    pub fn message(&self) -> String {
        match self {
            Problem::ComboTaken(combo) => tr!(
                "The game doesn't copy the item: another program takes {combo} — most often a \
                 graphics card overlay, a screen recorder or Discord. Free the shortcut in that \
                 program's settings.",
                combo = combo
            ),
            Problem::Unparsed { error, saved } => {
                let mut message = describe_parse_error(error);
                if let Some(path) = saved {
                    message.push_str("\n\n");
                    message.push_str(&tr!(
                        "The item's text was saved to {path}",
                        path = path.display()
                    ));
                }
                message
            }
        }
    }

    /// Whether it is an item the parser rejected: one to report (`PriceCheckApp::report_item`),
    /// unlike a gamble offer or a copy combo another program holds.
    pub fn reportable(&self) -> bool {
        matches!(self, Problem::Unparsed { error, .. } if *error != ParseError::Unrevealed)
    }
}

/// Why a search failed, kept as what happened like [`Problem`] and said in the interface language
/// as the panel draws it ([`SearchFailure::message`]).
#[derive(Debug)]
pub enum SearchFailure {
    /// The trade site restricted this IP: the seconds until it takes requests again, if it said.
    RateLimited(Option<u64>),
    /// The trade API refused the request, with its HTTP status and message.
    Refused { status: u16, message: String },
    /// The site found the query too complex (`TradeApiError::is_too_complex`). With
    /// `weighted_sums` it searched a checked «сумма» row, which the site takes only from a
    /// signed-in account (`stat_filters::Session`): the sign-in the app sent, if any, isn't
    /// accepted any more.
    TooComplex { weighted_sums: bool },
    /// The request never got through -- no network, no DNS, a refused or dropped connection: the
    /// player's connection, not the search. With the error's own words.
    Unreachable(String),
    /// Anything else, with the error's own words.
    Other(String),
}

impl SearchFailure {
    /// What a failed search's `err` means for the player; `weighted_sums`: whether the search had
    /// a checked weighted sum.
    fn of(err: &anyhow::Error, weighted_sums: bool) -> Self {
        if let Some(RateLimitedFor(wait)) = err.downcast_ref::<RateLimitedFor>() {
            return SearchFailure::RateLimited(Some(wait.as_secs()));
        }
        if let Some(api) = err.downcast_ref::<TradeApiError>() {
            if api.is_rate_limited() {
                return SearchFailure::RateLimited(api.retry_after_secs);
            }
            if api.is_too_complex() {
                return SearchFailure::TooComplex { weighted_sums };
            }
            return SearchFailure::Refused {
                status: api.status,
                message: api.message.clone(),
            };
        }
        if err
            .chain()
            .any(|cause| cause.downcast_ref::<std::io::Error>().is_some())
        {
            return SearchFailure::Unreachable(format!("{err:#}"));
        }
        SearchFailure::Other(format!("{err:#}"))
    }

    /// The failure in the interface language.
    pub fn message(&self) -> String {
        match self {
            SearchFailure::RateLimited(retry_after_secs) => rate_limit_message(*retry_after_secs),
            SearchFailure::Refused { status, message } => tr!(
                "The trade API refused the request (HTTP {status}): {message}",
                status = status,
                message = message
            ),
            SearchFailure::TooComplex {
                weighted_sums: true,
            } => tr!(
                "The trade site searches the “sum” rows only for a signed-in account, and it isn't \
                 accepting this app's sign-in now. Search without them, or sign in again in the \
                 settings, “Account” section."
            )
            .to_owned(),
            SearchFailure::TooComplex {
                weighted_sums: false,
            } => tr!(
                "The trade site finds this search too complex: uncheck some rows and search again. \
                 Signed in, the site allows more."
            )
            .to_owned(),
            SearchFailure::Unreachable(error) => tr!(
                "Can't reach the trade site — check your internet connection and try again.\n\n\
                 {error}",
                error = error
            ),
            SearchFailure::Other(error) => tr!("Search failed: {error}", error = error),
        }
    }
}

/// Per-filter-row UI state paired 1:1 with `PriceCheckApp.filters` (same index) -- the editable
/// min/max boxes, edited a keystroke at a time by `bound_input::type_key`. A bound the filter
/// doesn't set starts empty (by default the max, as in EE2), and the panel shows a placeholder
/// in its place.
pub struct FilterRowUi {
    pub min_focus: FocusHandle,
    pub max_focus: FocusHandle,
    pub min_text: String,
    pub max_text: String,
    /// Set when the input was just clicked into: the next keystroke replaces the whole value
    /// instead of appending to it -- EE2 selects the value on focus for the same reason.
    pub min_fresh: bool,
    pub max_fresh: bool,
}

impl FilterRowUi {
    fn new(cx: &mut Context<PriceCheckApp>, filter: &stat_filters::SearchFilter) -> Self {
        let bound_text = |bound: Option<f64>, dp: bool| {
            bound
                .map(|value| bound_input::format(value, dp))
                .unwrap_or_default()
        };
        let (min_text, max_text) = match &filter.roll {
            Some(roll) => (bound_text(roll.min, roll.dp), bound_text(roll.max, roll.dp)),
            None => (String::new(), String::new()),
        };
        Self {
            min_focus: cx.focus_handle(),
            max_focus: cx.focus_handle(),
            min_text,
            max_text,
            min_fresh: false,
            max_fresh: false,
        }
    }
}

/// A drag of the panel by its title bar (`PriceCheckApp::begin_panel_drag`): the pointer's x and
/// the panel's rect when it started, the side it's on, and the screen it's kept on.
#[derive(Debug, Clone, Copy)]
struct PanelDrag {
    cursor_x: i32,
    start: PhysicalRect,
    side: PanelSide,
    screen: GameScreen,
}

/// One trade site's localized catalogs: the stat templates `item-parser` matches mod lines
/// against, the exchange-tradable static items `route_search` matches currency-like items
/// against, the base types it recognizes a magic item's base in, and the leagues by their names
/// there. All are in the site's language, so a Russian item needs the Russian site's set.
#[derive(Default)]
struct SiteCatalog {
    stats: StatCatalog,
    currencies: Vec<StaticCurrency>,
    item_types: Vec<ItemTypeEntry>,
    leagues: Vec<League>,
}

/// An item's two ways to scope its filtered search -- by its category and by its base type --
/// when it has both (`trade_client::switched_scope`), and which one the player chose: EE2's
/// item-type chip. A rare goes by its category by default, which misses what its base is worth.
#[derive(Debug, Clone)]
pub struct ScopeChoice {
    default: SearchScope,
    switched: SearchScope,
    /// The player chose the other scope. Reset for every new item.
    pub switched_on: bool,
}

impl ScopeChoice {
    /// The scope the search goes by now.
    pub fn current(&self) -> &SearchScope {
        if self.switched_on {
            &self.switched
        } else {
            &self.default
        }
    }
}

/// The rarity a non-unique item's filtered search admits: the default `trade_client` chose (the
/// item's own rarity: a magic item among magic ones, a rare among rares) or the other one
/// (`trade_client::other_rarity`) -- the panel's rarity chip, PoE Overlay II's rarity toggle.
#[derive(Debug, Clone, Copy)]
pub struct RarityChoice {
    default: RarityFilter,
    other: RarityFilter,
    /// The player chose the other rarity. Reset for every new item.
    pub switched_on: bool,
}

impl RarityChoice {
    /// The rarity the search admits now.
    pub fn current(&self) -> RarityFilter {
        if self.switched_on {
            self.other
        } else {
            self.default
        }
    }
}

/// A yes/no trade filter the item's own state sets (`trade_client::MiscChoices`) -- corrupted or
/// not, unidentified -- which the player can drop for the next search, taking listings either
/// way: the panel's corruption and identification chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateChoice {
    /// The state the search asks for.
    pub value: bool,
    /// Whether the search asks for it at all. Reset for every new item.
    pub on: bool,
}

impl StateChoice {
    /// The search matches `value`, as it does by default.
    fn matched(value: bool) -> Self {
        StateChoice { value, on: true }
    }
}

pub struct PriceCheckApp {
    http_client: Arc<dyn HttpClient>,
    league: String,
    /// Every league the trade site lists, current first -- what the settings window offers and
    /// `Settings::league` resolves against.
    leagues: Vec<String>,
    /// The signed-in account's PoE 2 private leagues, as pathofexile.com lists them
    /// (`refresh_private_leagues`): on offer in the league menus, each priced by the public league
    /// its page names. Empty while signed out.
    private_leagues: Vec<PrivateLeague>,
    /// The player's settings as they apply: as saved, unless the file didn't take the last write
    /// (`save_failure`).
    pub settings: Settings,
    /// Why the settings file didn't take the last write (`save_settings`); `None` once one gets
    /// through.
    save_failure: Option<String>,
    /// Registers the hotkeys (`register_hotkeys`) for the app's lifetime.
    hotkeys: Option<GlobalHotKeyManager>,
    /// The combinations registered right now (`sync_hotkey_registration`): the price check's
    /// while the game or this app's panel is in front, the quick actions' while the game is.
    registered: Vec<Hotkey>,
    /// Combinations the system refused (another program holds them), each logged once.
    refused: Vec<Hotkey>,
    /// The open settings window, if any: opening it again brings this one forward, and the
    /// hotkeys are released meanwhile -- its recorders take key combinations themselves.
    settings_window: Option<AnyWindowHandle>,
    international: SiteCatalog,
    russian: SiteCatalog,
    /// One per `Endpoint`, indexed by it.
    limiters: [RateLimiter; 2],
    advanced_mod_desc_key: VIRTUAL_KEY,

    pub bootstrap: BootstrapState,
    pub item: Option<ParsedItem>,
    /// What went wrong with the last check -- an item the parser rejected, a copy combo another
    /// program holds -- shown in place of an item.
    pub problem: Option<Problem>,
    pub filters: Vec<stat_filters::SearchFilter>,
    pub filter_ui: Vec<FilterRowUi>,
    /// How the rows were set up (`choose_profile`): the item's own default profile
    /// (`SearchProfile::default_for`) when it opens. `None` for an item its search doesn't price
    /// by the rows: an exchange item, or one searched by its exact type.
    pub profile: Option<stat_filters::SearchProfile>,
    /// The row whose roll slider handle the player is dragging (`begin_roll_drag`).
    pub roll_drag: Option<usize>,
    pub search: SearchState,
    pub visible: bool,
    /// The title bar's league menu is open (`ui::panel::title_bar`).
    pub league_menu: bool,
    /// The profile select's menu is open (`ui::panel::results`).
    pub profile_menu: bool,
    /// The item trades on the Currency Exchange: priced from the market alone -- no filters, no
    /// trade search (`SearchRoute::Market`).
    pub priced_by_market: bool,
    /// Where the panel belongs for the current check (EE2 placement, or where the player dragged
    /// it on that side; physical pixels) -- the app's window wrapper (`app.rs`'s
    /// `PriceCheckRoot`) moves the OS window here. `None` until the first check.
    pub placement: Option<PhysicalRect>,
    /// The side of the game `placement` is on: the side whose place a drag of the panel sets.
    panel_side: Option<PanelSide>,
    /// The drag of the panel by its title bar under way (`begin_panel_drag`).
    panel_drag: Option<PanelDrag>,
    /// Checks shown so far: each one plays the panel's appearance again (`ui::panel`).
    pub appearances: u64,
    /// Which sellers the search asks for -- the trade site's "Instant Buyout" / "In Person"
    /// choice, PoE Overlay II's status dropdown. Starts at the settings' default sellers for every
    /// new item.
    pub listing_status: ListingStatus,
    /// The currency listings' prices must be in; kept from check to check, like EE2's.
    pub price_currency: PriceCurrency,
    /// Whether the rows EE2 hides (`SearchFilter::hidden`) are listed -- its "Hidden" toggle,
    /// PoE Overlay II's "show hidden mods". Reset for every new item.
    pub show_hidden: bool,
    /// The item's search by its category or by its base type, when it can go by either.
    pub scope: Option<ScopeChoice>,
    /// The item's corruption the search matches (`trade_client::MiscChoices::corrupted`): an
    /// uncorrupted item among listings that can still be modified, a corrupted one among
    /// corrupted ones -- or, dropped by the player, either. `None` where the search takes either.
    pub corruption: Option<StateChoice>,
    /// An unidentified item's search among unidentified listings
    /// (`trade_client::MiscChoices::identified`), or, dropped by the player, identified ones too.
    pub identification: Option<StateChoice>,
    /// The rarity a non-unique item's search admits, where it can admit two.
    pub rarity: Option<RarityChoice>,
    /// The player asked for an exchange item's trade listings where poe2scout priced it
    /// (`search_listings`): its searches go to the trade site. Reset for every new item.
    listings_wanted: bool,
    /// The next search is the relaxed one the player asked for after nothing matched
    /// (`search_one_fewer`): all the checked stat rows but one.
    one_fewer_next: bool,
    /// The last checked item's text, as the game copied it -- parsed or not: what a report of a
    /// misread or mispriced item carries (`report_item`).
    item_text: Option<String>,
    /// The listing whose whisper was just copied -- the search generation that listed it and its
    /// row -- which the row says for a few seconds.
    copied_whisper: Option<(u64, usize)>,
    /// Bumped by every search: only the newest one may write `search` -- a slower, older search
    /// (an earlier item, or filters since edited) must not overwrite what the panel now shows.
    search_generation: u64,
    /// Recent searches' listings by `search_key`, newest last, each with when it arrived: a
    /// repeated search within `SEARCH_CACHE_TTL` is answered without the trade API.
    search_cache: Vec<(String, Instant, SearchResults)>,

    /// Site the displayed item was parsed for; its searches go to the same site, because the
    /// `Exact` search and the exchange catalog `Market` routing match localized names.
    site: TradeSite,
    /// The league's exchange market -- GGG's prices of exchange items and poe2scout's of the rest,
    /// and the rates listing prices are normalized with -- and when it was loaded
    /// (reloaded after `MARKET_MAX_AGE`). A private league's is its reference league's
    /// (`market_league`).
    market: Option<(Market, Instant)>,
    /// The league's poe2scout prices of uniques, which the exchange doesn't trade, and when they
    /// were loaded (reloaded after `MARKET_MAX_AGE`, like the market); a private league's, its
    /// reference league's.
    scout: Option<(ScoutPrices, Instant)>,
}

impl PriceCheckApp {
    /// Placeholder state shown immediately when the window opens, before `bootstrap` resolves.
    fn loading(http_client: Arc<dyn HttpClient>, settings: Settings) -> Self {
        Self {
            http_client,
            league: String::new(),
            leagues: Vec::new(),
            private_leagues: Vec::new(),
            listing_status: settings.listing_status.into(),
            price_currency: PriceCurrency::Any,
            settings,
            save_failure: None,
            hotkeys: None,
            registered: Vec::new(),
            refused: Vec::new(),
            settings_window: None,
            international: SiteCatalog::default(),
            russian: SiteCatalog::default(),
            limiters: Default::default(),
            advanced_mod_desc_key: VIRTUAL_KEY(game_config::read().advanced_mod_desc_key),
            bootstrap: BootstrapState::Loading,
            item: None,
            problem: None,
            filters: Vec::new(),
            filter_ui: Vec::new(),
            profile: None,
            roll_drag: None,
            search: SearchState::NotSearched,
            visible: false,
            league_menu: false,
            profile_menu: false,
            priced_by_market: false,
            placement: None,
            panel_side: None,
            panel_drag: None,
            appearances: 0,
            show_hidden: false,
            scope: None,
            corruption: None,
            identification: None,
            rarity: None,
            listings_wanted: false,
            one_fewer_next: false,
            item_text: None,
            copied_whisper: None,
            search_generation: 0,
            search_cache: Vec::new(),
            site: TradeSite::International,
            market: None,
            scout: None,
        }
    }

    /// The current league name, for the panel header. Empty until `bootstrap` resolves.
    pub fn league(&self) -> &str {
        &self.league
    }

    /// Every league the trade site lists, current first. Empty until `bootstrap` resolves.
    pub fn leagues(&self) -> &[String] {
        &self.leagues
    }

    /// The signed-in account's PoE 2 private leagues; empty while signed out or until they load.
    pub fn private_leagues(&self) -> &[PrivateLeague] {
        &self.private_leagues
    }

    /// The trade site's leagues named in the interface language (`i18n::lang`): the Russian
    /// site's list for a Russian interface, the international one's otherwise.
    pub fn league_names(&self) -> &[League] {
        &self.catalog(i18n::lang().trade_site()).leagues
    }

    /// The league whose exchange market and poe2scout prices stand for the current one's: itself,
    /// or for a private league the public one it's made from (`league_chip::market_league`).
    pub fn market_league(&self) -> &str {
        league_chip::market_league(&self.league, &self.leagues, &self.private_leagues)
    }

    /// The public league a private one's prices come from, named in the interface language;
    /// `None` while the league prices by its own market.
    pub fn reference_league_name(&self) -> Option<&str> {
        let market = self.market_league();
        (market != self.league).then(|| league_chip::league_name(market, self.league_names()))
    }

    /// The open settings window, if any.
    pub fn settings_window(&self) -> Option<AnyWindowHandle> {
        self.settings_window
    }

    /// Records the settings window opening (`Some`) or closing (`None`). The hotkey follows at
    /// once: released while the window is open, so its recorder can take the combination, and
    /// held again after.
    pub fn set_settings_window(&mut self, window: Option<AnyWindowHandle>) {
        self.settings_window = window;
        self.sync_hotkey_registration(game_window::foreground());
    }

    /// The app's side of the diagnostics report (`diagnostics::write_report`): what it priced
    /// against and what state the hotkey and the last check are in. The settings file goes into
    /// the report whole, so they aren't repeated here.
    pub fn diagnostics_summary(&self) -> String {
        let catalog = match &self.bootstrap {
            BootstrapState::Loading => "loading".to_owned(),
            BootstrapState::Ready => {
                let counts = |catalog: &SiteCatalog| {
                    format!(
                        "{} stats, {} exchange items, {} base types",
                        catalog.stats.stats.len(),
                        catalog.currencies.len(),
                        catalog.item_types.len()
                    )
                };
                format!(
                    "ready -- international: {}; russian: {}",
                    counts(&self.international),
                    counts(&self.russian)
                )
            }
            BootstrapState::Failed(err) => format!("failed: {err}"),
        };
        let market = match &self.market {
            Some((market, loaded)) => format!(
                "loaded {} min ago, 1 divine = {:.1} exalted = {:.2} chaos",
                loaded.elapsed().as_secs() / 60,
                market.exalted_per_divine,
                market.chaos_per_divine
            ),
            None => "not loaded".to_owned(),
        };
        let hotkey = format!(
            "{}; held: {}{}; refused (another program holds them): {}",
            self.settings.hotkey,
            hotkey_list(&self.registered),
            if self.settings_window.is_some() {
                " (the settings window is open)"
            } else {
                ""
            },
            hotkey_list(&self.refused)
        );
        let last_check = match (&self.item, &self.problem) {
            (Some(item), _) => format!(
                "{} / {} [{}] on the {:?} site",
                item.name,
                item.base_type.as_deref().unwrap_or("-"),
                item.category
                    .as_ref()
                    .map_or("-", |category| category.id.as_str()),
                self.site
            ),
            (None, Some(problem)) => format!("not parsed: {}", problem.message()),
            (None, None) => "none yet".to_owned(),
        };
        format!(
            "[app]\nleague: {}\nleagues listed: {}\ncatalog: {catalog}\nmarket: {market}\n\
             hotkey: {hotkey}\nlast check: {last_check}\nsearches cached: {}\n",
            self.league,
            self.leagues.join(", "),
            self.search_cache.len()
        )
    }

    /// Shows `problem` in the panel in place of an item: what went wrong with the check.
    fn show_problem(&mut self, problem: Problem) {
        self.item = None;
        self.priced_by_market = false;
        self.filters.clear();
        self.filter_ui.clear();
        self.profile = None;
        self.profile_menu = false;
        self.roll_drag = None;
        self.problem = Some(problem);
        self.search = SearchState::NotSearched;
    }

    /// Takes over the settings the player just changed, for the caller to write as taken
    /// (`save_settings`). The hotkeys and the league change at once; listing status and client
    /// language apply from the next check, as in EE2. A hotkey another program already holds is
    /// refused -- the price check keeps its old one, a quick action goes without -- so the file
    /// never names a dead combination.
    pub fn apply_settings(&mut self, new: Settings, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.settings, new);
        self.refuse_taken_hotkeys(&old);
        self.sync_hotkey_registration(game_window::foreground());
        self.follow_league(cx);
        if self.settings.interface_language != old.interface_language
            && crate::i18n::apply(self.settings.interface_language)
        {
            cx.refresh_windows();
        }
        cx.notify();
    }

    /// Writes the settings as they stand. What changed applies whether or not the file takes it;
    /// a write it didn't take would be lost at the next launch, so the settings window says so
    /// (`save_failure`) until a later one gets through -- with the system's own reason, in the
    /// system's language; the log keeps the whole chain, file path included.
    pub fn save_settings(&mut self, cx: &mut Context<Self>) {
        let failure = settings::save(&self.settings).err().map(|err| {
            log::warn!("saving the settings failed: {err:#}");
            err.root_cause().to_string()
        });
        if self.save_failure != failure {
            self.save_failure = failure;
            cx.notify();
        }
    }

    /// Why the settings file didn't take the last write, until one gets through.
    pub fn save_failure(&self) -> Option<&str> {
        self.save_failure.as_deref()
    }

    /// Takes the league the player picked in the title bar's league menu: into the settings,
    /// saved at once, and followed (`follow_league`) -- which searches again only when the pick
    /// moves searches to another league.
    pub fn choose_league(&mut self, choice: LeagueChoice, cx: &mut Context<Self>) {
        self.league_menu = false;
        if choice != self.settings.league {
            self.settings.league = choice;
            self.save_settings(cx);
            self.follow_league(cx);
        }
        cx.notify();
    }

    /// Opens or closes the title bar's league menu.
    pub fn set_league_menu(&mut self, open: bool, cx: &mut Context<Self>) {
        self.league_menu = open;
        cx.notify();
    }

    /// Loads the signed-in account's private leagues from pathofexile.com
    /// (`trade_client::private_leagues::mine`): when the site accepts the session, and whenever
    /// the settings window opens -- the player may have joined one since. Signed out, they're
    /// forgotten. Should the league searched be one of them, priced by another public league than
    /// its name suggested, its market loads again from that one.
    pub fn refresh_private_leagues(&mut self, cx: &mut Context<Self>) {
        let signed_in = matches!(
            cx.try_global::<SessionStatus>(),
            Some(SessionStatus::SignedIn { .. })
        );
        if !signed_in {
            if !self.private_leagues.is_empty() {
                self.private_leagues.clear();
                cx.notify();
            }
            return;
        }
        let client = self.http_client.clone();
        cx.spawn(async move |this, cx| {
            let leagues = match private_leagues::mine(&client).await {
                Ok(leagues) => leagues,
                Err(err) => {
                    log::warn!("loading the account's private leagues failed: {err:#}");
                    return;
                }
            };
            log::info!("private leagues on pathofexile.com: {}", leagues.len());
            let _ = this.update(cx, |app, cx| {
                if app.private_leagues == leagues {
                    return;
                }
                let market = |app: &PriceCheckApp| {
                    league_chip::market_league(&app.league, &app.leagues, &app.private_leagues)
                        .to_owned()
                };
                let before = market(app);
                app.private_leagues = leagues;
                let after = market(app);
                if after != before {
                    log::info!(
                        "{} is private: its exchange items are priced by {after}",
                        app.league
                    );
                    app.market = None;
                    app.scout = None;
                    app.spawn_market_load(false, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Makes the league the settings resolve to the one searches go to, when it isn't already:
    /// its exchange market and poe2scout's prices load, and the item the panel shows is priced in
    /// it again -- once. A hidden panel's item isn't: the next check searches anew anyway.
    fn follow_league(&mut self, cx: &mut Context<Self>) {
        let Some(league) = self.settings.league.resolve(&self.leagues) else {
            return;
        };
        if league == self.league {
            return;
        }
        if !self.league.is_empty() {
            log::info!("league {} -> {league}", self.league);
        }
        let market = league_chip::market_league(league, &self.leagues, &self.private_leagues);
        if market != league {
            log::info!("{league} is private: its exchange items are priced by {market}");
        }
        self.league = league.to_owned();
        self.market = None;
        self.scout = None;
        let reprice = self.visible && self.item.is_some();
        // A trade search needs the market only for its rows' exchange-rate equivalents, after its
        // listings: it runs alongside the load. An exchange item's price is that market, so its
        // search follows the load rather than loading the market a second time.
        if reprice && !self.priced_by_market {
            self.spawn_search(cx);
        }
        self.spawn_market_load(reprice && self.priced_by_market, cx);
    }

    /// Tries each combination the settings brought in (one `old` didn't have) against the rest of
    /// the system, and takes back those another program holds: the price check's goes back to
    /// `old`'s, a quick action's to none. Everything this app holds is released first, so only
    /// other programs answer; `sync_hotkey_registration` holds what's wanted again after.
    fn refuse_taken_hotkeys(&mut self, old: &Settings) {
        let Some(manager) = &self.hotkeys else {
            return;
        };
        for hotkey in self.registered.drain(..) {
            let _ = manager.unregister(hotkey.to_global());
        }
        let known: Vec<Hotkey> = std::iter::once(old.hotkey)
            .chain(old.quick_actions.iter().filter_map(|action| action.hotkey))
            .collect();
        let free = |hotkey: Hotkey| {
            known.contains(&hotkey)
                || match manager.register(hotkey.to_global()) {
                    Ok(()) => {
                        let _ = manager.unregister(hotkey.to_global());
                        true
                    }
                    Err(err) => {
                        log::warn!("the {hotkey} hotkey is taken by another program: {err}");
                        false
                    }
                }
        };
        if !free(self.settings.hotkey) {
            self.settings.hotkey = old.hotkey;
        }
        for action in &mut self.settings.quick_actions {
            // The price check's hotkey may just have gone back to one an action now has.
            if let Some(hotkey) = action.hotkey
                && (hotkey == self.settings.hotkey || !free(hotkey))
            {
                action.hotkey = None;
            }
        }
    }

    /// Holds each hotkey only while its keys are for this app, and releases it otherwise -- a
    /// registered hotkey is global and swallows its keys wherever they're pressed. The price
    /// check's: the game in front, or this app's panel (the player clicked into it) with no
    /// settings window open. The quick actions': the game in front only, since they type into
    /// it. Anywhere else -- a browser, a chat -- the combinations are the other program's, as
    /// with EE2's in-game-only shortcuts, and the settings window's recorders can take them too.
    fn sync_hotkey_registration(&mut self, foreground: Foreground) {
        let price_check = match foreground {
            Foreground::Game => true,
            Foreground::ThisApp => self.settings_window.is_none(),
            Foreground::Other => false,
        };
        let mut wanted = Vec::new();
        if price_check {
            wanted.push(self.settings.hotkey);
        }
        if foreground == Foreground::Game {
            wanted.extend(
                self.settings
                    .quick_actions
                    .iter()
                    .filter_map(|action| action.hotkey),
            );
        }
        let Some(manager) = &self.hotkeys else {
            return;
        };
        let mut released = Vec::new();
        self.registered.retain(|&hotkey| {
            if wanted.contains(&hotkey) {
                return true;
            }
            if let Err(err) = manager.unregister(hotkey.to_global()) {
                log::warn!("releasing the {hotkey} hotkey failed: {err}");
            }
            released.push(hotkey);
            false
        });
        let mut held = Vec::new();
        for hotkey in wanted {
            if self.registered.contains(&hotkey) {
                continue;
            }
            match manager.register(hotkey.to_global()) {
                Ok(()) => {
                    self.registered.push(hotkey);
                    self.refused.retain(|&refused| refused != hotkey);
                    held.push(hotkey);
                }
                // Retried on the next change of foreground; said once.
                Err(err) if !self.refused.contains(&hotkey) => {
                    log::warn!("the {hotkey} hotkey is taken by another program: {err}");
                    self.refused.push(hotkey);
                }
                Err(_) => {}
            }
        }
        for (change, hotkeys) in [("held", held), ("released", released)] {
            if !hotkeys.is_empty() {
                log::info!(
                    "{} {change} ({foreground:?} in front)",
                    hotkey_list(&hotkeys)
                );
            }
        }
    }

    /// What a press of the hotkey with `id` is for. Nothing while the settings window is open:
    /// its recorders own key combinations then -- the current ones included.
    fn pressed(&self, id: u32) -> Option<Pressed> {
        if self.settings_window.is_some() {
            return None;
        }
        if id == self.settings.hotkey.to_global().id() {
            return Some(Pressed::PriceCheck);
        }
        self.settings
            .quick_actions
            .iter()
            .find(|action| {
                action
                    .hotkey
                    .is_some_and(|hotkey| hotkey.to_global().id() == id)
            })
            .cloned()
            .map(Pressed::QuickAction)
    }

    /// The listings a search with `key` got within `SEARCH_CACHE_TTL`, if any.
    fn cached_results(&self, key: &str) -> Option<SearchResults> {
        self.search_cache
            .iter()
            .rev()
            .find(|(cached, at, _)| cached == key && at.elapsed() < SEARCH_CACHE_TTL)
            .map(|(_, _, results)| results.clone())
    }

    /// Keeps a search's listings for `cached_results`, dropping the oldest beyond
    /// `SEARCH_CACHE_ENTRIES`.
    fn remember_results(&mut self, key: String, results: SearchResults) {
        self.search_cache.retain(|(cached, _, _)| *cached != key);
        self.search_cache.push((key, Instant::now(), results));
        if self.search_cache.len() > SEARCH_CACHE_ENTRIES {
            self.search_cache.remove(0);
        }
    }

    /// Loads (or refreshes) the current league's exchange market, then its poe2scout prices of
    /// uniques, in the background -- searching again in between when `then_search`, if the league
    /// is still this one: a search that prices the item from the market finds it loaded.
    fn spawn_market_load(&self, then_search: bool, cx: &mut Context<Self>) {
        let client = self.http_client.clone();
        let league = self.league.clone();
        cx.spawn(async move |this, cx| {
            let Some(view) = this.upgrade() else {
                return;
            };
            if let Err(err) = current_market(&view, cx, &client, &league).await {
                log::warn!("{err:#}");
            }
            if then_search && view.read_with(cx, |state, _| state.league == league) {
                run_search(&view, cx).await;
            }
            if let Err(err) = current_scout(&view, cx, &client, &league).await {
                log::warn!("{err:#}");
            }
        })
        .detach();
    }

    /// Takes in a load of both sites' catalogs, leagues included (`bootstrap_catalogs`). The
    /// first readies the app; a later one refreshes it and follows the league the settings
    /// resolve to, as when a new league starts and "current" moves to it. A failed refresh keeps
    /// what the app has; a failed first load stays `BootstrapState::Failed` until a retry
    /// succeeds.
    fn apply_bootstrap(
        &mut self,
        result: Result<(SiteCatalog, SiteCatalog)>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok((international, russian)) => {
                // Every site lists the same ids in the same order; the international one's are
                // what searches go by.
                self.leagues = international
                    .leagues
                    .iter()
                    .map(|league| league.id.clone())
                    .collect();
                self.international = international;
                self.russian = russian;
                self.bootstrap = BootstrapState::Ready;
                // The market loads up front: exchange items then price instantly, and the title
                // bar shows the divine rate from the start.
                self.follow_league(cx);
            }
            Err(err) => {
                log::warn!("loading the catalogs failed: {err:#}");
                if !matches!(self.bootstrap, BootstrapState::Ready) {
                    self.bootstrap = BootstrapState::Failed(format!("{err:#}"));
                }
            }
        }
        cx.notify();
    }

    /// Loads the catalogs and leagues again now, for a check the player made while the first
    /// load had failed; the refresh loop (`create_app`) keeps retrying meanwhile anyway.
    fn retry_bootstrap(&mut self, cx: &mut Context<Self>) {
        self.bootstrap = BootstrapState::Loading;
        let client = self.http_client.clone();
        cx.spawn(async move |this, cx| {
            let result = bootstrap_catalogs(&client).await;
            this.update(cx, |state, cx| state.apply_bootstrap(result, cx))
                .ok();
        })
        .detach();
    }

    /// poe2scout's price of `item` when it's an identified unique poe2scout prices, in the unit it
    /// reads best in (`ScoutPrices::unique_price`).
    pub fn scout_unique_price(&self, item: &ParsedItem) -> Option<(f64, PriceUnit)> {
        if item.rarity != Some(ItemRarity::Unique) {
            return None;
        }
        let (scout, _) = self.scout.as_ref()?;
        // poe2scout knows uniques by their English names; an unidentified one's name is its base.
        let name = item_refs::lookup(RefKind::Unique, &item.name)?.ref_name;
        scout.unique_price(name)
    }

    /// The trade site the displayed item was parsed for -- its language, too.
    pub fn trade_site(&self) -> TradeSite {
        self.site
    }

    /// The game client's language as far as the app knows it: the last checked item's, else the
    /// one the player set -- what a bug report says the client is.
    pub fn item_language(&self) -> Option<ItemLanguage> {
        if self.item_text.is_some() {
            Some(match self.site {
                TradeSite::International => ItemLanguage::English,
                TradeSite::Russian => ItemLanguage::Russian,
            })
        } else {
            self.settings.client_language.item_language()
        }
    }

    /// Opens the item problem form (`bug_report::item_problem_url`) for the last checked item --
    /// misread, or priced wrong -- with its text as the game copied it.
    pub fn report_item(&self, cx: &mut App) {
        let Some(text) = &self.item_text else {
            return;
        };
        let name = self
            .item
            .as_ref()
            .map_or(tr!("not recognized"), |item| item.name.as_str());
        cx.open_url(&bug_report::item_problem_url(
            self.item_language(),
            name,
            text,
        ));
    }

    /// Toggles one filter's checkbox. Does NOT trigger a re-search on its own (matches the
    /// reference's own lack of any debounce/auto-search-on-edit -- see `run_search`'s doc
    /// comment).
    pub fn toggle_filter(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(filter) = self.filters.get_mut(row) {
            filter.enabled = !filter.enabled;
            cx.notify();
        }
    }

    /// Checks every listed row a search can use, or -- when all already are -- unchecks them
    /// all: Sidekick's "check all / uncheck all". Rows EE2 hides stay as they are; like a
    /// checkbox, it takes effect with the next search.
    pub fn toggle_all_filters(&mut self, cx: &mut Context<Self>) {
        let listed = |filter: &stat_filters::SearchFilter| filter.searchable() && !filter.hidden;
        let check = !self
            .filters
            .iter()
            .filter(|filter| listed(filter))
            .all(|filter| filter.enabled);
        for filter in self.filters.iter_mut().filter(|filter| listed(filter)) {
            filter.enabled = check;
        }
        cx.notify();
    }

    /// Lists or folds away the rows EE2 hides.
    pub fn toggle_show_hidden(&mut self, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
        cx.notify();
    }

    /// Switches the search between the item's category and its base type (`ScopeChoice`); like a
    /// filter's checkbox, it takes effect with the next search.
    pub fn toggle_scope(&mut self, cx: &mut Context<Self>) {
        if let Some(choice) = &mut self.scope {
            choice.switched_on = !choice.switched_on;
            cx.notify();
        }
    }

    /// Drops the item's corruption from its search, or matches it again (`corruption`); takes
    /// effect with the next search.
    pub fn toggle_corruption(&mut self, cx: &mut Context<Self>) {
        if let Some(choice) = &mut self.corruption {
            choice.on = !choice.on;
            cx.notify();
        }
    }

    /// Lets identified listings into an unidentified item's search, or leaves them out again
    /// (`identification`); takes effect with the next search.
    pub fn toggle_identification(&mut self, cx: &mut Context<Self>) {
        if let Some(choice) = &mut self.identification {
            choice.on = !choice.on;
            cx.notify();
        }
    }

    /// Switches a non-unique item's search between its own rarity and every non-unique one
    /// (`RarityChoice`); takes effect with the next search.
    pub fn toggle_rarity(&mut self, cx: &mut Context<Self>) {
        if let Some(choice) = &mut self.rarity {
            choice.switched_on = !choice.switched_on;
            cx.notify();
        }
    }

    /// Steps the player's mark on a waystone modifier (by its trade stat id) through none,
    /// danger, warning and wanted, and saves it at once: marks outlive the check.
    pub fn cycle_waystone_mark(&mut self, stat_id: &str, cx: &mut Context<Self>) {
        let marks = &mut self.settings.waystone_marks;
        match WaystoneMark::next(marks.get(stat_id).copied()) {
            Some(mark) => {
                marks.insert(stat_id.to_owned(), mark);
            }
            None => {
                marks.remove(stat_id);
            }
        }
        self.save_settings(cx);
        cx.notify();
    }

    /// Steps the listing status through the choices the panel offers and re-searches with it.
    pub fn cycle_listing_status(&mut self, cx: &mut Context<Self>) {
        self.listing_status = match self.listing_status {
            ListingStatus::Available => ListingStatus::Securable,
            ListingStatus::Securable => ListingStatus::Online,
            ListingStatus::Online => ListingStatus::Any,
            ListingStatus::Any | ListingStatus::OnlineLeague => ListingStatus::Available,
        };
        self.spawn_search(cx);
        cx.notify();
    }

    /// Steps the price currency through EE2's choices and re-searches with it: any, exalted or
    /// divine (EE2's answer to prices fixed in rare currency) first, then each alone.
    pub fn cycle_price_currency(&mut self, cx: &mut Context<Self>) {
        self.set_price_currency(
            match self.price_currency {
                PriceCurrency::Any => PriceCurrency::ExaltedOrDivine,
                PriceCurrency::ExaltedOrDivine => PriceCurrency::Exalted,
                PriceCurrency::Exalted => PriceCurrency::Divine,
                PriceCurrency::Divine => PriceCurrency::Chaos,
                PriceCurrency::Chaos => PriceCurrency::Any,
            },
            cx,
        );
    }

    /// Searches again with prices only in `currency`.
    pub fn set_price_currency(&mut self, currency: PriceCurrency, cx: &mut Context<Self>) {
        self.price_currency = currency;
        self.spawn_search(cx);
        cx.notify();
    }

    /// A bound input was clicked into: its next keystroke replaces the value.
    pub fn begin_bound_edit(&mut self, row: usize, is_min: bool, cx: &mut Context<Self>) {
        if let Some(ui) = self.filter_ui.get_mut(row) {
            if is_min {
                ui.min_fresh = true;
            } else {
                ui.max_fresh = true;
            }
            cx.notify();
        }
    }

    /// The bound boxes lost the keyboard -- a press outside the focused box, or the panel losing
    /// the keyboard to the game: no value shows as selected any more, as in any field left.
    pub fn end_bound_edit(&mut self, cx: &mut Context<Self>) {
        for ui in &mut self.filter_ui {
            ui.min_fresh = false;
            ui.max_fresh = false;
        }
        cx.notify();
    }

    /// Edits filter `row`'s min (`is_min`) or max box with a keystroke (`bound_input::type_key`);
    /// `enter` re-searches.
    pub fn handle_filter_key(
        &mut self,
        row: usize,
        is_min: bool,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        if key == "enter" {
            self.spawn_search(cx);
            cx.notify();
            return;
        }
        let Some(ui) = self.filter_ui.get_mut(row) else {
            return;
        };
        let (text, fresh) = if is_min {
            (&mut ui.min_text, &mut ui.min_fresh)
        } else {
            (&mut ui.max_text, &mut ui.max_fresh)
        };
        if bound_input::type_key(text, fresh, key) {
            cx.notify();
        }
    }

    /// Writes every row's min/max input into its roll, so a search always runs with what the
    /// inputs show -- whether it was started by Enter, the Search plate or the sellers and price
    /// selects.
    fn commit_bound_inputs(&mut self) {
        for (filter, ui) in self.filters.iter_mut().zip(&mut self.filter_ui) {
            let Some(roll) = filter.roll.as_mut() else {
                continue;
            };
            // An empty or unparsable bound leaves that side of the search open.
            roll.min = ui.min_text.parse::<f64>().ok();
            roll.max = ui.max_text.parse::<f64>().ok();
            // Re-format from what was committed, so the inputs never show stale/invalid text.
            let format = |bound: Option<f64>| {
                bound
                    .map(|value| bound_input::format(value, roll.dp))
                    .unwrap_or_default()
            };
            ui.min_text = format(roll.min);
            ui.max_text = format(roll.max);
            ui.min_fresh = false;
            ui.max_fresh = false;
        }
    }

    /// The Search button's click handler.
    pub fn trigger_search(&mut self, cx: &mut Context<Self>) {
        self.spawn_search(cx);
    }

    /// Searches the trade site's listings of an exchange item the market doesn't price: what the
    /// panel's "Лоты на площадке" asks for after poe2scout's price (`SearchState::Scouted`).
    pub fn search_listings(&mut self, cx: &mut Context<Self>) {
        self.listings_wanted = true;
        self.spawn_search(cx);
    }

    /// Opens or closes the profile select's menu.
    pub fn set_profile_menu(&mut self, open: bool, cx: &mut Context<Self>) {
        self.profile_menu = open;
        cx.notify();
    }

    /// Sets the item's search up the way `profile` does and searches it, once: the player's pick
    /// from the profile select's menu, or the «Широкий −10 %» button after nothing matched. Broad
    /// keeps the rows the player checked and lowers their minimums from the rolls
    /// (`stat_filters::apply_profile`, PoE Overlay II's `copySelectedFromPrevious`); every other
    /// profile checks its own rows, built afresh as the item opens with it
    /// (`stat_filters::build_filters`). The search goes by the item's base type where the profile
    /// says so (`SearchProfile::searches_base_type`) and by the item's own scope otherwise, as
    /// PoE Overlay II's `includeTypeLine` goes with Crafting Base alone.
    pub fn choose_profile(&mut self, profile: stat_filters::SearchProfile, cx: &mut Context<Self>) {
        self.profile_menu = false;
        let Some(item) = self.item.as_ref().filter(|_| self.profile.is_some()) else {
            cx.notify();
            return;
        };
        if profile == stat_filters::SearchProfile::Broad {
            stat_filters::apply_profile(&mut self.filters, profile);
        } else {
            self.filters = stat_filters::build_filters(
                item,
                profile,
                &self.catalog(self.site).stats,
                search_session(cx),
            );
        }
        self.filter_ui = self
            .filters
            .iter()
            .map(|filter| FilterRowUi::new(cx, filter))
            .collect();
        self.roll_drag = None;
        if let Some(choice) = &mut self.scope {
            // An item whose own scope is its base type keeps it whatever the profile.
            choice.switched_on = profile.searches_base_type() && choice.default.base_type.is_none();
        }
        self.profile = Some(profile);
        self.spawn_search(cx);
        cx.notify();
    }

    /// Searches once more after nothing matched, for listings with all the checked stat rows but
    /// one (`trade_client::one_fewer_match`): the «Совпадение N из M» button.
    pub fn search_one_fewer(&mut self, cx: &mut Context<Self>) {
        self.one_fewer_next = true;
        self.spawn_search(cx);
    }

    /// Builds the item's rows again as a signed-out search does (`stat_filters::Session`), in the
    /// same profile: the mods the sums added up are rows of their own then, picked on their
    /// scores. Whether there were rows to build: an item searched by its exact type or priced by
    /// the exchange has none a profile sets up.
    fn build_without_sums(&mut self, cx: &mut Context<Self>) -> bool {
        let (Some(item), Some(profile)) = (self.item.as_ref(), self.profile) else {
            return false;
        };
        self.filters = stat_filters::build_filters(
            item,
            profile,
            &self.catalog(self.site).stats,
            stat_filters::Session::Anonymous,
        );
        self.filter_ui = self
            .filters
            .iter()
            .map(|filter| FilterRowUi::new(cx, filter))
            .collect();
        self.roll_drag = None;
        true
    }

    /// The «Искать без сумм» button after the site refused a weighted sum: the rows built again
    /// without sums (`build_without_sums`), searched once.
    pub fn search_without_sums(&mut self, cx: &mut Context<Self>) {
        if self.build_without_sums(cx) {
            self.spawn_search(cx);
            cx.notify();
        }
    }

    /// Sets the minimum of every checked row whose tier the tier table knows to the bottom of
    /// that tier (`roll_slider::tier_minimum`): the «минимум тира» button. Like a typed bound, it
    /// takes effect with the next search.
    pub fn set_tier_minimums(&mut self, cx: &mut Context<Self>) {
        for (filter, ui) in self.filters.iter().zip(&mut self.filter_ui) {
            let Some(roll) = filter.roll.as_ref().filter(|_| filter.enabled) else {
                continue;
            };
            if let Some(floor) = roll_slider::tier_minimum(filter) {
                ui.min_text = bound_input::format(floor, roll.dp);
            }
        }
        cx.notify();
    }

    /// Whether «минимум тира» has a checked row to set.
    pub fn has_tier_minimums(&self) -> bool {
        self.filters
            .iter()
            .any(|filter| filter.enabled && roll_slider::tier_minimum(filter).is_some())
    }

    /// The player pressed row `row`'s roll slider at `fraction` of its track: the handle jumps
    /// there and follows the mouse until the button is released (`slide_roll`, `end_roll_drag`).
    pub fn begin_roll_drag(&mut self, row: usize, fraction: f64, cx: &mut Context<Self>) {
        self.roll_drag = Some(row);
        self.slide_roll(row, fraction, cx);
    }

    /// Puts row `row`'s slider handle at `fraction` of its track: the row's minimum box -- its
    /// maximum box where a lower roll is better -- takes the roll there
    /// (`roll_slider::Slider::value_at`). Like a typed bound, it takes effect with the next search.
    pub fn slide_roll(&mut self, row: usize, fraction: f64, cx: &mut Context<Self>) {
        let (Some(filter), Some(ui)) = (self.filters.get(row), self.filter_ui.get_mut(row)) else {
            return;
        };
        let (Some(slider), Some(roll)) = (Slider::of(filter), filter.roll.as_ref()) else {
            return;
        };
        let text = bound_input::format(slider.value_at(fraction), roll.dp);
        let bound = match slider.handle {
            Handle::Min => &mut ui.min_text,
            Handle::Max => &mut ui.max_text,
        };
        if *bound != text {
            *bound = text;
            cx.notify();
        }
    }

    /// The slider's mouse button was released: the handle stays where it is.
    pub fn end_roll_drag(&mut self, cx: &mut Context<Self>) {
        if self.roll_drag.take().is_some() {
            cx.notify();
        }
    }

    /// Shows the panel for a check at `placement` -- the rect and side
    /// `game_window::panel_at_cursor` gave; `None` keeps the last ones -- and plays its appearance
    /// again (`ui::panel`).
    fn show_panel(&mut self, placement: Option<(PhysicalRect, PanelSide)>) {
        self.visible = true;
        self.appearances += 1;
        self.panel_drag = None;
        if let Some((rect, side)) = placement {
            self.placement = Some(rect);
            self.panel_side = Some(side);
        }
    }

    /// Whether the player is dragging the panel by its title bar.
    pub fn dragging_panel(&self) -> bool {
        self.panel_drag.is_some()
    }

    /// The player pressed the title bar's empty part: the panel follows the pointer sideways, kept
    /// on the game's monitor (`overlay_layout::dragged`), until the button is released -- and its
    /// side keeps the place (`drag_panel`). The pointer is polled rather than followed through
    /// mouse moves: the game keeps the foreground, so the panel can't capture the mouse, and the
    /// moves stop reaching it the moment the pointer outruns it. Nothing here activates the
    /// panel: the game keeps the keyboard throughout.
    pub fn begin_panel_drag(&mut self, cx: &mut Context<Self>) {
        let (Some(start), Some(side)) = (self.placement, self.panel_side) else {
            return;
        };
        let (Some(cursor_x), Some(screen)) = (game_window::cursor_x(), GameScreen::at_cursor())
        else {
            return;
        };
        // A press while a drag's loop still runs -- a quick second press before it saw the
        // release -- starts over from here in the same loop.
        let running = self.panel_drag.is_some();
        self.panel_drag = Some(PanelDrag {
            cursor_x,
            start,
            side,
            screen,
        });
        cx.notify();
        if running {
            return;
        }
        cx.spawn(async move |view, cx| {
            let mut dragging = true;
            while dragging {
                cx.background_executor().timer(PANEL_DRAG_POLL).await;
                let held = game_window::primary_button_down();
                let cursor_x = game_window::cursor_x();
                dragging = view
                    .update(cx, |state, cx| state.drag_panel(cursor_x, held, cx))
                    .unwrap_or(false);
            }
        })
        .detach();
    }

    /// One look at the pointer during a drag: the panel at its x -- then, the button released, the
    /// drag's end, where the side keeps the place the panel was left at. `false` once the drag is
    /// over or called off (the panel closed; a double-click sent it back).
    fn drag_panel(&mut self, cursor_x: Option<i32>, held: bool, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.panel_drag else {
            return false;
        };
        if !self.visible {
            self.panel_drag = None;
            return false;
        }
        if let Some(cursor_x) = cursor_x {
            let rect =
                overlay_layout::dragged(drag.start, cursor_x - drag.cursor_x, drag.screen.monitor);
            if self.placement != Some(rect) {
                self.placement = Some(rect);
                cx.notify();
            }
        }
        if held {
            return true;
        }
        self.panel_drag = None;
        cx.notify();
        if let Some(rect) = self.placement.filter(|rect| rect.x != drag.start.x) {
            self.settings
                .panel_positions
                .remember(drag.side, drag.screen.game, rect.x);
            log::info!(
                "the price panel was dragged to x {} on the {:?} side",
                rect.x,
                drag.side
            );
            self.save_settings(cx);
        }
        false
    }

    /// A double-click on the title bar: the panel goes back to EE2's placement on its side, where
    /// the next checks on that side open too.
    pub fn reset_panel_position(&mut self, cx: &mut Context<Self>) {
        self.panel_drag = None;
        let Some(side) = self.panel_side else {
            return;
        };
        let kept = self.settings.panel_positions;
        self.settings.panel_positions.forget(side);
        if let Some(rect) = game_window::automatic_panel_rect(side, self.settings.ui_scale) {
            self.placement = Some(rect);
        }
        log::info!("the price panel went back to its own place on the {side:?} side");
        if self.settings.panel_positions != kept {
            self.save_settings(cx);
        }
        cx.notify();
    }

    fn spawn_search(&mut self, cx: &mut Context<Self>) {
        self.commit_bound_inputs();
        cx.spawn(async move |weak, cx| {
            if let Some(view) = weak.upgrade() {
                run_search(&view, cx).await;
            }
        })
        .detach();
    }
}

impl PriceCheckApp {
    fn catalog(&self, site: TradeSite) -> &SiteCatalog {
        match site {
            TradeSite::International => &self.international,
            TradeSite::Russian => &self.russian,
        }
    }

    /// Puts `row`'s whisper on the clipboard for the game's chat and marks the row for a moment:
    /// copying, not sending -- the player sends it themselves, one action per keypress as the
    /// game's rules ask of tools.
    pub fn copy_whisper(&mut self, row: usize, whisper: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(whisper));
        let copied = (self.search_generation, row);
        self.copied_whisper = Some(copied);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WHISPER_COPIED_SHOWN).await;
            this.update(cx, |state, cx| {
                if state.copied_whisper == Some(copied) {
                    state.copied_whisper = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Whether `row` of the listings shown now just had its whisper copied.
    pub fn whisper_copied(&self, row: usize) -> bool {
        self.copied_whisper == Some((self.search_generation, row))
    }

    /// Icon URL of a currency (or any exchange-tradable static item) by its trade id, from the
    /// displayed item's site catalog.
    pub fn currency_icon(&self, id: &str) -> Option<&str> {
        self.catalog(self.site)
            .currencies
            .iter()
            .find(|currency| currency.id == id)?
            .icon_url
            .as_deref()
    }

    /// The league's exchange market, once loaded.
    pub fn market(&self) -> Option<&Market> {
        self.market.as_ref().map(|(market, _)| market)
    }

    /// A currency's (or other exchange item's) name on the displayed item's site, by trade id.
    pub fn currency_name(&self, id: &str) -> Option<&str> {
        self.catalog(self.site)
            .currencies
            .iter()
            .find(|currency| currency.id == id)
            .map(|currency| currency.display_name.as_str())
    }
}

/// Loads (or fetches) one site's catalogs (see `SiteCatalog`), cached per site for
/// `CATALOG_MAX_AGE` -- its league list for `LEAGUES_MAX_AGE`; a failed fetch falls back on the
/// cache, however old.
async fn load_site_catalog(
    client: &Arc<dyn HttpClient>,
    cache_dir: &std::path::Path,
    site: TradeSite,
) -> Result<SiteCatalog> {
    let (suffix, site_name) = match site {
        TradeSite::International => ("", "www"),
        TradeSite::Russian => ("-ru", "ru"),
    };
    let max_age = CATALOG_MAX_AGE;
    let stats = trade_client::cache::load_or_fetch(
        &cache_dir.join(format!("stat-catalog{suffix}.json")),
        max_age,
        trade_client::catalog::fetch_stat_catalog(client, site),
    )
    .await
    .with_context(|| format!("loading {site_name} stat catalog"))?;
    let currencies = trade_client::cache::load_or_fetch(
        &cache_dir.join(format!("static-items{suffix}.json")),
        max_age,
        trade_client::catalog::fetch_static_currencies(client, site),
    )
    .await
    .with_context(|| format!("loading {site_name} static item list"))?;
    let item_types = trade_client::cache::load_or_fetch(
        &cache_dir.join(format!("item-types{suffix}.json")),
        max_age,
        trade_client::catalog::fetch_item_types(client, site),
    )
    .await
    .with_context(|| format!("loading {site_name} base type list"))?;
    let leagues = trade_client::cache::load_or_fetch(
        &cache_dir.join(format!("leagues{suffix}.json")),
        LEAGUES_MAX_AGE,
        trade_client::leagues(client, site),
    )
    .await
    .with_context(|| format!("loading {site_name} league list"))?;
    Ok(SiteCatalog {
        stats,
        currencies,
        item_types,
        leagues,
    })
}

/// Both supported sites' catalogs, leagues included -- the item's language isn't known until the
/// first check, and the player can switch client language at any time. All of it is cached, so an
/// app started without a network has it all.
async fn bootstrap_catalogs(client: &Arc<dyn HttpClient>) -> Result<(SiteCatalog, SiteCatalog)> {
    let cache_dir = paths::cache_dir();
    let international = load_site_catalog(client, &cache_dir, TradeSite::International).await?;
    let russian = load_site_catalog(client, &cache_dir, TradeSite::Russian).await?;
    ensure!(
        !international.leagues.is_empty(),
        "the league list is empty"
    );
    Ok((international, russian))
}

/// The league's exchange market, asked for again once `MARKET_MAX_AGE` old (`fetch_market` keeps
/// GGG's hours and poe2scout's copy, so a reload downloads at most GGG's newest hour).
async fn current_market(
    view: &Entity<PriceCheckApp>,
    cx: &mut AsyncApp,
    client: &Arc<dyn HttpClient>,
    league: &str,
) -> Result<Market> {
    let (cached, market_league) = view.read_with(cx, |state, _| {
        let cached = state
            .market
            .as_ref()
            .filter(|(_, loaded_at)| loaded_at.elapsed() < MARKET_MAX_AGE)
            .map(|(market, _)| market.clone());
        let market_league =
            league_chip::market_league(league, &state.leagues, &state.private_leagues).to_owned();
        (cached, market_league)
    });
    if let Some(market) = cached {
        return Ok(market);
    }
    let market = trade_client::cx::fetch_market(client, &market_league, &paths::cache_dir())
        .await
        .context("loading the exchange market")?;
    // A market that arrives after the player switched leagues is not this league's.
    view.update(cx, |state, cx| {
        if state.league == league {
            state.market = Some((market.clone(), Instant::now()));
            cx.notify();
        }
    });
    Ok(market)
}

/// The league's poe2scout prices of uniques, reloaded once `MARKET_MAX_AGE` old (`fetch_prices`
/// keeps its own half-hour disk cache).
async fn current_scout(
    view: &Entity<PriceCheckApp>,
    cx: &mut AsyncApp,
    client: &Arc<dyn HttpClient>,
    league: &str,
) -> Result<()> {
    let (fresh, market_league) = view.read_with(cx, |state, _| {
        let fresh = state
            .scout
            .as_ref()
            .is_some_and(|(_, loaded_at)| loaded_at.elapsed() < MARKET_MAX_AGE);
        let market_league =
            league_chip::market_league(league, &state.leagues, &state.private_leagues).to_owned();
        (fresh, market_league)
    });
    if fresh {
        return Ok(());
    }
    let prices = trade_client::scout::fetch_prices(client, &market_league, &paths::cache_dir())
        .await
        .context("loading poe2scout's prices")?;
    // Prices that arrive after the player switched leagues are not this league's.
    view.update(cx, |state, cx| {
        if state.league == league {
            state.scout = Some((prices, Instant::now()));
            cx.notify();
        }
    });
    Ok(())
}

/// Opens the price-check `Entity`, immediately in `BootstrapState::Loading`, and spawns the task
/// that loads the catalogs and leagues (`PriceCheckApp::apply_bootstrap`) -- then again every
/// `CATALOG_REFRESH` while the app runs, and after a failed first load sooner
/// (`BOOTSTRAP_RETRY`).
pub fn create_app(
    cx: &mut App,
    http_client: Arc<dyn HttpClient>,
    settings: Settings,
) -> Entity<PriceCheckApp> {
    let view = cx.new(|_cx| PriceCheckApp::loading(http_client.clone(), settings));

    // The account's private leagues follow the session: loaded once the site accepts it,
    // forgotten once it's gone -- not while a check is under way.
    let weak = view.downgrade();
    cx.observe_global::<SessionStatus>(move |cx| {
        if matches!(
            cx.try_global::<SessionStatus>(),
            Some(
                SessionStatus::SignedIn { .. } | SessionStatus::SignedOut | SessionStatus::Invalid
            )
        ) {
            let _ = weak.update(cx, |app, cx| app.refresh_private_leagues(cx));
        }
    })
    .detach();

    let weak = view.downgrade();
    cx.spawn(async move |cx| {
        let mut failures = 0;
        loop {
            let result = bootstrap_catalogs(&http_client).await;
            let loaded = result.is_ok();
            let applied = weak.update(cx, |state, cx| {
                state.apply_bootstrap(result, cx);
                matches!(state.bootstrap, BootstrapState::Ready)
            });
            let Ok(ready) = applied else {
                return;
            };
            failures = if loaded { 0 } else { failures + 1 };
            let wait = if ready {
                CATALOG_REFRESH
            } else {
                BOOTSTRAP_RETRY[(failures - 1).min(BOOTSTRAP_RETRY.len() - 1)]
            };
            cx.background_executor().timer(wait).await;
        }
    })
    .detach();

    view
}

/// Sets up the price-check hotkey (the player's, `Ctrl+E` by default) and installs the Esc hook,
/// then spawns the long-lived tasks that bridge them onto GPUI's executor: the hotkey follows
/// the foreground window (`PriceCheckApp::sync_hotkey_registration`), its presses run the
/// price-check pipeline, and Esc presses close the panel. Each task awaits a channel its OS
/// callback feeds, so an idle app does no work at all. Must be called from the thread
/// `application().run` pumps the platform message loop on (`global_hotkey`'s and the foreground
/// hook's own requirement).
///
/// The panel closes only on Esc (or its × button), never on mouse movement -- the player moves
/// into it to use the filters -- and Esc is taken from the game only while the panel is open.
///
/// The tasks live as long as the app, so they hold only a `WeakEntity` and tolerate the window
/// having closed.
pub fn register_hotkeys(cx: &mut App, view: Entity<PriceCheckApp>) -> Result<()> {
    // Before anything is registered: `global_hotkey` settles on its channel for good with the
    // first event it delivers.
    let (press_tx, presses) = async_channel::unbounded();
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state() == HotKeyState::Pressed {
            let _ = press_tx.try_send(event);
        }
    }));
    let manager = GlobalHotKeyManager::new().context("GlobalHotKeyManager::new failed")?;
    let esc_presses = esc_hook::install()?;
    let foreground_changes = game_window::watch_foreground()?;
    view.update(cx, |state, _| {
        state.hotkeys = Some(manager);
        state.sync_hotkey_registration(game_window::foreground());
    });

    let weak = view.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(mut foreground) = foreground_changes.recv().await {
            // Alt+Tab through several windows is a burst of changes: one sync, to where it ended.
            while let Ok(later) = foreground_changes.try_recv() {
                foreground = later;
            }
            let Some(view) = weak.upgrade() else {
                return;
            };
            view.update(cx, |state, _| state.sync_hotkey_registration(foreground));
            // Events can land out of step with where the foreground settles (a console window's
            // activation was seen reported after the game had already taken the foreground back),
            // so once things are quiet the actual foreground has the last word.
            cx.background_executor().timer(FOREGROUND_SETTLE).await;
            if foreground_changes.is_empty() {
                let settled = game_window::foreground();
                view.update(cx, |state, _| state.sync_hotkey_registration(settled));
            }
        }
    })
    .detach();

    let weak = view.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(event) = presses.recv().await {
            let Some(view) = weak.upgrade() else {
                return;
            };
            match view.read_with(cx, |state, _| state.pressed(event.id())) {
                Some(Pressed::PriceCheck) => run_price_check(&view, cx).await,
                Some(Pressed::QuickAction(action)) => {
                    run_quick_action(action, cx).await;
                    // Presses that queued up meanwhile are dropped, as EE2's `restoreShortly`
                    // drops an action while the last one's clipboard is still out: a mashed key
                    // must not flood the chat -- the game disconnects for too many actions.
                    while presses.try_recv().is_ok() {}
                }
                None => {}
            }
        }
    })
    .detach();

    // Esc is taken from the game exactly while the panel is shown.
    cx.observe(&view, |view, cx| esc_hook::set_armed(view.read(cx).visible))
        .detach();
    // Separate from the hotkey task, which stays busy through a check's clipboard poll: Esc must
    // close the panel the moment it's pressed.
    let weak = view.downgrade();
    cx.spawn(async move |cx| {
        while esc_presses.recv().await.is_ok() {
            let Some(view) = weak.upgrade() else {
                return;
            };
            // An open menu -- the league's or the profile's -- closes first, the panel with the
            // next press.
            view.update(cx, |state, cx| {
                if state.league_menu || state.profile_menu {
                    state.league_menu = false;
                    state.profile_menu = false;
                    cx.notify();
                } else if state.visible {
                    state.visible = false;
                    cx.notify();
                }
            });
        }
    })
    .detach();

    Ok(())
}

/// What a hotkey press is for (`PriceCheckApp::pressed`).
enum Pressed {
    PriceCheck,
    QuickAction(QuickAction),
}

/// Types a quick action into the game (`game_chat`): its text through the clipboard, which gets
/// the player's own content back right after.
async fn run_quick_action(action: QuickAction, cx: &mut AsyncApp) {
    // The hotkey is held only while the game is in front, but a press can race the player's
    // switch to another program, and must not type there.
    if game_window::foreground() != Foreground::Game {
        return;
    }
    let Some(hotkey) = action.hotkey else {
        return;
    };
    let mut held = vec![VIRTUAL_KEY(hotkey.key.virtual_key())];
    for (down, key) in [
        (hotkey.ctrl, VK_CONTROL),
        (hotkey.shift, VK_SHIFT),
        (hotkey.alt, VK_MENU),
    ] {
        if down {
            held.push(key);
        }
    }
    log::info!("{hotkey}: quick action ({:?})", action.kind);
    game_chat::type_action(&action, &held, cx).await;
}

/// `Ctrl+E, F5` -- for the log and the diagnostics report.
fn hotkey_list(hotkeys: &[Hotkey]) -> String {
    if hotkeys.is_empty() {
        return "none".to_owned();
    }
    hotkeys
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The full pipeline for one hotkey trigger: synthesize the copy combo, poll the clipboard,
/// parse, build filters, and kick off the initial search. The panel then stays open until Esc or
/// its × button (see `register_hotkeys`).
async fn run_price_check(view: &Entity<PriceCheckApp>, cx: &mut AsyncApp) {
    // A press that raced the foreground switch -- the player just went to another program: the
    // copy combo must not land there.
    if game_window::foreground() == Foreground::Other {
        return;
    }
    let ready = view.read_with(cx, |state, _| match state.bootstrap {
        BootstrapState::Ready => Some((
            state.advanced_mod_desc_key,
            state.settings.hotkey,
            state.settings.ui_scale,
            state.settings.panel_positions,
        )),
        _ => None,
    });
    let Some((mod_key, hotkey, ui_scale, positions)) = ready else {
        // Nothing to parse against yet: the panel says why (still loading, or the first load
        // failed), and a failed load is tried again at once.
        view.update(cx, |state, cx| {
            state.show_panel(game_window::panel_at_cursor(
                state.settings.ui_scale,
                &state.settings.panel_positions,
            ));
            if matches!(state.bootstrap, BootstrapState::Failed(_)) {
                state.retry_bootstrap(cx);
            }
            cx.notify();
        });
        return;
    };

    // Where the check happened, recorded before anything is synthesized -- EE2 reads the cursor
    // (`screen.getCursorScreenPoint()`) ahead of `pressKeysToCopyItemText` for the same reason.
    let placement = game_window::panel_at_cursor(ui_scale, &positions);

    // After the player clicked into the open panel, the panel -- not the game -- has keyboard
    // focus, and the copy combo would land in the panel. Hand focus back first.
    if game_window::reclaim_game_focus() {
        cx.background_executor()
            .timer(game_window::FOCUS_SWITCH_DELAY)
            .await;
    }

    // EE2's `keepModKeys` (leave the player's own Ctrl alone) only while Ctrl is still physically
    // held: a quick tap can release it before this runs, and the game copies nothing on a bare
    // Alt+C. The hotkey's own key is released first, and so is a held Shift: the game copies on
    // Ctrl+Alt+C, not with Shift added.
    let keep_mod_keys = ctrl_is_down();
    let mut release = vec![VIRTUAL_KEY(hotkey.key.virtual_key())];
    if hotkey.shift {
        release.push(VK_SHIFT);
    }
    let clipboard_text = clipboard_poll::poll_item_clipboard(
        cx,
        move || synth_input::send_copy_item_combo(mod_key, keep_mod_keys, &release),
        CLIPBOARD_INITIAL_DELAY,
        CLIPBOARD_POLL_INTERVAL,
        CLIPBOARD_TIMEOUT,
    )
    .await;

    let Some(text) = clipboard_text else {
        // Usually no item under the cursor: nothing to say, as in EE2 (its empty `.catch`). But a
        // copy combo another program holds swallows every check, and the player can't tell why
        // -- that one gets the panel.
        if synth_input::copy_combo_taken(mod_key) {
            let combo = synth_input::copy_combo_label(mod_key);
            log::warn!("no item text: another program holds {combo}");
            view.update(cx, |state, cx| {
                state.show_panel(placement);
                state.league_menu = false;
                state.show_problem(Problem::ComboTaken(combo));
                cx.notify();
            });
        }
        return;
    };

    let (has_item, diagnosis) = view.update(cx, |state, cx| {
        state.show_panel(placement);
        state.league_menu = false;
        state.profile_menu = false;
        state.item_text = Some(text.clone());
        let parsed = match item_site(&text, state.settings.client_language.item_language()) {
            Some((language, site)) => {
                state.site = site;
                item_parser::parse_clipboard(&text, language, &state.catalog(site).stats)
            }
            None => Err(ParseError::UnknownLanguage),
        };
        let has_item = parsed.is_ok();
        let diagnosis = match &parsed {
            // A gamble offer is fully understood: there is just nothing to price.
            Err(ParseError::Unrevealed) => None,
            Err(_) => Some(UnparsedReason::Rejected),
            Ok(item) if has_unread_lines(item) => Some(UnparsedReason::UnreadLines),
            Ok(_) => None,
        };
        match parsed {
            Ok(item) => {
                let profile = stat_filters::SearchProfile::default_for(&item);
                state.filters = stat_filters::build_filters(
                    &item,
                    profile,
                    &state.catalog(state.site).stats,
                    search_session(cx),
                );
                state.filter_ui = state
                    .filters
                    .iter()
                    .map(|filter| FilterRowUi::new(cx, filter))
                    .collect();
                let catalog = state.catalog(state.site);
                let route =
                    trade_client::route_search(&item, &catalog.currencies, &catalog.item_types);
                let scope = match &route {
                    SearchRoute::Filtered { scope } => {
                        trade_client::switched_scope(scope, &item, &catalog.item_types).map(
                            |switched| ScopeChoice {
                                default: scope.clone(),
                                switched,
                                switched_on: false,
                            },
                        )
                    }
                    _ => None,
                };
                state.priced_by_market = matches!(route, SearchRoute::Market { .. });
                // Only a filtered search goes by the rows a profile sets up.
                state.profile = matches!(route, SearchRoute::Filtered { .. }).then_some(profile);
                let misc = match &route {
                    SearchRoute::Filtered { scope } => Some(scope.misc),
                    _ => None,
                };
                state.corruption = misc.and_then(|misc| misc.corrupted.map(StateChoice::matched));
                state.identification =
                    misc.and_then(|misc| misc.identified.map(StateChoice::matched));
                state.rarity = match &route {
                    SearchRoute::Filtered { scope } => scope.rarity.and_then(|default| {
                        trade_client::other_rarity(default, &item).map(|other| RarityChoice {
                            default,
                            other,
                            switched_on: false,
                        })
                    }),
                    _ => None,
                };
                state.scope = scope;
                state.item = Some(item);
                state.problem = None;
                state.show_hidden = false;
                state.listings_wanted = false;
                state.one_fewer_next = false;
                state.roll_drag = None;
                state.listing_status = state.settings.listing_status.into();
                state.search = SearchState::NotSearched;
            }
            Err(err) => {
                log::warn!("not parsed: {err:?}");
                state.show_problem(Problem::Unparsed {
                    error: err,
                    saved: None,
                });
            }
        }
        cx.notify();
        (has_item, diagnosis)
    });

    // With `KEEP_ITEM_TEXTS_ENV` set every checked text is kept, not only the troubled ones.
    let diagnosis =
        diagnosis.or_else(|| std::env::var_os(KEEP_ITEM_TEXTS_ENV).map(|_| UnparsedReason::Sample));
    if let Some(reason) = diagnosis {
        match save_unparsed(&paths::unparsed_dir(), &text, reason) {
            // A rejected item says where its text went, so the player can send it in.
            Ok(path) if reason == UnparsedReason::Rejected => view.update(cx, |state, cx| {
                if let Some(Problem::Unparsed { saved, .. }) = &mut state.problem {
                    *saved = Some(path);
                    cx.notify();
                }
            }),
            Ok(_) => {}
            Err(err) => log::warn!("saving the item text failed: {err:#}"),
        }
    }

    // Detached: the next press -- another check, a quick action -- must not wait on the network
    // (a rate-limit wait alone can take `MAX_RATE_LIMIT_WAIT`). A newer search supersedes this
    // one through `search_generation`.
    if has_item {
        let view = view.clone();
        cx.spawn(async move |cx| run_search(&view, cx).await)
            .detach();
    }
    // A unique shows poe2scout's price too: fresh prices for it, if the league's are stale.
    let scout_refresh = view.read_with(cx, |state, _| {
        let unique = state
            .item
            .as_ref()
            .is_some_and(|item| item.rarity == Some(ItemRarity::Unique));
        unique.then(|| (state.http_client.clone(), state.league.clone()))
    });
    if let Some((client, league)) = scout_refresh {
        let view = view.clone();
        cx.spawn(async move |cx| {
            if let Err(err) = current_scout(&view, cx, &client, &league).await {
                log::warn!("{err:#}");
            }
        })
        .detach();
    }
}

/// Set (to anything) to keep every checked item's text beside the troubled ones: real client
/// copies to build parser fixtures from, e.g. by sweeping an inventory.
const KEEP_ITEM_TEXTS_ENV: &str = "POE2_ORACLE_KEEP_ITEM_TEXTS";

/// Why an item text is kept for diagnosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnparsedReason {
    /// The parser refused the text.
    Rejected,
    /// The item parsed, but some stat lines matched nothing in the site's stat catalog.
    UnreadLines,
    /// Kept because `KEEP_ITEM_TEXTS_ENV` asks for every text.
    Sample,
}

/// How many kept item texts stay on disk; the oldest go first.
const UNPARSED_KEEP: usize = 100;

/// Any stat line the parser couldn't resolve to a trade stat id -- inside a resolved modifier, or
/// a whole modifier none of whose lines resolved (`ParsedItem::unknown_mods`).
fn has_unread_lines(item: &ParsedItem) -> bool {
    !item.unknown_mods.is_empty()
        || item
            .mods
            .iter()
            .flat_map(|modifier| &modifier.stats)
            .any(|stat| stat.stat_id.is_none())
}

/// Writes `text` into `dir` as `<unix millis>-<reason>.txt`, then drops the oldest files beyond
/// `UNPARSED_KEEP` (the millisecond prefix sorts them oldest first).
fn save_unparsed(dir: &Path, text: &str, reason: UnparsedReason) -> Result<PathBuf> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let suffix = match reason {
        UnparsedReason::Rejected => "rejected",
        UnparsedReason::UnreadLines => "unread-lines",
        UnparsedReason::Sample => "sample",
    };
    let path = dir.join(format!("{millis}-{suffix}.txt"));
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;

    let mut kept: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
        .collect();
    if kept.len() > UNPARSED_KEEP {
        kept.sort();
        for old in &kept[..kept.len() - UNPARSED_KEEP] {
            let _ = fs::remove_file(old);
        }
    }
    Ok(path)
}

/// Which parser language and trade site `text` belongs to: the client language the player set
/// (`Settings::client_language`), else the one detected from the text -- `None` for a client
/// language this app can't parse (the poll accepts every language, so this is a real, reportable
/// case).
fn item_site(text: &str, forced: Option<ItemLanguage>) -> Option<(ItemLanguage, TradeSite)> {
    match forced.or_else(|| item_parser::detect_item_language(text))? {
        ItemLanguage::English => Some((ItemLanguage::English, TradeSite::International)),
        ItemLanguage::Russian => Some((ItemLanguage::Russian, TradeSite::Russian)),
    }
}

/// Re-runs search for whatever item/filters are currently in `state` -- the same entry point the
/// initial post-parse auto-search uses, and what the UI calls again when the user presses Enter
/// in a filter input or clicks the Search affordance (neither editing a filter's checkbox/min/max
/// nor building the filter list itself triggers this on its own -- matches the reference's own
/// lack of any debounce anywhere in its codebase).
pub async fn run_search(view: &Entity<PriceCheckApp>, cx: &mut AsyncApp) {
    let prepared = view.update(cx, |state, cx| {
        // A weighted sum goes out only while the site takes one from the session
        // (`search_session`): rows built with sums before it stopped -- the site refused one, the
        // player signed out -- are built again without them first, as «Искать без сумм» does,
        // whatever asked for the search.
        if search_session(cx) == stat_filters::Session::Anonymous
            && state
                .filters
                .iter()
                .any(|filter| filter.enabled && filter.weighted_sum)
            && state.build_without_sums(cx)
        {
            log::info!("no weighted sums without an accepted sign-in: the rows are built again");
        }
        let item = state.item.as_ref()?;
        let catalog = state.catalog(state.site);
        let mut route = trade_client::route_search(item, &catalog.currencies, &catalog.item_types);
        if state.listings_wanted && matches!(route, SearchRoute::Market { .. }) {
            route = SearchRoute::Exact {
                exact_type: item.name.clone(),
            };
        }
        if let (SearchRoute::Filtered { scope }, Some(choice)) = (&mut route, &state.scope) {
            *scope = choice.current().clone();
        }
        if let SearchRoute::Filtered { scope } = &mut route {
            if state.corruption.is_some_and(|choice| !choice.on) {
                scope.misc.corrupted = None;
            }
            if state.identification.is_some_and(|choice| !choice.on) {
                scope.misc.identified = None;
            }
        }
        if let (SearchRoute::Filtered { scope }, Some(choice)) = (&mut route, state.rarity) {
            scope.rarity = Some(choice.current());
        }
        if let SearchRoute::Filtered { scope } = &mut route {
            scope.price = state.price_currency;
        }
        // The relaxed search the player asked for, this once (`search_one_fewer`).
        let one_fewer = std::mem::take(&mut state.one_fewer_next)
            .then(|| trade_client::one_fewer_match(&state.filters))
            .flatten();
        if let (SearchRoute::Filtered { scope }, Some((least, _))) = (&mut route, one_fewer) {
            scope.stat_match = StatMatch::AtLeast(least);
        }
        let key = search_key(
            state.site,
            &state.league,
            state.listing_status,
            &route,
            &state.filters,
        );
        let cached = state.cached_results(&key);
        let item_name = item.name.clone();
        // One line per check: what was checked and how it's priced -- with the outcome line
        // below, the audit trail of a session's checks.
        log::info!(
            "{} / {} [{}] -> {} in {}",
            item.name,
            item.base_type.as_deref().unwrap_or("-"),
            item.category
                .as_ref()
                .map_or("-", |category| category.id.as_str()),
            match &route {
                SearchRoute::Market { .. } => "market".to_owned(),
                SearchRoute::Exact { .. } => "exact".to_owned(),
                SearchRoute::Filtered { .. } => format!(
                    "filtered{}{}",
                    state
                        .profile
                        .map(|profile| format!(", {profile:?}"))
                        .unwrap_or_default(),
                    one_fewer
                        .map(|(least, of)| format!(", at least {least} of {of} stats"))
                        .unwrap_or_default()
                ),
            },
            state.league
        );
        state.search = SearchState::Searching;
        state.search_generation += 1;
        cx.notify();
        Some((
            state.http_client.clone(),
            state.site,
            state.league.clone(),
            route,
            state.filters.clone(),
            state.listing_status,
            state.search_generation,
            (key, cached, item_name),
        ))
    });
    let Some((client, site, league, route, filters, status, generation, (key, cached, item_name))) =
        prepared
    else {
        return;
    };
    let target = SearchTarget {
        client: &client,
        site,
        league: &league,
        status,
        generation,
        item_name: &item_name,
    };

    let from_cache = cached.is_some();
    let summed = filters
        .iter()
        .any(|filter| filter.enabled && filter.weighted_sum);
    let outcome = match cached {
        Some(results) => Ok(RouteOutcome::Listings(results)),
        None => execute_route(view, cx, &target, route, &filters).await,
    };
    if !from_cache && let Ok(RouteOutcome::Listings(results)) = &outcome {
        let results = results.clone();
        view.update(cx, |state, _| state.remember_results(key, results));
    }
    let listed =
        matches!(&outcome, Ok(RouteOutcome::Listings(results)) if !results.rows.is_empty());

    match &outcome {
        Ok(RouteOutcome::Market(price)) => {
            log::info!("  market {:.4} div", price.divine_value);
        }
        Ok(RouteOutcome::Scouted { value, unit }) => {
            log::info!(
                "  no exchange trades lately; poe2scout {value:.4} {}",
                unit.trade_id()
            );
        }
        Ok(RouteOutcome::Listings(results)) => log::info!(
            "  {} found, {} listed{}{}",
            results.total,
            results.rows.len(),
            results
                .relaxed
                .map(|(least, of)| format!(" matching {least} of {of} stats"))
                .unwrap_or_default(),
            if from_cache { " (cached)" } else { "" }
        ),
        Err(err) => log::warn!("search failed: {err:#}"),
    }
    view.update(cx, |state, cx| {
        if state.search_generation != generation {
            return;
        }
        // Rows stay in the order they arrived in: the search sorts by price across currencies,
        // which raw amounts ("1 divine" vs "40 exalted") can't.
        state.search = match outcome {
            Ok(RouteOutcome::Market(price)) => SearchState::Market(price),
            Ok(RouteOutcome::Scouted { value, unit }) => SearchState::Scouted { value, unit },
            Ok(RouteOutcome::Listings(results)) if results.rows.is_empty() => SearchState::Empty {
                relaxed: results.relaxed,
            },
            Ok(RouteOutcome::Listings(results)) => SearchState::Matched {
                total: results.total,
                rows: results.rows,
                trade_url: results.trade_url,
                relaxed: results.relaxed,
            },
            Err(err) => SearchState::Failed(SearchFailure::of(&err, summed)),
        };
        // A weighted sum refused: the site doesn't take the session the app holds for a signed-in
        // one. No search sends a sum until the account page accepts the session again
        // (`session::sum_refused`): the next one builds its rows without them.
        if matches!(
            state.search,
            SearchState::Failed(SearchFailure::TooComplex {
                weighted_sums: true
            })
        ) {
            log::warn!("the trade site refused a weighted sum; checking the session again");
            crate::session::sum_refused(cx);
        }
        cx.notify();
    });
    // The rows' exchange-rate equivalents and the title bar's rate: a market that has aged is
    // asked for again once the listings show, which don't wait on it -- they fill in when it
    // arrives, and go without it when it doesn't.
    if listed {
        let _ = current_market(view, cx, &client, &league).await;
    }
}

/// How a search's rows are built: with weighted sums only while the trade site takes one from the
/// session (`session::sums_allowed`).
fn search_session(cx: &App) -> stat_filters::Session {
    if crate::session::sums_allowed(cx) {
        stat_filters::Session::SignedIn
    } else {
        stat_filters::Session::Anonymous
    }
}

/// The trade site's own results page for `query_id` -- on the item's site, so a Russian item's
/// link opens the Russian trade site like EE2's does.
fn trade_site_url(site: TradeSite, league: &str, query_id: &str) -> String {
    format!(
        "{}/trade2/search/poe2/{}/{query_id}",
        site.origin(),
        trade_client::encode_league(league)
    )
}

/// A trade search's listing rows, plus the search's own total and trade-site link.
#[derive(Clone)]
struct SearchResults {
    rows: Vec<ListingRow>,
    total: u64,
    trade_url: String,
    /// See `SearchState::Matched::relaxed`.
    relaxed: Option<(u32, u32)>,
}

/// What pricing an item produced: trade listings, an exchange item's market price, or -- for one
/// GGG's record of the exchange doesn't price -- poe2scout's.
enum RouteOutcome {
    Listings(SearchResults),
    Market(MarketPrice),
    Scouted { value: f64, unit: PriceUnit },
}

/// What every request of one search shares.
struct SearchTarget<'a> {
    client: &'a Arc<dyn HttpClient>,
    site: TradeSite,
    league: &'a str,
    status: ListingStatus,
    /// `PriceCheckApp::search_generation` when this search started.
    generation: u64,
    /// The item's own name: what an exchange item neither market source prices is searched by.
    item_name: &'a str,
}

async fn execute_route(
    view: &Entity<PriceCheckApp>,
    cx: &mut AsyncApp,
    target: &SearchTarget<'_>,
    route: SearchRoute,
    filters: &[stat_filters::SearchFilter],
) -> Result<RouteOutcome> {
    let SearchTarget {
        client,
        site,
        league,
        status,
        generation,
        item_name,
    } = *target;
    match route {
        SearchRoute::Market { trade_id } => {
            let market = match current_market(view, cx, client, league).await {
                Ok(market) => market,
                // Neither GGG's record nor poe2scout within reach, nor kept: the trade site's own
                // listings still price the item, if more roughly -- better than no price at all.
                Err(err) => {
                    log::warn!("{err:#}; pricing {trade_id} from trade listings instead");
                    return exact_listings(view, cx, target, item_name).await;
                }
            };
            if let Some(price) = market.price(&trade_id) {
                return Ok(RouteOutcome::Market(price.clone()));
            }
            // Not every exchange item changes hands every few hours -- a Regal Shard, most of a
            // thin league's runes -- and without GGG's record nothing has a GGG price. poe2scout's
            // price stands in without spending the trade API's IP budget -- the player can still
            // ask for the listings -- and without one, the trade listings by name.
            match market.scout_price(&trade_id) {
                Some((value, unit)) => Ok(RouteOutcome::Scouted { value, unit }),
                None => exact_listings(view, cx, target, item_name).await,
            }
        }
        SearchRoute::Filtered { scope } => {
            // One search, as the player asked for it: when it finds nothing the panel offers the
            // broader ones (`choose_profile`, `search_one_fewer`) rather than spending the trade
            // site's limit on them unasked.
            let mut limiter = limiter_for_request(view, cx, generation, Endpoint::Search).await?;
            let result = trade_client::search_with_filters(
                client,
                site,
                league,
                &scope,
                filters,
                status,
                &mut limiter,
            )
            .await;
            store_limiter(view, cx, Endpoint::Search, limiter);
            let mut results = fetch_listings(view, cx, target, result?).await?;
            if let StatMatch::AtLeast(least) = scope.stat_match {
                results.relaxed = Some((least, trade_client::enabled_stat_rows(filters)));
            }
            Ok(RouteOutcome::Listings(results))
        }
        SearchRoute::Exact { exact_type } => exact_listings(view, cx, target, &exact_type).await,
    }
}

/// The listings of an exact search by type (EE2's exact search): the `Exact` route, and the
/// fallback for an exchange item the market has no price for, or no market at all.
async fn exact_listings(
    view: &Entity<PriceCheckApp>,
    cx: &mut AsyncApp,
    target: &SearchTarget<'_>,
    exact_type: &str,
) -> Result<RouteOutcome> {
    let mut limiter = limiter_for_request(view, cx, target.generation, Endpoint::Search).await?;
    let result = trade_client::search_exact(
        target.client,
        target.site,
        target.league,
        exact_type,
        target.status,
        &mut limiter,
    )
    .await;
    store_limiter(view, cx, Endpoint::Search, limiter);
    Ok(RouteOutcome::Listings(
        fetch_listings(view, cx, target, result?).await?,
    ))
}

/// How long a search's listings are reused for the same search -- the same item checked again,
/// or filters edited back: listings change over minutes, and the trade API's windows are tight.
const SEARCH_CACHE_TTL: Duration = Duration::from_secs(120);
/// How many recent searches are kept.
const SEARCH_CACHE_ENTRIES: usize = 8;

/// Everything that decides a search's listings: site, league, sellers, how the item is routed,
/// and every filter row as it stands.
fn search_key(
    site: TradeSite,
    league: &str,
    status: ListingStatus,
    route: &SearchRoute,
    filters: &[stat_filters::SearchFilter],
) -> String {
    let route = match route {
        SearchRoute::Market { trade_id } => format!("market {trade_id}"),
        SearchRoute::Exact { exact_type } => format!("exact {exact_type}"),
        SearchRoute::Filtered { scope } => format!("filtered {scope:?}"),
    };
    format!("{site:?} {league} {status:?} {route} {filters:?}")
}

/// The first listings of a search -- the `Filtered` and `Exact` routes' shared second step --
/// with a seller's repeated listings folded into one row, as EE2 shows them.
async fn fetch_listings(
    view: &Entity<PriceCheckApp>,
    cx: &mut AsyncApp,
    target: &SearchTarget<'_>,
    outcome: SearchOutcome,
) -> Result<SearchResults> {
    let trade_url = trade_site_url(target.site, target.league, &outcome.query_id);
    let mut groups = Vec::new();
    for ids in outcome
        .listing_ids
        .chunks(FETCH_PAGE_SIZE)
        .take(FETCH_PAGES)
    {
        let mut limiter = limiter_for_request(view, cx, target.generation, Endpoint::Fetch).await?;
        let result = trade_client::fetch(
            target.client,
            target.site,
            ids,
            &outcome.query_id,
            &mut limiter,
        )
        .await;
        store_limiter(view, cx, Endpoint::Fetch, limiter);
        // Unpriced listings are dropped before grouping, so they can't decide which rows count
        // as a seller's "last two".
        trade_client::group_listings(
            &mut groups,
            result?.into_iter().filter(|item| item.price.is_some()),
        );
    }
    Ok(SearchResults {
        rows: groups.into_iter().map(ListingRow::from).collect(),
        total: outcome.total,
        trade_url,
        relaxed: None,
    })
}

/// The trade API's independently rate-limited endpoint families -- one `RateLimiter` each,
/// indexing `PriceCheckApp::limiters`.
#[derive(Clone, Copy)]
enum Endpoint {
    Search,
    Fetch,
}

/// A request refused locally: the trade API's restriction outlasts `MAX_RATE_LIMIT_WAIT`.
#[derive(Debug)]
struct RateLimitedFor(Duration);

impl std::fmt::Display for RateLimitedFor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "trade API rate limit: {}s left", self.0.as_secs())
    }
}

impl std::error::Error for RateLimitedFor {}

/// A copy of `endpoint`'s limiter for the next request, after sitting out whatever wait it says
/// the trade API needs first (shown in the panel meanwhile, if this is still the current search).
/// The network calls need `&mut RateLimiter` outside any `state.update` borrow, so each request
/// works on this copy and `store_limiter` merges it back -- on failure too: a 429 teaches the
/// limiter the restriction.
async fn limiter_for_request(
    view: &Entity<PriceCheckApp>,
    cx: &mut AsyncApp,
    generation: u64,
    endpoint: Endpoint,
) -> Result<RateLimiter> {
    let wait = view.read_with(cx, |state, _| {
        state.limiters[endpoint as usize].required_wait()
    });
    if let Some(wait) = wait {
        if wait > MAX_RATE_LIMIT_WAIT {
            return Err(RateLimitedFor(wait).into());
        }
        show_search_state(
            view,
            cx,
            generation,
            SearchState::RateLimiting {
                wait_secs: wait.as_secs_f64().ceil() as u64,
            },
        );
        cx.background_executor().timer(wait).await;
        show_search_state(view, cx, generation, SearchState::Searching);
    }
    Ok(view.read_with(cx, |state, _| state.limiters[endpoint as usize].clone()))
}

fn show_search_state(
    view: &Entity<PriceCheckApp>,
    cx: &mut AsyncApp,
    generation: u64,
    search: SearchState,
) {
    view.update(cx, |state, cx| {
        if state.search_generation == generation {
            state.search = search;
            cx.notify();
        }
    });
}

/// Merges a request's limiter copy back: concurrent searches each work on their own copy, and
/// the last to finish must not erase a restriction another one just learned. A refusal holds
/// every endpoint, not only the refused one: the trade API restricts the whole IP then
/// (`RateLimiter::refused_until`), and a request sent anyway only earns another refusal.
fn store_limiter(
    view: &Entity<PriceCheckApp>,
    cx: &mut AsyncApp,
    endpoint: Endpoint,
    limiter: RateLimiter,
) {
    view.update(cx, |state, _cx| {
        state.limiters[endpoint as usize].merge(&limiter);
        if let Some(until) = limiter.refused_until() {
            for other in &mut state.limiters {
                other.hold_until(until);
            }
        }
    });
}

/// A rejected item text's problem in the interface language.
fn describe_parse_error(err: &ParseError) -> String {
    match err {
        ParseError::Empty => tr!("Couldn't read the item").to_owned(),
        // The clipboard poll only returns text that looks like an item in *some* client
        // language, so reaching this means a client language the parser doesn't cover.
        ParseError::UnknownLanguage | ParseError::WrongLanguage { .. } => {
            tr!("Couldn't read the item: only the English and Russian game clients are supported")
                .to_owned()
        }
        ParseError::MissingNameplate => {
            tr!("Couldn't read the item: its name wasn't found").to_owned()
        }
        ParseError::UnrecognizedItemClass(class) => tr!(
            "Couldn't read the item: unknown item class “{class}”",
            class = class
        ),
        ParseError::Unrevealed => tr!(
            "This is a vendor's gamble: which item it is shows only once you buy it. Items like \
             this aren't sold on the trade site."
        )
        .to_owned(),
    }
}

/// The trade site's refusal, said the way the player can act on it: when to try again, and that
/// the limit counts every request from their IP -- the trade site open in their browser included
/// -- while the market prices, which don't go through it, still work.
fn rate_limit_message(retry_after_secs: Option<u64>) -> String {
    let when = match retry_after_secs {
        Some(secs) => tr!("in {wait}", wait = i18n::duration_secs(secs)),
        None => tr!("a little later").to_owned(),
    };
    tr!(
        "The trade site has limited searches for a while — try again {when}.\n\nThe limit counts \
         every request from your IP, your browser's included. Currency Exchange prices still \
         work.",
        when = when
    )
}

fn ctrl_is_down() -> bool {
    // High bit of the return value indicates the key is currently down; `GetAsyncKeyState`
    // returns a `u16`-repr `SHORT` in the `windows` crate, so a "negative" (high-bit-set) value
    // is the down state.
    (unsafe { GetAsyncKeyState(VK_CONTROL.0.into()) } as i16) < 0
}
