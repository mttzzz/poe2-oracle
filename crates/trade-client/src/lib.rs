//! PoE2 trade API client (leagues -> search -> fetch).
//!
//! Deliberately UI/runtime-agnostic: takes an already-constructed `Arc<dyn HttpClient>` rather
//! than building one itself (`reqwest_client::ReqwestClient` construction, including the
//! `USER_AGENT` choice, stays in `crates/poe2-oracle` -- the one place that actually knows what
//! kind of app is making the request). Item shapes come from `poe2-domain`, filter rows from
//! `stat-filters`, and `http_client` supplies the trait, not a concrete client.
//!
//! `catalog`/`cache`/`rate_limit` extend this crate beyond trade *listings*: `catalog` fetches
//! the stat/item/currency catalogs those listings get filtered against
//! (`/api/trade2/data/{stats,items,static}`), `cache` gives that slow-changing data a disk
//! cache, and `rate_limit` tracks the real rate-limit response headers so a caller that owns an
//! executor can hold off until the trade API will take its next request -- this crate itself
//! never sleeps. `account` asks whether the session the caller's client sends is signed in.
//!
//! Every trade API response, `catalog`'s included, goes through `checked_body`: a refusal (a `429`
//! while rate-limited, a rejected query, a Cloudflare error page) reaches the caller as a
//! [`TradeApiError`], never as a JSON parse error from feeding its body to a success-shape
//! parser. The account page is the exception: its 401 is an answer, not a refusal.

pub mod account;
pub mod cache;
pub mod catalog;
pub mod cx;
pub mod rate_limit;
pub mod rates;
pub mod scout;

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::AsyncReadExt;
use http_client::http::HeaderMap;
use http_client::{AsyncBody, HttpClient, Json, Response, StatusCode};
use serde::{Deserialize, Serialize};

use catalog::{ItemTypeEntry, StaticCurrency};
use poe2_domain::{ItemRarity, ParsedItem};
use rate_limit::RateLimiter;

/// Which GGG trade site a call goes to. The backend is shared -- league ids, stat ids, category
/// ids and exchange currency ids are identical on every subdomain (verified live 2026-09-22:
/// `ru.pathofexile.com` returns the same 8217 stat ids and the same league ids as `www`) -- but
/// every *text* field is localized: stat templates, static-currency names, league display text,
/// and the `type` values `search_exact` matches against. Russian clipboard text therefore has to
/// be matched against, and searched on, `ru.pathofexile.com`, the same way EE2's own `poeWebApi()`
/// (`renderer/src/web/Config.ts`) picks the subdomain from the game language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TradeSite {
    #[default]
    International,
    Russian,
}

impl TradeSite {
    /// Browser-facing origin, for building trade-site links (`{origin}/trade2/search/poe2/...`).
    pub fn origin(self) -> &'static str {
        match self {
            TradeSite::International => "https://www.pathofexile.com",
            TradeSite::Russian => "https://ru.pathofexile.com",
        }
    }

    pub(crate) fn api_base(self) -> &'static str {
        match self {
            TradeSite::International => "https://www.pathofexile.com/api/trade2",
            TradeSite::Russian => "https://ru.pathofexile.com/api/trade2",
        }
    }
}

/// The trade API's refusal of a request: any non-2xx response. GGG wraps its errors in a
/// `{"error":{"code":N,"message":"..."}}` envelope -- verified live 2026-09-22 on a search made
/// while rate-limited: `HTTP/2 429`, `retry-after: 259`, body
/// `{"error":{"code":3,"message":"Rate limit exceeded"}}`. Every network function in this crate
/// returns it under its request's `"{method} {url}"` context, so a caller finds it with
/// `err.downcast_ref::<TradeApiError>()` -- e.g. to show a rate-limit countdown rather than an
/// error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeApiError {
    pub status: u16,
    /// The envelope's `error.code`; `None` when the body isn't that envelope.
    pub code: Option<i64>,
    /// The envelope's `error.message`; otherwise the trimmed raw body, capped at 200 characters so
    /// an HTML error page can't flood the UI; otherwise, for an empty body, the status' canonical
    /// reason phrase.
    pub message: String,
    /// The response's integer-seconds `Retry-After` header, when present.
    pub retry_after_secs: Option<u64>,
}

impl TradeApiError {
    /// Whether the trade API's rate limiter refused the request: status `429`, or envelope code
    /// `3` ("Rate limit exceeded").
    pub fn is_rate_limited(&self) -> bool {
        self.status == 429 || self.code == Some(3)
    }

    /// Whether the site found the query too complex: envelope code `2`, which it gives any
    /// invalid query, with its «Query is too complex» message -- in English on www, in Russian on
    /// ru. An anonymous search with a weighted sum gets it (verified live 2026-09-23 on both), and
    /// so would one with too many filters for the account.
    pub fn is_too_complex(&self) -> bool {
        let message = self.message.to_lowercase();
        self.code == Some(2)
            && (message.contains("too complex") || message.contains("слишком сложный"))
    }
}

impl fmt::Display for TradeApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.message)
    }
}

impl std::error::Error for TradeApiError {}

/// [`TradeApiError::message`]'s cap on a raw (non-envelope) body, in characters.
const MAX_RAW_MESSAGE_CHARS: usize = 200;

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorEnvelopeBody,
}
#[derive(Deserialize)]
struct ErrorEnvelopeBody {
    code: i64,
    message: String,
}

/// Builds the [`TradeApiError`] for a non-2xx response; kept apart from `checked_body` as a pure
/// function so the envelope parsing and its fallbacks are testable without an `HttpClient`.
fn api_error(status: u16, headers: &HeaderMap, body: &str) -> TradeApiError {
    let retry_after_secs = rate_limit::retry_after_secs(headers);
    if let Ok(ErrorEnvelope { error }) = serde_json::from_str(body) {
        return TradeApiError {
            status,
            code: Some(error.code),
            message: error.message,
            retry_after_secs,
        };
    }
    let body = body.trim();
    let message = if body.is_empty() {
        StatusCode::from_u16(status)
            .ok()
            .and_then(|status| status.canonical_reason())
            .unwrap_or("no response body")
            .to_owned()
    } else {
        match body.char_indices().nth(MAX_RAW_MESSAGE_CHARS) {
            Some((cut, _)) => format!("{}…", &body[..cut]),
            None => body.to_owned(),
        }
    };
    TradeApiError {
        status,
        code: None,
        message,
        retry_after_secs,
    }
}

/// The one path every trade API response takes, `catalog`'s included: awaits `request` (the
/// in-flight `HttpClient::get`/`post_json` call for `method` `url`) and records the response's
/// status and headers on `limiter` -- the endpoint family's; `None` for the unthrottled
/// `leagues` and `catalog` GETs -- BEFORE judging the status, since a `429` carries exactly the
/// `Retry-After` the next request has to honour. A 2xx body is handed back for the caller's typed
/// JSON parse; anything else becomes a [`TradeApiError`] under `"{method} {url}"` context, even
/// when its body fails to read (the status is the verdict, the body only explains it). `what`
/// names the body in a read-failure context (`"reading {what} response body"`).
pub(crate) async fn checked_body(
    request: impl Future<Output = Result<Response<AsyncBody>>>,
    method: &str,
    url: &str,
    limiter: Option<&mut RateLimiter>,
    what: &str,
) -> Result<String> {
    let request_context = || format!("{method} {url}");
    let mut response = request.await.with_context(request_context)?;
    let status = response.status();
    if let Some(limiter) = limiter {
        limiter.record_response(status.as_u16(), response.headers());
        // The audit trail of the player's IP budget: what this response says is left of it.
        match rate_limit::describe_limits(response.headers()) {
            Some(limits) if status.as_u16() == 429 => {
                log::warn!("trade API refused the {what}: {limits}")
            }
            Some(limits) => log::info!("trade limits after the {what}: {limits}"),
            None if status.as_u16() == 429 => {
                log::warn!("trade API refused the {what}, saying nothing of its limits")
            }
            None => {}
        }
    }
    let mut body = String::new();
    let read = response.body_mut().read_to_string(&mut body).await;
    if !status.is_success() {
        let error = api_error(status.as_u16(), response.headers(), &body);
        return Err(anyhow::Error::new(error).context(request_context()));
    }
    read.with_context(|| format!("reading {what} response body"))?;
    Ok(body)
}

/// One league as `GET /api/trade2/data/leagues` lists it. `Serialize`/`Deserialize` for the app's
/// disk cache ([`cache::load_or_fetch`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct League {
    /// What searches name it by: the same on every site ("Forbidden Rites").
    pub id: String,
    /// Its name in the site's language: "Запретные ритуалы" on `ru`; the id itself on `www`, and on
    /// `ru` for a league the site leaves untranslated ("HC Forbidden Rites").
    pub text: String,
}

#[derive(Deserialize)]
struct LeaguesResponse {
    result: Vec<League>,
}

/// `GET /api/trade2/data/leagues` on `site` -- every currently-active league, most-current first
/// (verified live: taking `leagues[0]` gives the current top league, e.g. "Forbidden Rites"). Every
/// site lists the same ids in the same order; only `text` is localized (verified live 2026-09-23:
/// `ru` names "Forbidden Rites", "Runes of Aldur", "Standard" and "Hardcore" "Запретные ритуалы",
/// "Руны Альдура", "Стандарт" and "Одна жизнь", and leaves the "HC ..." leagues in English).
pub async fn leagues(client: &Arc<dyn HttpClient>, site: TradeSite) -> Result<Vec<League>> {
    let url = format!("{}/data/leagues", site.api_base());
    let request = client.get(&url, AsyncBody::default(), true);
    let body = checked_body(request, "GET", &url, None, "leagues").await?;
    let parsed: LeaguesResponse =
        serde_json::from_str(&body).context("parsing leagues response JSON")?;
    Ok(parsed.result)
}

/// Which listings a search matches by how their seller sells: the status choice EE2 offers as
/// its online toggle and PoE Overlay II as its "Instant Buyout" dropdown. The variants are the
/// trade site's own `status` options (verified live 2026-09-22 via
/// `GET /api/trade2/data/filters`; English labels quoted below). EE2's item search leaves out
/// `OnlineLeague`, offering it for bulk exchange only, but the trade site's own search form lists
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListingStatus {
    /// "Instant Buyout and In Person".
    Available,
    /// "Instant Buyout" -- EE2's default for item searches.
    Securable,
    /// "In Person (Online in League)".
    OnlineLeague,
    /// "In Person (Online)".
    Online,
    /// "Any": offline sellers too.
    Any,
}

impl ListingStatus {
    /// The `query.status.option` value.
    fn option(self) -> &'static str {
        match self {
            ListingStatus::Available => "available",
            ListingStatus::Securable => "securable",
            ListingStatus::OnlineLeague => "onlineleague",
            ListingStatus::Online => "online",
            ListingStatus::Any => "any",
        }
    }
}

/// The rarities a filtered search admits (`query.filters.type_filters.filters.rarity`), among the
/// live options (verified 2026-09-23): the item's own rarity, every non-unique one, and `unique`
/// for an unidentified unique, whose base alone would also list its normal, magic and rare copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RarityFilter {
    Normal,
    Magic,
    Rare,
    /// Every rarity but unique.
    NonUnique,
    Unique,
}

impl RarityFilter {
    fn option(self) -> &'static str {
        match self {
            RarityFilter::Normal => "normal",
            RarityFilter::Magic => "magic",
            RarityFilter::Rare => "rare",
            RarityFilter::NonUnique => "nonunique",
            RarityFilter::Unique => "unique",
        }
    }
}

/// Which items a filtered search considers at all, before its stat and property rows narrow them
/// down: EE2's `searchExact`/`searchRelaxed` pair, of which `createTradeRequest` sends one
/// (`pathofexile-trade.ts:617-643`), plus its rarity filter. [`route_search`] fills it in the way
/// EE2's `createFilters` does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchScope {
    /// `query.name`: an identified unique's name.
    pub name: Option<String>,
    /// `query.type`: the base type, when this is EE2's exact search.
    pub base_type: Option<String>,
    /// The trade category (`query.filters.type_filters.filters.category`), when this is EE2's
    /// relaxed search.
    pub category: Option<String>,
    pub rarity: Option<RarityFilter>,
    /// How many of the enabled stat rows a listing must match: all of them, or -- a search that
    /// found nothing, relaxed by the player -- at least some.
    pub stat_match: StatMatch,
    /// Corrupted, mirrored, sanctified and fractured listings the search leaves out or asks for.
    pub misc: MiscChoices,
    /// The currency a listing's price must be in.
    pub price: PriceCurrency,
}

/// The currency a listing's price must be in (`trade_filters.filters.price.option`): EE2's
/// choices (`OnlineFilter.vue`), the answer to price-fixers asking a rare currency and to players
/// who only deal in exalted orbs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PriceCurrency {
    #[default]
    Any,
    ExaltedOrDivine,
    Exalted,
    Divine,
    Chaos,
}

impl PriceCurrency {
    fn option(self) -> Option<&'static str> {
        match self {
            PriceCurrency::Any => None,
            PriceCurrency::ExaltedOrDivine => Some("exalted_divine"),
            PriceCurrency::Exalted => Some("exalted"),
            PriceCurrency::Divine => Some("divine"),
            PriceCurrency::Chaos => Some("chaos"),
        }
    }
}

/// The trade site's `misc_filters` a filtered search sets: the item's own state, matched -- what
/// PoE Overlay II and Sidekick send (compared on 2026-09-23 after the owner found EE2's defaults,
/// which drop half of it, pricing an unidentified waystone among 10000 identified ones instead of
/// 72 unidentified). For the yes/no ones `Some(false)` leaves listings with the property out,
/// `Some(true)` asks for them, `None` takes either. An uncorrupted item goes among uncorrupted
/// listings -- items that can still be modified, which a buyer of a base or of a waystone to roll
/// is after -- and a corrupted one among corrupted ones, whose implicits and sockets it shares;
/// the same for mirrored and sanctified items. An unidentified item goes among unidentified
/// listings, which are what it sells as: an identified one shows its mods. The panel lets the
/// player drop the corruption and identification choices for the next search.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MiscChoices {
    pub corrupted: Option<bool>,
    pub mirrored: Option<bool>,
    pub sanctified: Option<bool>,
    pub fractured: Option<bool>,
    pub identified: Option<bool>,
    /// The least unidentified tier a listing must have (`unidentified_tier.min`).
    pub unidentified_tier: Option<u32>,
}

impl MiscChoices {
    /// The choices for `item`, whose trade category is `category_id`; `exact` is EE2's exact
    /// preset (`stat_filters::uses_exact_preset`), which also leaves fractured items out.
    fn for_item(item: &ParsedItem, category_id: Option<&str>, exact: bool) -> MiscChoices {
        let gem = category_id.is_some_and(|id| id.starts_with("gem"));
        // An unmodifiable item's corruption changes nothing a buyer could do with it.
        let has_state = (gem || item.rarity.is_some()) && !item.is_unmodifiable;
        let gear = has_state && !gem;
        // An unidentified item with a tier is matched by it from tier 5 up (EE2).
        let unidentified_tier = item
            .unidentified_tier
            .filter(|&tier| item.is_unidentified && tier >= 5);
        MiscChoices {
            corrupted: has_state.then_some(item.is_corrupted),
            mirrored: gear.then_some(item.is_mirrored),
            sanctified: gear.then_some(item.is_sanctified),
            fractured: (exact && !item.is_fractured).then_some(false),
            identified: item.is_unidentified.then_some(false),
            unidentified_tier,
        }
    }

    /// `(misc_filters key, value)` for each choice the search sets.
    fn filters(self) -> impl Iterator<Item = (&'static str, QueryFilter<'static>)> {
        let yes_no = [
            ("corrupted", self.corrupted),
            ("mirrored", self.mirrored),
            ("sanctified", self.sanctified),
            ("fractured_item", self.fractured),
            ("identified", self.identified),
        ]
        .into_iter()
        .filter_map(|(key, choice)| {
            let option = if choice? { "true" } else { "false" };
            Some((key, QueryFilter::Option(OptionField { option })))
        });
        let tier = self.unidentified_tier.map(|tier| {
            let min = Some(f64::from(tier));
            let bounds = StatFilterValue {
                min,
                max: None,
                weight: None,
            };
            ("unidentified_tier", QueryFilter::Range(bounds))
        });
        yes_no.chain(tier)
    }
}

#[derive(Serialize)]
struct FilteredSearchRequestBody<'a> {
    query: FilteredSearchQuery<'a>,
    sort: SearchSort,
}
#[derive(Serialize)]
struct FilteredSearchQuery<'a> {
    status: OptionField<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    type_: Option<&'a str>,
    stats: Vec<StatGroup<'a>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    filters: BTreeMap<&'a str, QueryFilterGroup<'a>>,
}
/// A `{"option": ...}` choice: a listing status, category or rarity.
#[derive(Serialize)]
struct OptionField<'a> {
    option: &'a str,
}
#[derive(Serialize)]
struct StatGroup<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    /// A `"count"` group's `{"min": k}`: how many of its filters a listing must match.
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<StatFilterValue>,
    /// A stat row's own `"count"` group while the row is unchecked (see `stat_groups`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    disabled: bool,
    filters: Vec<StatFilterEntry<'a>>,
}
/// One `query.stats[].filters[]` entry (see `search_with_filters` for which rows get one).
#[derive(Serialize)]
struct StatFilterEntry<'a> {
    id: &'a str,
    value: StatFilterValue,
    disabled: bool,
}
/// A stat or property filter's bounds. An unset bound is omitted -- neither `null` nor filled
/// in -- which leaves that side of the search open: the request shape EE2 sends when its
/// min-only default leaves `roll.max` `undefined`. A weighted sum's entry carries its `weight`
/// instead.
#[derive(Serialize)]
struct StatFilterValue {
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    weight: Option<f64>,
}
/// One `query.filters.<group>` object (`type_filters`, `equipment_filters`, ...).
#[derive(Serialize, Default)]
struct QueryFilterGroup<'a> {
    filters: BTreeMap<&'a str, QueryFilter<'a>>,
}
/// One `query.filters.<group>.filters.<key>` value: a choice (category, rarity) or a property
/// row's bounds.
#[derive(Serialize)]
#[serde(untagged)]
enum QueryFilter<'a> {
    Option(OptionField<'a>),
    Range(StatFilterValue),
}
#[derive(Serialize)]
struct SearchSort {
    price: &'static str,
}

#[derive(Deserialize)]
struct SearchResponse {
    id: String,
    result: Vec<String>,
    total: u64,
}

/// The listing-id page and query id `fetch` needs -- shared by `search_with_filters` and
/// `search_exact`, which both need a separate `fetch` call afterward for the listings themselves.
pub struct SearchOutcome {
    pub query_id: String,
    pub total: u64,
    pub listing_ids: Vec<String>,
}

/// How many of the enabled stat rows a listing must match: every one (the trade site's `"and"`
/// group), or at least some -- its `"count"` group, what the player can relax a search that found
/// nothing to (`one_fewer_match`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatMatch {
    #[default]
    All,
    AtLeast(u32),
}

/// How many stat rows a search asks for: the enabled ones with a trade id (properties are placed
/// elsewhere and always kept). A row counts once however many trade ids it has.
pub fn enabled_stat_rows(filters: &[stat_filters::SearchFilter]) -> u32 {
    filters
        .iter()
        .filter(|filter| filter.enabled)
        .filter_map(stat_filter_entries)
        .count() as u32
}

/// The relaxed search the player can ask for when matching every enabled stat row found nothing:
/// listings with all of them but one, as `(least, of)` -- at least `least` of the `of` rows
/// (`StatMatch::AtLeast(least)`). `None` under two rows: all but one of one would match anything.
pub fn one_fewer_match(filters: &[stat_filters::SearchFilter]) -> Option<(u32, u32)> {
    let wanted = enabled_stat_rows(filters);
    (wanted >= 2).then(|| (wanted - 1, wanted))
}

/// `POST /api/trade2/search/{league}` for gear and everything else priced by its mods and
/// properties: the items `scope` admits, listed with `status`, narrowed by `filters`, cheapest
/// first. `league` is the raw league id (may contain spaces, e.g. "Forbidden Rites") -- this
/// function URL-encodes it itself.
///
/// Stat rows (every tag but `Property`) with a trade id go into `query.stats` (see
/// `stat_groups`), each sent regardless of `enabled`, with `disabled: !enabled`: the real trade
/// site's own request shape, where an unchecked filter row still round-trips through the query
/// object, so a value a player edited while unchecked isn't lost. A property row goes where its
/// `<group>.<key>` trade id points, `query.filters.<group>.filters.<key>`, and only while
/// enabled: EE2 leaves an unchecked property out of the query (`createTradeRequest` skips a
/// disabled row before placing it, `pathofexile-trade.ts:900`). Either kind carries only the
/// bounds actually set (`roll.min`/`roll.max`): an unset bound stays open. Every search folds a
/// seller's listings of one item into one (`trade_filters.collapse`), as PoE Overlay II's does.
pub async fn search_with_filters(
    client: &Arc<dyn HttpClient>,
    site: TradeSite,
    league: &str,
    scope: &SearchScope,
    filters: &[stat_filters::SearchFilter],
    status: ListingStatus,
    limiter: &mut RateLimiter,
) -> Result<SearchOutcome> {
    let body = filtered_search_body(scope, filters, status);
    let url = format!("{}/search/{}", site.api_base(), encode_league(league));
    let request = client.post_json(&url, AsyncBody::from(Json(body)));
    let response_body = checked_body(request, "POST", &url, Some(limiter), "search").await?;
    let parsed: SearchResponse =
        serde_json::from_str(&response_body).context("parsing search response JSON")?;
    Ok(SearchOutcome {
        query_id: parsed.id,
        total: parsed.total,
        listing_ids: parsed.result,
    })
}

/// `search_with_filters`'s request body.
fn filtered_search_body<'a>(
    scope: &'a SearchScope,
    filters: &'a [stat_filters::SearchFilter],
    status: ListingStatus,
) -> FilteredSearchRequestBody<'a> {
    let mut query_filters: BTreeMap<&str, QueryFilterGroup> = BTreeMap::new();
    let choices = [
        ("category", scope.category.as_deref()),
        ("rarity", scope.rarity.map(RarityFilter::option)),
    ];
    for (key, option) in choices {
        if let Some(option) = option {
            let choice = QueryFilter::Option(OptionField { option });
            let group = query_filters.entry("type_filters").or_default();
            group.filters.insert(key, choice);
        }
    }
    for (key, filter) in scope.misc.filters() {
        let group = query_filters.entry("misc_filters").or_default();
        group.filters.insert(key, filter);
    }
    let trade_filters = query_filters.entry("trade_filters").or_default();
    trade_filters.filters.insert("collapse", COLLAPSE);
    if let Some(option) = scope.price.option() {
        trade_filters
            .filters
            .insert("price", QueryFilter::Option(OptionField { option }));
    }
    for (group, key, bounds) in filters.iter().filter_map(property_filter) {
        let group = query_filters.entry(group).or_default();
        group.filters.insert(key, QueryFilter::Range(bounds));
    }
    FilteredSearchRequestBody {
        query: FilteredSearchQuery {
            status: OptionField {
                option: status.option(),
            },
            name: scope.name.as_deref(),
            type_: scope.base_type.as_deref(),
            stats: stat_groups(filters, scope.stat_match),
            filters: query_filters,
        },
        sort: SearchSort { price: "asc" },
    }
}

/// `query.stats` for the stat rows, as EE2's `createTradeRequest` builds it
/// (`pathofexile-trade.ts:1215-1224`): a row with one trade id goes into the `"and"` group, and a
/// row whose text several trade ids share (`# to all Attributes` is both
/// `explicit.stat_1379411836` and `explicit.stat_2897413282`) gets a `"count"` group of its own
/// that any one of them satisfies -- sending only the first would lose every listing whose mod
/// the site files under another. A relaxed search (`StatMatch::AtLeast`) puts every row's ids
/// into its one `"count"` group instead, where a listing matches a row through any of them. A
/// weighted sum is its own `"weight2"` group in either (`weighted_sum_group`).
fn stat_groups(
    filters: &[stat_filters::SearchFilter],
    stat_match: StatMatch,
) -> Vec<StatGroup<'_>> {
    let rows = filters
        .iter()
        .filter_map(|filter| Some((filter, stat_filter_entries(filter)?)));
    let weighted_sums = filters.iter().filter_map(weighted_sum_group);
    if let StatMatch::AtLeast(count) = stat_match {
        let relaxed = StatGroup {
            kind: "count",
            value: Some(StatFilterValue {
                min: Some(f64::from(count)),
                max: None,
                weight: None,
            }),
            disabled: false,
            filters: rows.flat_map(|(_, entries)| entries).collect(),
        };
        return std::iter::once(relaxed).chain(weighted_sums).collect();
    }
    let mut every = StatGroup {
        kind: "and",
        value: None,
        disabled: false,
        filters: Vec::new(),
    };
    let mut any_of = Vec::new();
    for (filter, entries) in rows {
        if entries.len() == 1 {
            every.filters.extend(entries);
        } else {
            any_of.push(StatGroup {
                kind: "count",
                value: Some(StatFilterValue {
                    min: Some(1.0),
                    max: None,
                    weight: None,
                }),
                disabled: !filter.enabled,
                filters: entries,
            });
        }
    }
    std::iter::once(every)
        .chain(any_of)
        .chain(weighted_sums)
        .collect()
}

/// A weighted sum's `"weight2"` group, as PoE Overlay II sends one: every trade id at weight 1,
/// the group's value the row's bounds, unchecked with the row. `None` for any other row.
fn weighted_sum_group(filter: &stat_filters::SearchFilter) -> Option<StatGroup<'_>> {
    if !filter.weighted_sum {
        return None;
    }
    let roll = filter.roll.as_ref()?;
    Some(StatGroup {
        kind: "weight2",
        value: Some(StatFilterValue {
            min: roll.min,
            max: roll.max,
            weight: None,
        }),
        disabled: !filter.enabled,
        filters: filter
            .trade_ids
            .iter()
            .map(|id| StatFilterEntry {
                id,
                value: StatFilterValue {
                    min: None,
                    max: None,
                    weight: Some(1.0),
                },
                disabled: !filter.enabled,
            })
            .collect(),
    })
}

/// A stat row's `query.stats` entries, one per trade id, carrying the row's bounds -- none for a
/// row without a roll: a flag stat (`Enemies in your Presence are Blinded`), which a listing
/// matches by having it, sent with an empty `value` as EE2's `tradeIdToQuery` sends one. A row
/// in the item's own words (`SearchFilter::inverted`) has its bounds negated and swapped back
/// into the catalog's terms. `None` for a row without a trade id, for a property row, which
/// `property_filter` places instead, and for a weighted sum (`weighted_sum_group`).
fn stat_filter_entries(filter: &stat_filters::SearchFilter) -> Option<Vec<StatFilterEntry<'_>>> {
    if filter.tag == stat_filters::FilterTag::Property
        || filter.weighted_sum
        || filter.trade_ids.is_empty()
    {
        return None;
    }
    let (min, max) = filter
        .roll
        .as_ref()
        .map_or((None, None), |roll| (roll.min, roll.max));
    let (min, max) = if filter.inverted {
        (max.map(|max| -max), min.map(|min| -min))
    } else {
        (min, max)
    };
    Some(
        filter
            .trade_ids
            .iter()
            .map(|id| StatFilterEntry {
                id,
                value: StatFilterValue {
                    min,
                    max,
                    weight: None,
                },
                disabled: !filter.enabled,
            })
            .collect(),
    )
}

/// Where an enabled property row goes: the `(group, key)` its `<group>.<key>` trade id names,
/// with its bounds. `None` for a stat row and for a disabled property.
fn property_filter(filter: &stat_filters::SearchFilter) -> Option<(&str, &str, StatFilterValue)> {
    if filter.tag != stat_filters::FilterTag::Property || !filter.enabled {
        return None;
    }
    let (group, key) = filter.trade_ids.first()?.split_once('.')?;
    let roll = filter.roll.as_ref()?;
    Some((
        group,
        key,
        StatFilterValue {
            min: roll.min,
            max: roll.max,
            weight: None,
        },
    ))
}

#[derive(Serialize)]
struct ExactSearchRequestBody<'a> {
    query: ExactSearchQuery<'a>,
    sort: SearchSort,
}
#[derive(Serialize)]
struct ExactSearchQuery<'a> {
    status: OptionField<'static>,
    #[serde(rename = "type")]
    type_: &'a str,
    filters: BTreeMap<&'static str, QueryFilterGroup<'static>>,
}

/// `trade_filters.collapse`: a seller's listings of one item as one.
const COLLAPSE: QueryFilter<'static> = QueryFilter::Option(OptionField { option: "true" });

/// `POST /api/trade2/search/{league}` for name/base-type-exact items with no useful mod search
/// (Unique-by-name, Divination Cards, Skill/Support/Meta Gems): `query.type` exact-matches
/// `exact_type` instead of building a `query.stats` filter group, among listings matching
/// `status`. `exact_type` is the item's own `name` (for an unidentified Unique this is the
/// base-type text, the only line the game prints pre-identification -- see `item-parser`'s
/// nameplate handling) -- the trade API's `type` field is overloaded to accept either a unique
/// name or a plain base type. No category constraint is sent; exact-type matching alone is
/// precise enough for this item class.
pub async fn search_exact(
    client: &Arc<dyn HttpClient>,
    site: TradeSite,
    league: &str,
    exact_type: &str,
    status: ListingStatus,
    limiter: &mut RateLimiter,
) -> Result<SearchOutcome> {
    let body = ExactSearchRequestBody {
        query: ExactSearchQuery {
            status: OptionField {
                option: status.option(),
            },
            type_: exact_type,
            filters: BTreeMap::from([(
                "trade_filters",
                QueryFilterGroup {
                    filters: BTreeMap::from([("collapse", COLLAPSE)]),
                },
            )]),
        },
        sort: SearchSort { price: "asc" },
    };
    let url = format!("{}/search/{}", site.api_base(), encode_league(league));
    let request = client.post_json(&url, AsyncBody::from(Json(body)));
    let response_body = checked_body(request, "POST", &url, Some(limiter), "exact-search").await?;
    let parsed: SearchResponse =
        serde_json::from_str(&response_body).context("parsing exact-search response JSON")?;
    Ok(SearchOutcome {
        query_id: parsed.id,
        total: parsed.total,
        listing_ids: parsed.result,
    })
}

/// How a parsed item is priced.
pub enum SearchRoute {
    /// Traded on the in-game Currency Exchange, which the trade site's listings don't reflect:
    /// priced from the exchange market (see `cx`), never searched. `trade_id` is the item's
    /// `/data/static` id, the id the market is keyed by.
    Market {
        trade_id: String,
    },
    Exact {
        exact_type: String,
    },
    Filtered {
        scope: SearchScope,
    },
}

/// Decides how `item` is priced, and resolves whatever parameters can be derived from the item
/// alone, plus `static_currencies` (`catalog::fetch_static_currencies`: the trade site's exchange
/// catalog) and `item_types` (`catalog::fetch_item_types` for the item's site, to find a magic
/// item's base type). Callers still supply their own `league`/`client`/filters/limiter.
///
/// Routing rules:
/// - an item named exactly like an exchange catalog entry -- currency, omens, runes, essences,
///   catalysts, soul cores, fragments, uncut and lineage gems, white waystones (`Waystone (Tier
///   N)`): everything the in-game Currency Exchange trades -- routes to `Market`. Not
///   magic/rare/unique items (a random rare name never means an exchange entry, a rare waystone's
///   value is in its mods) and not Expedition Logbooks, which EE2 prices by area level although
///   the exchange lists them;
/// - any other `Rarity: Currency` item (e.g. an Inscribed Ultimatum, verified live 2026-09-22 to
///   be missing from the catalog), a map fragment missing from the exchange and Divination Cards
///   route to `Exact` by `item.name` -- a fragment category search would list every other
///   fragment;
/// - gems route to `Filtered` by their type, which `stat_filters` gives level, quality and socket
///   rows (EE2's `createGemFilters`);
/// - everything else routes to `Filtered`, scoped the way EE2 scopes it (see `filtered_scope`).
pub fn route_search(
    item: &ParsedItem,
    static_currencies: &[StaticCurrency],
    item_types: &[ItemTypeEntry],
) -> SearchRoute {
    let category_id = item.category.as_ref().map(|c| c.id.as_str());
    let exchange_candidate = category_id != Some("map.logbook")
        && !matches!(
            item.rarity,
            Some(ItemRarity::Magic | ItemRarity::Rare | ItemRarity::Unique)
        );
    if exchange_candidate
        && let Some(entry) = static_currencies
            .iter()
            .find(|currency| currency.display_name == item.name)
    {
        return SearchRoute::Market {
            trade_id: entry.id.clone(),
        };
    }

    if category_id.is_some_and(|id| id.starts_with("gem")) {
        return SearchRoute::Filtered {
            scope: SearchScope {
                base_type: Some(item.name.clone()),
                misc: MiscChoices::for_item(item, category_id, false),
                ..SearchScope::default()
            },
        };
    }
    let is_currency = category_id.is_some_and(|id| id == "currency" || id.starts_with("currency."));
    let is_fragment = category_id == Some("map.fragment");
    let is_divination_card = category_id == Some("card");
    if is_currency || is_fragment || is_divination_card {
        return SearchRoute::Exact {
            exact_type: item.name.clone(),
        };
    }

    SearchRoute::Filtered {
        scope: filtered_scope(item, item_types),
    }
}

/// What EE2 matches a filtered search's item by (`createFilters`,
/// `create-item-filters.ts:135-220`): an identified unique by its name and base type, an
/// unidentified one -- which shows only its base -- by its base among unidentified uniques;
/// anything else by its trade category (EE2's `searchRelaxed`), unless EE2 prices it with its
/// exact preset (`stat_filters::uses_exact_preset`) or knows no category for it -- then by its
/// base type (`searchExact`). Waystones and charms go by category either way. A base type this
/// crate can't resolve falls back to the category. The rarity filter and the misc choices are
/// EE2's (`create-item-filters.ts:303-366`), but for the unidentified unique's `unique`.
fn filtered_scope(item: &ParsedItem, item_types: &[ItemTypeEntry]) -> SearchScope {
    let category_id = item.category.as_ref().map(|category| category.id.as_str());
    let exact = stat_filters::uses_exact_preset(item);
    let misc = MiscChoices::for_item(item, category_id, exact);
    if item.rarity == Some(ItemRarity::Unique) {
        return if item.is_unidentified {
            SearchScope {
                base_type: resolve_base_type(item, item_types),
                rarity: Some(RarityFilter::Unique),
                misc,
                ..SearchScope::default()
            }
        } else {
            SearchScope {
                name: Some(item.name.clone()),
                base_type: item.base_type.clone(),
                misc,
                ..SearchScope::default()
            }
        };
    }
    let by_category = category_id.is_some_and(|id| {
        has_category_search(id) && (!exact || matches!(id, "map.waystone" | "flask.charm"))
    });
    let base_type = if by_category {
        None
    } else {
        resolve_base_type(item, item_types)
    };
    SearchScope {
        name: None,
        category: category_id
            .filter(|_| base_type.is_none())
            .map(str::to_owned),
        base_type,
        rarity: own_rarity(item),
        stat_match: StatMatch::All,
        misc,
        price: PriceCurrency::Any,
    }
}

/// The other way to scope a filtered search of `item`: by its base type where `scope` goes by
/// category, and back -- EE2's item-type chip, its `searchExact`/`searchRelaxed` pair. A rare
/// is priced against its whole category by default, which misses what a base itself is worth: a
/// base implicit, a rare base type. `None` where there's no other way: a unique goes by its name,
/// a gem only by its type, and an item needs both a trade category and a base type this crate
/// can resolve.
pub fn switched_scope(
    scope: &SearchScope,
    item: &ParsedItem,
    item_types: &[ItemTypeEntry],
) -> Option<SearchScope> {
    if scope.name.is_some() || scope.rarity == Some(RarityFilter::Unique) {
        return None;
    }
    let category = item
        .category
        .as_ref()
        .map(|category| category.id.as_str())
        .filter(|id| has_category_search(id))?;
    if scope.category.is_some() {
        Some(SearchScope {
            category: None,
            base_type: Some(resolve_base_type(item, item_types)?),
            ..scope.clone()
        })
    } else {
        Some(SearchScope {
            category: Some(category.to_owned()),
            base_type: None,
            ..scope.clone()
        })
    }
}

/// Whether EE2 has a trade category for this kind of item (`CATEGORY_TO_TRADE_ID`,
/// `pathofexile-trade.ts:35-87`) and so can search it by category at all: gear, jewels, flasks
/// and charms, waystones, tablets and fragments. EE2 has none for PoE2's relics, logbooks,
/// breachstones, pinnacle keys, ultimatums and baryas, which it searches by base type.
fn has_category_search(category_id: &str) -> bool {
    category_id.starts_with("weapon.")
        || category_id.starts_with("armour.")
        || category_id.starts_with("accessory.")
        || matches!(
            category_id,
            "jewel"
                | "flask.life"
                | "flask.mana"
                | "flask.charm"
                | "map.waystone"
                | "map.tablet"
                | "map.fragment"
        )
}

/// The rarity a filtered search admits by default: the item's own, as PoE Overlay II searches
/// (EE2 widens most items to every non-unique): a magic item among magic ones -- its one prefix
/// and one suffix are the whole item, and a buyer after a magic base doesn't take a rare -- a rare
/// among rares, which a magic item sharing its few selected mods would price low, and a normal
/// base among normal ones. [`other_rarity`] is the panel's way to every non-unique. Uniques get
/// none: their name pins them down.
fn own_rarity(item: &ParsedItem) -> Option<RarityFilter> {
    match item.rarity? {
        ItemRarity::Normal => Some(RarityFilter::Normal),
        ItemRarity::Magic => Some(RarityFilter::Magic),
        ItemRarity::Rare => Some(RarityFilter::Rare),
        ItemRarity::Unique => None,
    }
}

/// The other rarity a filtered search of `item` can admit than `current`: every non-unique one
/// where it admits only the item's own, and back. `None` for a unique, or where `current` is
/// neither (an unidentified unique's `unique`).
pub fn other_rarity(current: RarityFilter, item: &ParsedItem) -> Option<RarityFilter> {
    let own = own_rarity(item)?;
    if current == RarityFilter::NonUnique {
        Some(own)
    } else if current == own {
        Some(RarityFilter::NonUnique)
    } else {
        None
    }
}

/// The item's base type in its site's language: the nameplate's base-type line where it has one
/// (rares, uniques), else its name -- a Normal or unidentified item's name is its base once
/// `item-parser` drops the Superior/Exceptional wording -- except for an identified magic item,
/// whose name wraps the base in affixes (`magic_base_type`).
fn resolve_base_type(item: &ParsedItem, item_types: &[ItemTypeEntry]) -> Option<String> {
    if let Some(base_type) = &item.base_type {
        return Some(base_type.clone());
    }
    if item.rarity == Some(ItemRarity::Magic) && !item.is_unidentified {
        return magic_base_type(&item.name, item_types).map(str::to_owned);
    }
    Some(item.name.clone())
}

/// EE2's `magicBasetype` (`parser/magic-name.ts`): a magic item's name wraps its base type in
/// affixes ("Crackling Temple Maul of the Brute", "Здоровые Поножи ваал тролля"), so the base is
/// the longest run of whole words naming a base type -- here an entry of the site's own
/// `/data/items` catalog, the table EE2 itself falls back to (`TRADE_ITEM_BY_REF`), so a Russian
/// name finds its Russian base.
fn magic_base_type<'a>(name: &str, item_types: &'a [ItemTypeEntry]) -> Option<&'a str> {
    let mut words = Vec::new();
    let mut start = 0;
    for word in name.split(' ') {
        words.push((start, start + word.len()));
        start += word.len() + 1;
    }
    let mut best: Option<&'a str> = None;
    for (first, &(start, _)) in words.iter().enumerate() {
        for &(_, end) in &words[first..] {
            let run = &name[start..end];
            // EE2 sorts its matches longest first, so the earliest of the longest wins.
            if best.is_some_and(|best| best.chars().count() >= run.chars().count()) {
                continue;
            }
            if let Some(entry) = item_types.iter().find(|entry| entry.type_name == run) {
                best = Some(entry.type_name.as_str());
            }
        }
    }
    best
}

#[derive(Deserialize)]
struct FetchResponse {
    result: Vec<Option<FetchResultItem>>,
}
/// One `result[]` entry. Past the live-verified basics (`name`, `typeLine`, `ilvl`, `price`,
/// `account.name`, `indexed`), fields follow EE2's `FetchResult` typing
/// (`renderer/src/web/price-check/trade/pathofexile-trade.ts`), whose `requestResults` reads them
/// into its results table; [`FetchedItem`] documents what each one means.
#[derive(Deserialize)]
struct FetchResultItem {
    item: FetchItem,
    listing: FetchListing,
    gone: Option<bool>,
}
/// `result[].item`: what [`ListedItem`] reads. The fields only the item card shows go through
/// [`lenient`], so a line the site words in a shape this crate doesn't expect costs that line,
/// never the listing: its price and seller still reach the results table.
#[derive(Deserialize)]
struct FetchItem {
    name: String,
    #[serde(rename = "typeLine")]
    type_line: String,
    rarity: Option<String>,
    #[serde(default, rename = "frameType", deserialize_with = "lenient")]
    frame_type: Option<u32>,
    ilvl: Option<u32>,
    identified: Option<bool>,
    #[serde(default, rename = "unidentifiedTier", deserialize_with = "lenient")]
    unidentified_tier: Option<u32>,
    #[serde(default)]
    corrupted: bool,
    /// The game's Mirrored: GGG's item field is `duplicated`, the trade site's filter `mirrored`,
    /// so either counts.
    #[serde(default)]
    duplicated: bool,
    #[serde(default)]
    mirrored: bool,
    #[serde(default)]
    sanctified: bool,
    #[serde(default)]
    fractured: bool,
    icon: Option<String>,
    note: Option<String>,
    #[serde(rename = "stackSize")]
    stack_size: Option<u32>,
    #[serde(default, deserialize_with = "lenient")]
    properties: Vec<RawProperty>,
    #[serde(default, deserialize_with = "lenient")]
    requirements: Vec<RawProperty>,
    #[serde(default, rename = "grantedSkills", deserialize_with = "lenient")]
    granted_skills: Vec<RawProperty>,
    /// Only counted: what fills them comes as `socketedItems`.
    #[serde(default, deserialize_with = "lenient")]
    sockets: Vec<serde::de::IgnoredAny>,
    /// A skill gem's support sockets, counted the same way.
    #[serde(default, rename = "gemSockets", deserialize_with = "lenient")]
    gem_sockets: Vec<serde::de::IgnoredAny>,
    #[serde(default, rename = "socketedItems", deserialize_with = "lenient")]
    socketed_items: Vec<RawSocketed>,
    #[serde(default, rename = "flavourText", deserialize_with = "lenient")]
    flavour_text: Vec<String>,
    #[serde(rename = "descrText")]
    descr_text: Option<String>,
    #[serde(default, rename = "enchantMods")]
    enchant_mods: Vec<RawMod>,
    #[serde(default, rename = "runeMods")]
    rune_mods: Vec<RawMod>,
    #[serde(default, rename = "implicitMods")]
    implicit_mods: Vec<RawMod>,
    #[serde(default, rename = "fracturedMods")]
    fractured_mods: Vec<RawMod>,
    #[serde(default, rename = "explicitMods")]
    explicit_mods: Vec<RawMod>,
    #[serde(default, rename = "desecratedMods")]
    desecrated_mods: Vec<RawMod>,
    #[serde(default, rename = "craftedMods")]
    crafted_mods: Vec<RawMod>,
}
/// One `properties`, `requirements` or `grantedSkills` entry: `{"name": "[Evasion|Уклонение]",
/// "values": [["500", 1]], "displayMode": 0, "type": 17}`.
#[derive(Deserialize)]
struct RawProperty {
    name: String,
    #[serde(default)]
    values: Vec<(String, u32)>,
    #[serde(default, rename = "displayMode")]
    display_mode: u32,
}
/// One `socketedItems` entry: the rune, soul core or talisman in the socket at index `socket`.
#[derive(Deserialize)]
struct RawSocketed {
    #[serde(rename = "typeLine")]
    type_line: String,
    socket: Option<usize>,
}
/// Reads a field only the item card shows, falling back to its default where the site words it
/// in a shape this crate doesn't expect (`null` included): a card line lost beats a listing lost.
fn lenient<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}
/// One entry of an item's mod list, in either form the trade API sends.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawMod {
    /// Plain text: PoE1's form, and what EE2's `FetchResult` typing expects.
    Text(String),
    /// PoE2's described form (live 2026-09-23): `{"description": "+16% to [Resistances|Cold
    /// Resistance]", "hash": "stat.explicit.stat_4220027924", "mods": [{"name": "of the
    /// Narwhal", "tier": "S6", ...}]}`.
    Described {
        description: String,
        hash: Option<String>,
        #[serde(default)]
        mods: Vec<RawModTier>,
    },
}
/// One affix behind a described mod line.
#[derive(Deserialize)]
struct RawModTier {
    tier: Option<String>,
    level: Option<u32>,
    /// The range of each number the affix rolls: `[{"min": "142", "max": "161"}]`.
    #[serde(default, deserialize_with = "lenient")]
    magnitudes: Vec<RawMagnitude>,
}
/// A roll range, whose bounds the live site sends as strings (`"23.1"`).
#[derive(Deserialize)]
struct RawMagnitude {
    min: serde_json::Value,
    max: serde_json::Value,
}
#[derive(Deserialize)]
struct FetchListing {
    price: Option<FetchPrice>,
    account: FetchAccount,
    indexed: String,
    /// Only its presence is read (`FetchedItem::instant_buyout`).
    fee: Option<serde::de::IgnoredAny>,
    in_demand: Option<bool>,
    whisper: Option<String>,
}
#[derive(Deserialize)]
struct FetchPrice {
    amount: f64,
    currency: String,
}
#[derive(Deserialize)]
struct FetchAccount {
    name: String,
    /// `null` (or absent) while the seller is offline.
    online: Option<FetchOnline>,
}
#[derive(Deserialize)]
struct FetchOnline {
    status: Option<FetchOnlineStatus>,
}
/// `account.online.status`: EE2 singles out `"afk"`; any other value still counts as online.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum FetchOnlineStatus {
    Afk,
    #[serde(other)]
    Other,
}

/// A seller's presence, read from `listing.account.online` the way EE2's `requestResults` reads
/// it: no `online` object means offline, one whose `status` is `"afk"` means AFK, any other means
/// online.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    Online,
    Afk,
    Offline,
}

/// Which of a listed item's mod lists a mod came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    Enchant,
    Rune,
    Implicit,
    Fractured,
    Explicit,
    Desecrated,
    Crafted,
}

/// One of a listed item's mods, as the trade site describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListedMod {
    pub kind: ModKind,
    /// The mod's text, the site's link markup stripped (`strip_link_markup`).
    pub text: String,
    /// The trade stat id it counts toward (`explicit.stat_4220027924`), when the site gives one.
    pub stat_id: Option<String>,
    /// Prefix or suffix and tier (`P9`, `S6`), when the site gives them.
    pub tier: Option<String>,
    /// The item level the mod needs to roll (the highest of a hybrid mod's parts), when the site
    /// gives it.
    pub level: Option<u32>,
    /// The roll a stat filter compares (`mod_value`); `None` for a flag mod.
    pub value: Option<f64>,
    /// The range each of the text's rolled numbers rolls in, in order (`(142.0, 161.0)`), summed
    /// over the affixes of a line several feed: two hybrid prefixes' `55% увеличение уклонения`
    /// rolls 27-32 from each, 54-64 in all. Empty when the site gives none, or when the affixes'
    /// ranges don't line up.
    pub ranges: Vec<(f64, f64)>,
}

impl ListedMod {
    fn from_raw(kind: ModKind, raw: RawMod) -> ListedMod {
        match raw {
            RawMod::Text(text) => {
                let text = strip_link_markup(&text);
                ListedMod {
                    kind,
                    value: mod_value(&text),
                    text,
                    stat_id: None,
                    tier: None,
                    level: None,
                    ranges: Vec::new(),
                }
            }
            RawMod::Described {
                description,
                hash,
                mods,
            } => {
                let text = strip_link_markup(&description);
                ListedMod {
                    kind,
                    value: mod_value(&text),
                    text,
                    stat_id: hash.map(|hash| {
                        hash.strip_prefix("stat.")
                            .map_or(hash.clone(), str::to_owned)
                    }),
                    level: mods.iter().filter_map(|tier| tier.level).max(),
                    ranges: roll_ranges(&mods),
                    tier: mods.into_iter().find_map(|tier| tier.tier),
                }
            }
        }
    }

    /// `text` the way the game's advanced tooltip (Alt held) writes it, each rolled number
    /// followed by the range it rolls in -- `+38(36-40)% к сопротивлению холоду` -- and where
    /// each range sits. `text` as it is when the ranges don't pair off one to one with its
    /// numbers; a fixed number (`25% снижение`, rolled -25 to -25) gets none.
    pub fn text_with_ranges(&self) -> (String, Vec<Range<usize>>) {
        use std::fmt::Write as _;
        let numbers = number_spans(&self.text);
        if self.ranges.is_empty() || numbers.len() != self.ranges.len() {
            return (self.text.clone(), Vec::new());
        }
        let mut text = String::with_capacity(self.text.len() + 16 * numbers.len());
        let mut inserted = Vec::with_capacity(numbers.len());
        let mut copied = 0;
        for ((digits, number), &(min, max)) in numbers.into_iter().zip(&self.ranges) {
            text.push_str(&self.text[copied..digits.end]);
            copied = digits.end;
            // The site rolls some numbers the other way round from the text: `25% снижение` as
            // -25, `уменьшение зарядов флакона на 35%` from 35 down to 30.
            let (min, max) = if number >= 0.0 && min <= 0.0 && max <= 0.0 {
                (-min, -max)
            } else {
                (min, max)
            };
            let (low, high) = (min.min(max), min.max(max));
            if low == high {
                continue;
            }
            let start = text.len();
            // Writing into a `String` can't fail.
            let _ = write!(text, "({}-{})", rounded(low), rounded(high));
            inserted.push(start..text.len());
        }
        text.push_str(&self.text[copied..]);
        (text, inserted)
    }
}

/// Each number's roll range, summed over the affixes behind the line (see [`ListedMod::ranges`]).
fn roll_ranges(affixes: &[RawModTier]) -> Vec<(f64, f64)> {
    let Some(first) = affixes.first() else {
        return Vec::new();
    };
    let mut ranges = vec![(0.0, 0.0); first.magnitudes.len()];
    for affix in affixes {
        if affix.magnitudes.len() != ranges.len() {
            return Vec::new();
        }
        for (range, magnitude) in ranges.iter_mut().zip(&affix.magnitudes) {
            let (Some(min), Some(max)) = (bound(&magnitude.min), bound(&magnitude.max)) else {
                return Vec::new();
            };
            range.0 += min;
            range.1 += max;
        }
    }
    ranges
}

/// A roll range's bound: a number, or a string holding one (`"23.1"`, the live site's form).
fn bound(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::String(text) => text.parse().ok(),
        other => other.as_f64(),
    }
}

/// `value` to two decimals at most, trailing zeros dropped (`142`, `23.1`), as a roll range
/// writes it.
fn rounded(value: f64) -> f64 {
    // `+ 0.0` turns a negative zero into the zero it reads as.
    (value * 100.0).round() / 100.0 + 0.0
}

/// The roll a stat filter compares for a mod's `text`, read the way the trade site and EE2's
/// `getRollOrMinmaxAvg` read it: the mean of two or four numbers ("Adds 1 to 15" rolls 8), else
/// the first ("+93 to maximum Life", "-10% to Fire Resistance"). `None` for a mod without a
/// number, a flag.
fn mod_value(text: &str) -> Option<f64> {
    let numbers = number_spans(text);
    match numbers.len() {
        0 => None,
        2 | 4 => Some(numbers.iter().map(|(_, number)| number).sum::<f64>() / numbers.len() as f64),
        _ => Some(numbers[0].1),
    }
}

/// The numbers in `text`: where each one's digits sit, and its value, a `-` right before it
/// making it negative, `.` or `,` its decimal mark.
fn number_spans(text: &str) -> Vec<(Range<usize>, f64)> {
    let bytes = text.as_bytes();
    let mut numbers = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if !bytes[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let negative = at > 0 && bytes[at - 1] == b'-';
        let start = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        if at + 1 < bytes.len()
            && matches!(bytes[at], b'.' | b',')
            && bytes[at + 1].is_ascii_digit()
        {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
        }
        if let Ok(number) = text[start..at].replace(',', ".").parse::<f64>() {
            numbers.push((start..at, if negative { -number } else { number }));
        }
    }
    numbers
}

/// The words the site's link markup shows: `+16% to [Resistances|Cold Resistance]` -> `+16% to
/// Cold Resistance`, `[Lightning] damage` -> `Lightning damage`. Mod texts, property names and
/// values all carry it.
fn strip_link_markup(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else {
            break;
        };
        plain.push_str(&rest[..open]);
        let link = &rest[open + 1..open + close];
        plain.push_str(link.rsplit_once('|').map_or(link, |(_, shown)| shown));
        rest = &rest[open + close + 1..];
    }
    plain.push_str(rest);
    plain
}

/// What frames a listed item's tooltip and colours its name: gear's rarity, or the kind of item
/// that has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemFrame {
    #[default]
    Normal,
    Magic,
    Rare,
    Unique,
    Gem,
    Currency,
}

impl ItemFrame {
    /// `frameType` 4 and 5 are gems and currency, whatever `rarity` says; anything else goes by
    /// `rarity` (`"Rare"`), or by `frameType` 1-3 where there's none.
    fn of(frame_type: Option<u32>, rarity: Option<&str>) -> ItemFrame {
        match (frame_type, rarity) {
            (Some(4), _) => ItemFrame::Gem,
            (Some(5), _) => ItemFrame::Currency,
            (_, Some("Unique")) | (Some(3), None) => ItemFrame::Unique,
            (_, Some("Rare")) | (Some(2), None) => ItemFrame::Rare,
            (_, Some("Magic")) | (Some(1), None) => ItemFrame::Magic,
            _ => ItemFrame::Normal,
        }
    }
}

/// The colour the game draws a property's value in: `values[][1]`, GGG's value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueColor {
    /// 0, and every type this crate doesn't tell apart.
    Default,
    /// 1: raised by the item's own mods or quality.
    Augmented,
    /// 2: a requirement the character doesn't meet.
    Unmet,
    /// 3-7: damage of that kind.
    Physical,
    Fire,
    Cold,
    Lightning,
    Chaos,
}

impl ValueColor {
    fn from_type(value_type: u32) -> ValueColor {
        match value_type {
            1 => ValueColor::Augmented,
            2 => ValueColor::Unmet,
            3 => ValueColor::Physical,
            4 => ValueColor::Fire,
            5 => ValueColor::Cold,
            6 => ValueColor::Lightning,
            7 => ValueColor::Chaos,
            _ => ValueColor::Default,
        }
    }
}

/// How a property line's name and values make up its text: GGG's `displayMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyLayout {
    /// 0: `name: values` (`Уклонение: 500`), or the name alone where there are none -- the item
    /// class line. Also 2, a progress bar the game draws under such a line (a gem's experience),
    /// and any mode this crate doesn't know.
    NameFirst,
    /// 1: `values name`, a requirement's `50 Ловк`.
    ValuesFirst,
    /// 3: the name with its `{0}`, `{1}` slots filled with the values.
    Template,
}

impl PropertyLayout {
    fn from_mode(display_mode: u32) -> PropertyLayout {
        match display_mode {
            1 => PropertyLayout::ValuesFirst,
            3 => PropertyLayout::Template,
            _ => PropertyLayout::NameFirst,
        }
    }
}

/// One line of a listed item's properties, requirements or granted skills.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemProperty {
    /// The line's words, link markup stripped (`Уклонение`).
    pub name: String,
    /// Its values in order, markup stripped too, each with the colour the game draws it in.
    pub values: Vec<(String, ValueColor)>,
    pub layout: PropertyLayout,
}

impl ItemProperty {
    fn from_raw(raw: RawProperty) -> ItemProperty {
        ItemProperty {
            name: strip_link_markup(&raw.name),
            values: raw
                .values
                .iter()
                .map(|(value, value_type)| {
                    (strip_link_markup(value), ValueColor::from_type(*value_type))
                })
                .collect(),
            layout: PropertyLayout::from_mode(raw.display_mode),
        }
    }

    /// The line as the game writes it, and where each value sits in it, with its colour.
    pub fn text(&self) -> (String, Vec<(Range<usize>, ValueColor)>) {
        let mut text = String::with_capacity(self.name.len() + 16 * self.values.len());
        let mut spans = Vec::with_capacity(self.values.len());
        match self.layout {
            PropertyLayout::NameFirst => {
                text.push_str(&self.name);
                if !self.values.is_empty() {
                    text.push_str(": ");
                    self.push_values(&mut text, &mut spans);
                }
            }
            PropertyLayout::ValuesFirst => {
                self.push_values(&mut text, &mut spans);
                if !self.name.is_empty() {
                    text.push(' ');
                    text.push_str(&self.name);
                }
            }
            PropertyLayout::Template => {
                let mut rest = self.name.as_str();
                while let Some(open) = rest.find('{') {
                    let after = &rest[open + 1..];
                    let slot = after.find('}').and_then(|close| {
                        let value = self.values.get(after[..close].parse::<usize>().ok()?)?;
                        Some((close, value))
                    });
                    let Some((close, (value, color))) = slot else {
                        // A brace that opens no slot is text.
                        text.push_str(&rest[..=open]);
                        rest = after;
                        continue;
                    };
                    text.push_str(&rest[..open]);
                    let start = text.len();
                    text.push_str(value);
                    spans.push((start..text.len(), *color));
                    rest = &after[close + 1..];
                }
                text.push_str(rest);
            }
        }
        (text, spans)
    }

    /// The values, comma-separated, onto `text`, noting where each one sits.
    fn push_values(&self, text: &mut String, spans: &mut Vec<(Range<usize>, ValueColor)>) {
        for (index, (value, color)) in self.values.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            let start = text.len();
            text.push_str(value);
            spans.push((start..text.len(), *color));
        }
    }
}

/// A listed item as the game's own tooltip shows it: `result[].item`, link markup
/// (`[Evasion|Уклонение]`) reduced to the words it shows. The default is an identified normal
/// item the site says nothing more about.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListedItem {
    /// A rare's or unique's own name (`Штурмовой саван`); empty for any other item, whose type
    /// line names it.
    pub name: String,
    /// The type line: the base type (`Накидка сокольничего`) -- a magic item's wrapped in its
    /// affixes -- or a currency's or gem's name.
    pub type_line: String,
    pub frame: ItemFrame,
    /// `item.ilvl` (verified live) -- `None` for item types with no item level at all (Currency,
    /// Divination Cards); gems and currency may list 0 instead.
    pub item_level: Option<u32>,
    /// `item.stackSize`; `None` for items that don't stack. [`group_listings`] sums it over a
    /// seller's folded listings.
    pub stack_size: Option<u32>,
    pub unidentified: bool,
    /// The tier an unidentified item dropped as (the game's `Неопознано (Ранг 3)`), when the site
    /// gives it.
    pub unidentified_tier: Option<u32>,
    pub corrupted: bool,
    pub mirrored: bool,
    pub sanctified: bool,
    /// The game's Fractured Item: some of its mods are fractured, which nothing can change.
    pub fractured: bool,
    /// The item's art (`https://web.poecdn.com/gen/image/...png`).
    pub icon: Option<String>,
    /// The seller's note (`~b/o 1 exalted`): the item carries its own price. Without one, the
    /// listing's price comes from its stash tab's name -- a whole dump tab priced at once, which
    /// EE2 flags with a `?` after the price as "likely not real" (EE2 `docs/faq.md`).
    pub note: Option<String>,
    /// The lines under the name: the item class, defences, damage, a waystone's modifiers.
    pub properties: Vec<ItemProperty>,
    /// The level and attributes it takes, which the game lists on one line
    /// ([`ListedItem::requirements_text`]).
    pub requirements: Vec<ItemProperty>,
    /// Each socket -- a skill gem's support sockets likewise -- with the name of the rune, soul
    /// core or talisman in it, `None` while empty.
    pub sockets: Vec<Option<String>>,
    /// The skills the item grants: `Дарует умение: Снаряд хаоса 17 уровня`.
    pub granted_skills: Vec<ItemProperty>,
    /// The item's mods in the order the site lists them: enchants, runes, implicits, fractured,
    /// explicits, desecrated, crafted.
    pub mods: Vec<ListedMod>,
    /// A unique's flavour text, its lines joined with `\n`.
    pub flavour: Option<String>,
    /// What the item is for: `Можно использовать в Машине картоходца, чтобы войти на карту.`
    pub description: Option<String>,
}

impl ListedItem {
    fn from_fetched(item: FetchItem) -> ListedItem {
        let lines = |raw: Vec<RawProperty>| -> Vec<ItemProperty> {
            raw.into_iter().map(ItemProperty::from_raw).collect()
        };
        let mods = [
            (ModKind::Enchant, item.enchant_mods),
            (ModKind::Rune, item.rune_mods),
            (ModKind::Implicit, item.implicit_mods),
            (ModKind::Fractured, item.fractured_mods),
            (ModKind::Explicit, item.explicit_mods),
            (ModKind::Desecrated, item.desecrated_mods),
            (ModKind::Crafted, item.crafted_mods),
        ]
        .into_iter()
        .flat_map(|(kind, list)| {
            list.into_iter()
                .map(move |raw| ListedMod::from_raw(kind, raw))
        })
        .collect();
        let flavour = item
            .flavour_text
            .iter()
            .flat_map(|line| line.split(['\r', '\n']))
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        ListedItem {
            frame: ItemFrame::of(item.frame_type, item.rarity.as_deref()),
            name: item.name,
            type_line: item.type_line,
            item_level: item.ilvl,
            stack_size: item.stack_size,
            unidentified: item.identified == Some(false),
            unidentified_tier: item.unidentified_tier,
            corrupted: item.corrupted,
            mirrored: item.duplicated || item.mirrored,
            sanctified: item.sanctified,
            fractured: item.fractured,
            icon: item.icon.filter(|icon| !icon.is_empty()),
            note: item.note,
            properties: lines(item.properties),
            requirements: lines(item.requirements),
            sockets: filled_sockets(
                item.sockets.len().max(item.gem_sockets.len()),
                item.socketed_items,
            ),
            granted_skills: lines(item.granted_skills),
            mods,
            flavour: (!flavour.is_empty()).then_some(flavour),
            description: item
                .descr_text
                .map(|text| strip_link_markup(&text))
                .filter(|text| !text.is_empty()),
        }
    }

    /// The requirements the way the game's one requirements line lists them after its label --
    /// `Уровень 75, 50 Ловк, 50 Инт`, the level's name first but without its colon -- and where
    /// each value sits in it, with its colour.
    pub fn requirements_text(&self) -> (String, Vec<(Range<usize>, ValueColor)>) {
        let mut text = String::new();
        let mut spans = Vec::with_capacity(self.requirements.len());
        for (index, requirement) in self.requirements.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            let gap = !requirement.name.is_empty() && !requirement.values.is_empty();
            if requirement.layout == PropertyLayout::ValuesFirst {
                requirement.push_values(&mut text, &mut spans);
                text.push_str(if gap { " " } else { "" });
                text.push_str(&requirement.name);
            } else {
                text.push_str(&requirement.name);
                text.push_str(if gap { " " } else { "" });
                requirement.push_values(&mut text, &mut spans);
            }
        }
        (text, spans)
    }
}

/// `count` sockets, each filled with the item `socketedItems` puts at its index; one whose index
/// is missing or already taken gets a socket of its own after them.
fn filled_sockets(count: usize, socketed: Vec<RawSocketed>) -> Vec<Option<String>> {
    let mut sockets = vec![None; count];
    for item in socketed {
        let name = strip_link_markup(&item.type_line);
        match item.socket.and_then(|at| sockets.get_mut(at)) {
            Some(slot) if slot.is_none() => *slot = Some(name),
            _ => sockets.push(Some(name)),
        }
    }
    sockets
}

/// One resolved listing from `fetch`: the listed item and how it's sold. Presentation formatting
/// (how to display the price, etc.) is left to the caller -- this is raw parsed API data, not a
/// UI-ready row.
#[derive(Debug)]
pub struct FetchedItem {
    pub item: ListedItem,
    /// `(amount, currency)`, e.g. `(1.0, "transmute")`; `None` if the listing has no set price.
    pub price: Option<(f64, String)>,
    pub account_name: String,
    /// `listing.indexed`, an ISO-8601 timestamp string (verified live this session, e.g.
    /// `"2026-09-20T04:31:02Z"`) -- kept as raw text; relative "N days ago"-style formatting is a
    /// UI-rendering concern, not this crate's.
    pub indexed: String,
    /// Whether the seller is around to answer a whisper. Moot for an
    /// [`instant_buyout`](Self::instant_buyout) listing, which sells without its seller -- EE2
    /// draws no status dot for one.
    pub account_status: AccountStatus,
    /// `listing.fee` is present (EE2's `isInstantBuyout` test): the item sits in a Merchant tab and
    /// sells outright for its price plus that gold fee, paid by the buyer, with no whisper and no
    /// online seller needed -- the trade site's "Instant Buyout" (status option `securable` in
    /// `GET /api/trade2/data/filters`, verified 2026-09-22).
    pub instant_buyout: bool,
    /// `listing.in_demand`, which EE2 renders as an "in demand" badge.
    pub in_demand: bool,
    /// The result's top-level `gone`, which EE2 renders as a red "Gone" badge.
    pub gone: bool,
    /// `listing.whisper`: the complete message to the seller, in the seller's language (live
    /// 2026-09-23: `@Virsavia Здравствуйте, хочу купить у вас Сердце Мина Кольцо с аметистом за 1
    /// exalted в лиге Standard (секция "~b/o 1 exalted"; позиция: 22 столбец, 21 ряд)` for a
    /// `ru_RU` seller). `None` for an instant-buyout listing, which needs no whisper.
    pub whisper: Option<String>,
}

/// `GET /api/trade2/fetch/{ids}?query={query_id}` for up to 10 listing ids at a time (the trade
/// API's own per-request limit, unenforced here -- pass a pre-sliced `listing_ids` page). Entries
/// the API returns as `null` (delisted between search and fetch) are silently dropped.
///
/// Like every rate-limited call here, `fetch` records its response on `limiter` but never waits
/// on it: this crate is UI/runtime-agnostic (see the module doc comment) and has no executor to
/// sleep on. Holding off is the caller's job -- check [`RateLimiter::required_wait`] on the same
/// `limiter` before calling again.
pub async fn fetch(
    client: &Arc<dyn HttpClient>,
    site: TradeSite,
    listing_ids: &[String],
    query_id: &str,
    limiter: &mut RateLimiter,
) -> Result<Vec<FetchedItem>> {
    if listing_ids.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!(
        "{}/fetch/{}?query={query_id}",
        site.api_base(),
        listing_ids.join(",")
    );
    let request = client.get(&url, AsyncBody::default(), true);
    let body = checked_body(request, "GET", &url, Some(limiter), "fetch").await?;
    parse_fetch_response(&body)
}

/// `fetch`'s response body -> its listings, `null` entries dropped. Split out of `fetch` so the
/// field mapping is testable without an `HttpClient`.
fn parse_fetch_response(body: &str) -> Result<Vec<FetchedItem>> {
    let parsed: FetchResponse =
        serde_json::from_str(body).context("parsing fetch response JSON")?;

    Ok(parsed
        .result
        .into_iter()
        .flatten()
        .map(|entry| FetchedItem {
            item: ListedItem::from_fetched(entry.item),
            price: entry.listing.price.map(|p| (p.amount, p.currency)),
            account_name: entry.listing.account.name,
            indexed: entry.listing.indexed,
            account_status: match entry.listing.account.online {
                None => AccountStatus::Offline,
                Some(FetchOnline {
                    status: Some(FetchOnlineStatus::Afk),
                }) => AccountStatus::Afk,
                Some(_) => AccountStatus::Online,
            },
            instant_buyout: entry.listing.fee.is_some(),
            in_demand: entry.listing.in_demand.unwrap_or(false),
            gone: entry.gone.unwrap_or(false),
            whisper: entry.listing.whisper.filter(|whisper| !whisper.is_empty()),
        })
        .collect())
}

/// One results-table row after [`group_listings`] folds a seller's repeats into it.
#[derive(Debug)]
pub struct GroupedListing {
    /// The row's first listing in search order, i.e. its cheapest. For a stackable item its
    /// `item.stack_size` is the stock of every listing folded into the row.
    pub listing: FetchedItem,
    /// How many listings of an unstackable item the row stands for. EE2 shows `× N` after the
    /// price once this passes 2, in place of the unnoted-price `?`.
    pub listed_times: u32,
}

/// Folds each seller's repeated listings into one row the way EE2's `groupedResults`
/// (`renderer/src/web/price-check/trade/trade-api.ts`) does, so one seller listing the same item
/// ten times can't fill the whole cheapest page. Each of `listings`, in search order, joins the
/// first row from the same account that asks the identical price or is one of the last two rows;
/// otherwise it opens a new row. A joining listing adds its stack to a stackable row and bumps
/// `listed_times` of any other.
///
/// Appends to `groups` because each listing looks only at the rows built before it: grouping
/// page by page into the same `groups` gives the rows one pass over every page would -- the
/// whole list EE2 regroups after each fetched page.
pub fn group_listings(
    groups: &mut Vec<GroupedListing>,
    listings: impl IntoIterator<Item = FetchedItem>,
) {
    for listing in listings {
        let rows = groups.len();
        let joined = groups.iter().enumerate().position(|(index, group)| {
            group.listing.account_name == listing.account_name
                && (group.listing.price == listing.price || rows - index <= 2)
        });
        let Some(index) = joined else {
            groups.push(GroupedListing {
                listing,
                listed_times: 1,
            });
            continue;
        };
        let group = &mut groups[index];
        match &mut group.listing.item.stack_size {
            Some(stock) => *stock += listing.item.stack_size.unwrap_or(0),
            None => group.listed_times += 1,
        }
    }
}

/// A league id as one URL path segment or query value, encoded the way the trade site's own
/// frontend encodes it (JavaScript's `encodeURIComponent`): letters, digits and `-_.!~*'()` stay as
/// they are, every other byte of its UTF-8 becomes `%XX`. League ids hold spaces ("Forbidden
/// Rites") and a private league's parentheses ("My League (PL12345)"), and a name the player typed
/// may hold anything; the trade API and poe2scout take this form.
pub fn encode_league(league: &str) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(league.len());
    for byte in league.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            // Writing into a `String` can't fail.
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(test)]
mod fetch_tests {
    use super::*;

    /// A fetch page shaped after EE2's `FetchResult` typing around the live-verified basics: an
    /// online seller's individually noted listing, an AFK seller's tab-priced one, an offline
    /// seller's instant-buyout Merchant-tab listing (`"online": null`), a delisted `null`, and a
    /// flagged stack whose seller has no `online` key at all.
    const PAGE: &str = r#"{"result": [
        {"id": "a1",
         "item": {"name": "Mind Core", "typeLine": "Alloy Crossbow", "baseType": "Alloy Crossbow",
                  "rarity": "Rare", "ilvl": 79, "identified": true, "note": "~price 1 transmute"},
         "listing": {"method": "psapi", "indexed": "2026-09-20T04:31:02Z",
                     "account": {"name": "nivon#0926", "online": {"league": "Forbidden Rites"},
                                 "lastCharacterName": "Nivon", "language": "en_US"},
                     "price": {"type": "~price", "amount": 1, "currency": "transmute"},
                     "whisper": "@Nivon Hi, I would like to buy your Mind Core Alloy Crossbow listed for 1 transmute in Forbidden Rites (stash tab \"~price 1 transmute\"; position: left 3, top 3)"}},
        {"id": "b2",
         "item": {"name": "Storm Hold", "typeLine": "Alloy Crossbow", "baseType": "Alloy Crossbow",
                  "rarity": "Rare", "ilvl": 81, "identified": true},
         "listing": {"method": "psapi", "indexed": "2026-09-21T10:00:00Z",
                     "account": {"name": "dumper#1111", "lastCharacterName": "Dumper",
                                 "online": {"league": "Forbidden Rites", "status": "afk"}},
                     "price": {"type": "~price", "amount": 5, "currency": "exalted"}}},
        {"id": "c3",
         "item": {"name": "Grim Bane", "typeLine": "Alloy Crossbow", "baseType": "Alloy Crossbow",
                  "rarity": "Rare", "ilvl": 82, "identified": true, "note": "~price 2 divine"},
         "listing": {"method": "psapi", "indexed": "2026-09-22T08:15:00Z", "fee": 250,
                     "account": {"name": "merchant#2222", "online": null,
                                 "lastCharacterName": "Merchant"},
                     "price": {"type": "~price", "amount": 2, "currency": "divine"}}},
        null,
        {"id": "d4", "gone": true,
         "item": {"name": "", "typeLine": "Breach Splinter", "baseType": "Breach Splinter",
                  "rarity": "Normal", "identified": true, "stackSize": 40},
         "listing": {"method": "psapi", "indexed": "2026-09-22T09:00:00Z", "in_demand": true,
                     "account": {"name": "splinters#3333", "lastCharacterName": "Splint"},
                     "price": {"type": "~price", "amount": 3, "currency": "exalted"}}}
    ]}"#;

    #[test]
    fn listing_flags_follow_ee2s_reading_of_the_fetch_json() {
        let items = parse_fetch_response(PAGE).expect("the page parses");
        let flags: Vec<_> = items
            .iter()
            .map(|item| {
                (
                    item.account_status,
                    item.instant_buyout,
                    item.item.note.is_some(),
                )
            })
            .collect();
        assert_eq!(
            flags,
            [
                (AccountStatus::Online, false, true),
                (AccountStatus::Afk, false, false),
                (AccountStatus::Offline, true, true),
                (AccountStatus::Offline, false, false),
            ]
        );
        let (crossbow, splinters) = (&items[0], &items[3]);
        assert_eq!(
            (crossbow.item.stack_size, crossbow.in_demand, crossbow.gone),
            (None, false, false)
        );
        assert_eq!(
            (
                splinters.item.stack_size,
                splinters.in_demand,
                splinters.gone
            ),
            (Some(40), true, true)
        );
        // A seller in person gets a whisper; an instant-buyout listing sells without one.
        assert!(
            crossbow
                .whisper
                .as_deref()
                .is_some_and(|whisper| whisper.starts_with("@Nivon "))
        );
        assert_eq!(items[2].whisper, None);
    }

    #[test]
    fn a_listed_items_mods_come_with_their_stat_ids_tiers_and_levels() {
        // PoE2's described form as the live fetch sends it, and the plain text form beside it;
        // a hybrid mod's parts need different levels.
        let page = r#"{"result": [{
            "id": "x",
            "item": {
                "name": "", "typeLine": "Amethyst Ring", "ilvl": 80,
                "implicitMods": [{"description": "+10% to [Resistances|Chaos Resistance]",
                    "domain": "implicit", "hash": "stat.implicit.stat_2923486259",
                    "mods": [{"level": 25}]}],
                "explicitMods": [
                    {"description": "Adds 1 to 15 [Lightning] damage to [Attack|Attacks]",
                        "domain": "explicit", "hash": "stat.explicit.stat_1754445556",
                        "mods": [{"name": "Buzzing", "tier": "P8", "level": 8},
                                 {"name": "Buzzing", "tier": "P8", "level": 16}]},
                    "+12 to maximum Life"
                ]
            },
            "listing": {"price": {"amount": 1, "currency": "exalted"}, "account": {"name": "a"},
                "indexed": "2026-09-23T00:00:00Z"}
        }]}"#;
        let items = parse_fetch_response(page).expect("the page parses");
        assert_eq!(
            items[0].item.mods,
            [
                ListedMod {
                    kind: ModKind::Implicit,
                    text: "+10% to Chaos Resistance".to_owned(),
                    stat_id: Some("implicit.stat_2923486259".to_owned()),
                    tier: None,
                    level: Some(25),
                    value: Some(10.0),
                    ranges: Vec::new(),
                },
                ListedMod {
                    kind: ModKind::Explicit,
                    text: "Adds 1 to 15 Lightning damage to Attacks".to_owned(),
                    stat_id: Some("explicit.stat_1754445556".to_owned()),
                    tier: Some("P8".to_owned()),
                    level: Some(16),
                    value: Some(8.0),
                    ranges: Vec::new(),
                },
                ListedMod {
                    kind: ModKind::Explicit,
                    text: "+12 to maximum Life".to_owned(),
                    stat_id: None,
                    tier: None,
                    level: None,
                    value: Some(12.0),
                    ranges: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn a_mods_roll_is_read_the_way_a_stat_filter_compares_it() {
        // Live Russian listings' texts (2026-09-23) and EN ones.
        assert_eq!(mod_value("Регенерация 11.1 здоровья в секунду"), Some(11.1));
        assert_eq!(mod_value("-10% to Fire Resistance"), Some(-10.0));
        assert_eq!(
            mod_value("Adds 12 to 20 Physical Damage to Attacks"),
            Some(16.0)
        );
        assert_eq!(
            mod_value("Для окружения требуется на 3 врагов меньше"),
            Some(3.0)
        );
        // A flag has no roll to compare.
        assert_eq!(mod_value("Enemies in your Presence are Blinded"), None);
    }

    #[test]
    fn link_markup_reduces_to_the_words_it_shows() {
        assert_eq!(strip_link_markup("[Evasion|Уклонение]"), "Уклонение");
        assert_eq!(
            strip_link_markup(
                "108% усиление [AilmentApplication|наложения] [ElementalAilments|стихийных] \
                 состояний у монстров"
            ),
            "108% усиление наложения стихийных состояний у монстров"
        );
        assert_eq!(strip_link_markup("[Lightning] damage"), "Lightning damage");
        // Text without markup, and a bracket that closes nothing, stay as they are.
        assert_eq!(
            strip_link_markup("+36 к максимуму здоровья"),
            "+36 к максимуму здоровья"
        );
        assert_eq!(strip_link_markup("Ранг [3"), "Ранг [3");
    }

    /// Live `ru.pathofexile.com` fetch pages (2026-09-23, sellers anonymised): three rare body
    /// armours and three rare tier-15 waystones.
    const RU_CHEST: &str = include_str!("../tests/fixtures/fetch-ru-chest.json");
    const RU_WAYSTONE: &str = include_str!("../tests/fixtures/fetch-ru-waystone.json");

    /// A line's text, and each value's own text and colour.
    fn with_values(
        (text, spans): &(String, Vec<(Range<usize>, ValueColor)>),
    ) -> (&str, Vec<(&str, ValueColor)>) {
        let values = spans
            .iter()
            .map(|(span, color)| (&text[span.clone()], *color))
            .collect();
        (text, values)
    }

    #[test]
    fn a_live_body_armour_reads_like_its_game_tooltip() {
        let items = parse_fetch_response(RU_CHEST).expect("the live page parses");
        assert_eq!(items.len(), 3);
        let armour = &items[0].item;
        assert_eq!(
            (
                armour.name.as_str(),
                armour.type_line.as_str(),
                armour.frame,
                armour.item_level
            ),
            (
                "Штурмовой саван",
                "Накидка сокольничего",
                ItemFrame::Rare,
                Some(75)
            )
        );
        assert_eq!(armour.note.as_deref(), Some("~b/o 1 exalted"));
        assert!(
            armour
                .icon
                .as_deref()
                .is_some_and(|icon| icon.starts_with("https://web.poecdn.com/gen/image/"))
        );
        assert!(!armour.unidentified && !armour.corrupted && !armour.mirrored);

        // The item class line has no values; `[Evasion|Уклонение]` shows its words, and the
        // evasion the item's own mods raise comes augmented.
        let class = armour.properties[0].text();
        assert_eq!(with_values(&class), ("Нательный доспех", vec![]));
        let evasion = armour.properties[1].text();
        assert_eq!(
            with_values(&evasion),
            ("Уклонение: 500", vec![("500", ValueColor::Augmented)])
        );
        // The requirements' one line, the attributes lowered by the item's own mod.
        let requirements = armour.requirements_text();
        assert_eq!(
            with_values(&requirements),
            (
                "Уровень 75, 50 Ловк, 50 Инт",
                vec![
                    ("75", ValueColor::Default),
                    ("50", ValueColor::Augmented),
                    ("50", ValueColor::Augmented)
                ]
            )
        );

        // The implicit first, then the explicits with their tiers, levels and roll ranges; a
        // roll that can't vary (the implicit's 5-5, the attributes' -25 to -25) gets no range.
        assert_eq!(armour.mods[0].kind, ModKind::Implicit);
        let evasion_mod = &armour.mods[1];
        assert_eq!(
            (
                evasion_mod.kind,
                evasion_mod.tier.as_deref(),
                evasion_mod.level
            ),
            (ModKind::Explicit, Some("P1"), Some(75))
        );
        let written: Vec<String> = armour
            .mods
            .iter()
            .map(|listed| listed.text_with_ranges().0)
            .collect();
        assert_eq!(
            written,
            [
                "5% повышение скорости передвижения",
                "+125(142-161) к уклонению",
                "+45(43-48) к максимуму энергетического щита",
                "34(33-38)% увеличение уклонения и энергетического щита",
                "+36(33-41) к максимуму здоровья",
                "25% снижение требований к характеристикам",
                "+38(36-40)% к сопротивлению холоду",
                "+32(31-35)% к сопротивлению молнии",
            ]
        );
        let (_, ranges) = armour.mods[1].text_with_ranges();
        assert_eq!(&written[1][ranges[0].clone()], "(142-161)");
    }

    #[test]
    fn a_line_two_affixes_feed_and_a_two_number_roll_get_their_ranges() {
        let items = parse_fetch_response(RU_CHEST).expect("the live page parses");
        let armour = &items[2].item;
        let written: Vec<String> = armour
            .mods
            .iter()
            .map(|listed| listed.text_with_ranges().0)
            .collect();
        assert_eq!(
            written,
            [
                "+53(39-53) к уклонению",
                // Two P3 prefixes' 27-32 each.
                "55(54-64)% увеличение уклонения",
                "+28(26-32) к максимуму здоровья",
                "+14(11-15)% к сопротивлению холоду",
                "+32(31-35)% к сопротивлению молнии",
                "Регенерация 23.3(23.1-29) здоровья в секунду",
                "От 105(101-151) до 173(152-220) физического урона шипами",
            ]
        );
        assert_eq!(armour.mods[1].tier.as_deref(), Some("P3"));
    }

    #[test]
    fn a_live_waystone_keeps_its_map_properties_and_description() {
        let items = parse_fetch_response(RU_WAYSTONE).expect("the live page parses");
        let waystone = &items[0].item;
        assert_eq!(
            (
                waystone.name.as_str(),
                waystone.type_line.as_str(),
                waystone.item_level
            ),
            ("Тайная решимость", "Путевой камень (Ур. 15)", Some(82))
        );
        let lines: Vec<_> = waystone.properties.iter().map(ItemProperty::text).collect();
        assert_eq!(
            with_values(&lines[1]),
            (
                "Размер групп монстров: +29%",
                vec![("+29%", ValueColor::Augmented)]
            )
        );
        assert_eq!(
            lines
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>(),
            [
                "Доступно возрождений: 1",
                "Размер групп монстров: +29%",
                "Эффективность монстров: +13%",
                "Шанс выпадения путевого камня: +65%",
            ]
        );
        assert!(waystone.requirements.is_empty());
        assert_eq!(
            waystone.description.as_deref(),
            Some(
                "Можно использовать в Машине картоходца, чтобы войти на карту. Путевые камни одноразовые."
            )
        );
        assert_eq!(waystone.note.as_deref(), Some("~b/o 1 exalted"));
        // A flag has no number to range; a roll the site ranges from 35 down to 30 reads 30-35.
        assert_eq!(
            waystone.mods[0].text_with_ranges().0,
            "Область содержит участки заряженной земли"
        );
        assert_eq!(
            waystone.mods[4].text_with_ranges().0,
            "Игроки получают уменьшение зарядов флакона на 35(30-35)%"
        );
    }

    #[test]
    fn sockets_skills_flags_and_templated_lines_read_like_the_tooltip() {
        let page = r#"{"result": [{
            "id": "x",
            "item": {
                "name": "", "typeLine": "Сияющий скипетр", "rarity": "Unique", "frameType": 3,
                "ilvl": 80, "identified": false, "unidentifiedTier": 3, "corrupted": true,
                "duplicated": true, "sanctified": true, "fractured": true,
                "sockets": [{"group": 0, "type": "rune"}, {"group": 0, "type": "rune"}],
                "socketedItems": [{"typeLine": "Большая [Rune|руна] железа", "socket": 1}],
                "grantedSkills": [{"name": "Дарует умение",
                    "values": [["[ChaosBolt|Снаряд хаоса] 17 уровня", 0]], "displayMode": 0}],
                "properties": [{"name": "Хранит {0} из {1} зарядов",
                    "values": [["3", 1], ["5", 0]], "displayMode": 3}],
                "craftedMods": ["+10 к силе"],
                "flavourText": ["Смертные проводят жизнь, гадая,\r", "какая именно судьба их ждёт."]
            },
            "listing": {"price": {"amount": 1, "currency": "exalted"}, "account": {"name": "a"},
                "indexed": "2026-09-23T00:00:00Z"}
        }]}"#;
        let items = parse_fetch_response(page).expect("the page parses");
        let sceptre = &items[0].item;
        assert_eq!(sceptre.frame, ItemFrame::Unique);
        assert_eq!(
            (
                sceptre.unidentified,
                sceptre.unidentified_tier,
                sceptre.corrupted,
                sceptre.mirrored,
                sceptre.sanctified,
                sceptre.fractured
            ),
            (true, Some(3), true, true, true, true)
        );
        assert_eq!(
            sceptre.sockets,
            [None, Some("Большая руна железа".to_owned())]
        );
        assert_eq!(
            sceptre.granted_skills[0].text().0,
            "Дарует умение: Снаряд хаоса 17 уровня"
        );
        let charges = sceptre.properties[0].text();
        assert_eq!(
            with_values(&charges),
            (
                "Хранит 3 из 5 зарядов",
                vec![("3", ValueColor::Augmented), ("5", ValueColor::Default)]
            )
        );
        assert_eq!(
            (sceptre.mods[0].kind, sceptre.mods[0].text.as_str()),
            (ModKind::Crafted, "+10 к силе")
        );
        assert_eq!(
            sceptre.flavour.as_deref(),
            Some("Смертные проводят жизнь, гадая,\nкакая именно судьба их ждёт.")
        );
    }

    #[test]
    fn a_card_field_in_an_unexpected_shape_costs_only_itself() {
        let page = r#"{"result": [{
            "id": "x",
            "item": {
                "name": "", "typeLine": "Сфера хаоса", "frameType": 5, "stackSize": 3,
                "properties": "misshapen", "sockets": {"group": 0}, "grantedSkills": null,
                "explicitMods": [{"description": "Изменяет свойство",
                    "mods": [{"tier": "S1", "magnitudes": "misshapen"}]}]
            },
            "listing": {"price": {"amount": 2, "currency": "exalted"}, "account": {"name": "a"},
                "indexed": "2026-09-23T00:00:00Z"}
        }]}"#;
        let items = parse_fetch_response(page).expect("the page parses");
        let orb = &items[0];
        assert_eq!(orb.price, Some((2.0, "exalted".to_owned())));
        assert_eq!(orb.item.frame, ItemFrame::Currency);
        assert_eq!(orb.item.stack_size, Some(3));
        assert!(orb.item.properties.is_empty() && orb.item.sockets.is_empty());
        assert_eq!(orb.item.mods[0].tier.as_deref(), Some("S1"));
        assert!(orb.item.mods[0].ranges.is_empty());
    }

    /// A priced, unflagged listing: only what grouping reads varies.
    fn listing(account: &str, amount: f64, stack_size: Option<u32>) -> FetchedItem {
        FetchedItem {
            item: ListedItem {
                type_line: "Alloy Crossbow".to_owned(),
                stack_size,
                ..ListedItem::default()
            },
            price: Some((amount, "exalted".to_owned())),
            account_name: account.to_owned(),
            indexed: String::new(),
            account_status: AccountStatus::Online,
            instant_buyout: false,
            in_demand: false,
            gone: false,
            whisper: None,
        }
    }

    /// `(account, price amount, listed_times)` per row.
    fn rows(groups: &[GroupedListing]) -> Vec<(&str, f64, u32)> {
        groups
            .iter()
            .map(|row| {
                let amount = row.listing.price.as_ref().map_or(0.0, |price| price.0);
                (row.listing.account_name.as_str(), amount, row.listed_times)
            })
            .collect()
    }

    #[test]
    fn a_seller_joins_their_row_at_the_same_price_or_among_the_last_two_rows() {
        let listings = || {
            [
                listing("f", 1.0, None),
                listing("a", 1.0, None),
                listing("b", 2.0, None),
                listing("c", 2.0, None),
                // The same price joins however far back the row is.
                listing("f", 1.0, None),
                // Another price joins only the last row ("c") or the one before it ("b")...
                listing("c", 3.0, None),
                listing("b", 3.0, None),
                // ...so "a", further back, opens a new row.
                listing("a", 3.0, None),
            ]
        };
        let expected = [
            ("f", 1.0, 2),
            ("a", 1.0, 1),
            ("b", 2.0, 2),
            ("c", 2.0, 2),
            ("a", 3.0, 1),
        ];

        let mut one_pass = Vec::new();
        group_listings(&mut one_pass, listings());
        assert_eq!(rows(&one_pass), expected);

        // Split mid-list, the second page still joins rows the first one built.
        let mut paged = Vec::new();
        let mut pages = listings().into_iter();
        group_listings(&mut paged, pages.by_ref().take(5));
        group_listings(&mut paged, pages);
        assert_eq!(rows(&paged), expected);
    }

    #[test]
    fn a_stackable_row_sums_its_stock_instead_of_counting_listings() {
        let mut groups = Vec::new();
        group_listings(
            &mut groups,
            [listing("s", 3.0, Some(40)), listing("s", 3.0, Some(15))],
        );
        assert_eq!(rows(&groups), [("s", 3.0, 1)]);
        assert_eq!(groups[0].listing.item.stack_size, Some(55));
    }
}

#[cfg(test)]
mod route_search_tests {
    use poe2_domain::ItemCategory;

    use super::*;

    fn category(id: &str) -> Option<ItemCategory> {
        Some(ItemCategory {
            id: id.to_string(),
            display_name: id.to_string(),
        })
    }

    fn catalog() -> Vec<StaticCurrency> {
        [
            ("divine", "Divine Orb"),
            ("omen-of-light", "Предзнаменование света"),
            ("uncut-skill-gem-19", "Uncut Skill Gem (Level 19)"),
            ("waystone-13", "Путевой камень (Ур. 13)"),
        ]
        .into_iter()
        .map(|(id, name)| StaticCurrency {
            id: id.to_string(),
            display_name: name.to_string(),
            icon_url: None,
        })
        .collect()
    }

    #[test]
    fn exchange_catalog_items_route_to_the_market_by_their_static_id() {
        for (name, category_id, trade_id) in [
            ("Divine Orb", "currency", "divine"),
            ("Предзнаменование света", "currency", "omen-of-light"),
            (
                "Uncut Skill Gem (Level 19)",
                "currency",
                "uncut-skill-gem-19",
            ),
        ] {
            let item = ParsedItem {
                name: name.to_string(),
                category: category(category_id),
                ..Default::default()
            };
            match route_search(&item, &catalog(), &[]) {
                SearchRoute::Market { trade_id: id } => assert_eq!(id, trade_id),
                other => panic!("{name}: expected Market, got {}", route_debug(&other)),
            }
        }
    }

    #[test]
    fn currency_missing_from_the_exchange_catalog_is_searched_by_type() {
        // Live 2026-09-22: an Inscribed Ultimatum is `Rarity: Currency` but no exchange entry --
        // it used to end in "the item is not in the exchange catalog" instead of a search.
        let item = ParsedItem {
            name: "Начертанный Ультиматум".to_string(),
            category: category("currency"),
            ..Default::default()
        };
        match route_search(&item, &catalog(), &[]) {
            SearchRoute::Exact { exact_type } => assert_eq!(exact_type, "Начертанный Ультиматум"),
            other => panic!("expected Exact, got {}", route_debug(&other)),
        }
    }

    #[test]
    fn white_waystones_route_to_the_market_and_rare_ones_are_searched() {
        let white = ParsedItem {
            name: "Путевой камень (Ур. 13)".to_string(),
            category: category("map.waystone"),
            rarity: Some(ItemRarity::Normal),
            ..Default::default()
        };
        match route_search(&white, &catalog(), &[]) {
            SearchRoute::Market { trade_id } => assert_eq!(trade_id, "waystone-13"),
            other => panic!("expected Market, got {}", route_debug(&other)),
        }
        let rare_waystone = ParsedItem {
            rarity: Some(ItemRarity::Rare),
            ..white
        };
        let rare = ParsedItem {
            name: "Divine Orb".to_string(),
            category: category("accessory.ring"),
            rarity: Some(ItemRarity::Rare),
            ..Default::default()
        };
        for item in [rare_waystone, rare] {
            assert!(matches!(
                route_search(&item, &catalog(), &[]),
                SearchRoute::Filtered { .. }
            ));
        }
    }

    #[test]
    fn gems_are_searched_by_their_type() {
        let item = ParsedItem {
            name: "Mirage Archer".to_string(),
            category: category("gem"),
            ..Default::default()
        };
        assert_eq!(
            filtered_scope_of(&item, &[]),
            SearchScope {
                base_type: Some("Mirage Archer".to_owned()),
                misc: MiscChoices {
                    corrupted: Some(false),
                    ..MiscChoices::default()
                },
                ..SearchScope::default()
            }
        );
    }

    #[test]
    fn fragments_off_the_exchange_are_searched_by_type_and_logbooks_by_type_with_filters() {
        let fragment = ParsedItem {
            name: "Ancient Crisis Fragment".to_string(),
            category: category("map.fragment"),
            rarity: Some(ItemRarity::Normal),
            ..Default::default()
        };
        match route_search(&fragment, &catalog(), &[]) {
            SearchRoute::Exact { exact_type } => assert_eq!(exact_type, "Ancient Crisis Fragment"),
            other => panic!("expected Exact, got {}", route_debug(&other)),
        }
        let logbook = ParsedItem {
            name: "Expedition Logbook".to_string(),
            category: category("map.logbook"),
            rarity: Some(ItemRarity::Normal),
            area_level: Some(79),
            ..Default::default()
        };
        let listed = [StaticCurrency {
            id: "expedition-logbook".to_string(),
            display_name: "Expedition Logbook".to_string(),
            icon_url: None,
        }];
        match route_search(&logbook, &listed, &[]) {
            SearchRoute::Filtered { scope } => {
                assert_eq!(scope.base_type.as_deref(), Some("Expedition Logbook"))
            }
            other => panic!("expected Filtered, got {}", route_debug(&other)),
        }
    }

    #[test]
    fn russian_magic_names_find_their_base_type() {
        // Magic names from the live RU client (2026-09-22); the base types are EE2's
        // `ru/items.ndjson` names (the client's), beside bases sharing a word with them.
        let item_types: Vec<ItemTypeEntry> = [
            "Увядший жезл",
            "Костяной жезл",
            "Поножи ваал",
            "Железные поножи",
            "Путевой камень (Ур. 1)",
            "Путевой камень (Ур. 13)",
        ]
        .into_iter()
        .map(|type_name| ItemTypeEntry {
            group_label: String::new(),
            type_name: type_name.to_owned(),
        })
        .collect();
        for (name, base) in [
            ("Глифический Увядший жезл катастрофы", "Увядший жезл"),
            ("Здоровые Поножи ваал тролля", "Поножи ваал"),
            (
                "Адский Путевой камень (Ур. 13) уклонения",
                "Путевой камень (Ур. 13)",
            ),
        ] {
            assert_eq!(magic_base_type(name, &item_types), Some(base), "{name}");
        }
    }

    #[test]
    fn divination_card_category_routes_to_exact() {
        let item = ParsedItem {
            name: "The Doctor".to_string(),
            category: category("card"),
            ..Default::default()
        };
        assert!(matches!(
            route_search(&item, &[], &[]),
            SearchRoute::Exact { .. }
        ));
    }

    #[test]
    fn an_unidentified_unique_is_priced_among_unidentified_uniques_of_its_base() {
        // The game shows only the base; alone it would list the base's normal, magic and rare
        // copies and every identified unique on it.
        let item = ParsedItem {
            name: "Crystal Focus".to_string(),
            rarity: Some(ItemRarity::Unique),
            is_unidentified: true,
            ..Default::default()
        };
        let scope = filtered_scope_of(&item, &[]);
        assert_eq!(scope.name, None);
        assert_eq!(scope.base_type.as_deref(), Some("Crystal Focus"));
        assert_eq!(scope.rarity, Some(RarityFilter::Unique));
        assert_eq!(scope.misc.identified, Some(false));
        assert_eq!(switched_scope(&scope, &item, &[]), None);

        // A tier from 5 up is matched too.
        let tiered = ParsedItem {
            unidentified_tier: Some(5),
            ..item
        };
        let body_scope = filtered_scope_of(&tiered, &[]);
        let body = filtered_search_body(&body_scope, &[], ListingStatus::Online);
        let misc = &serde_json::to_value(body).expect("the body serializes")["query"]["filters"]["misc_filters"]
            ["filters"];
        assert_eq!(misc["identified"], serde_json::json!({ "option": "false" }));
        assert_eq!(misc["unidentified_tier"], serde_json::json!({ "min": 5.0 }));
    }

    fn filtered_scope_of(item: &ParsedItem, item_types: &[ItemTypeEntry]) -> SearchScope {
        match route_search(item, &[], item_types) {
            SearchRoute::Filtered { scope } => scope,
            other => panic!(
                "expected Filtered, got a different route: {}",
                route_debug(&other)
            ),
        }
    }

    fn item_types(names: &[&str]) -> Vec<ItemTypeEntry> {
        names
            .iter()
            .map(|&name| ItemTypeEntry {
                group_label: "Flasks".to_owned(),
                type_name: name.to_owned(),
            })
            .collect()
    }

    #[test]
    fn identified_unique_is_searched_by_name_and_base_type() {
        let item = ParsedItem {
            name: "The Eternal Spark".to_string(),
            base_type: Some("Crystal Focus".to_string()),
            category: category("armour.focus"),
            rarity: Some(ItemRarity::Unique),
            ..Default::default()
        };
        assert_eq!(
            filtered_scope_of(&item, &[]),
            SearchScope {
                name: Some("The Eternal Spark".to_owned()),
                base_type: Some("Crystal Focus".to_owned()),
                category: None,
                rarity: None,
                stat_match: StatMatch::All,
                // In its own state, like any item: uncorrupted, unmirrored, unsanctified.
                misc: clean_gear(),
                price: PriceCurrency::Any,
            }
        );
    }

    #[test]
    fn rare_equipment_is_searched_by_category_among_rares() {
        let item = ParsedItem {
            name: "Dragon Core".to_string(),
            base_type: Some("Bombard Crossbow".to_string()),
            category: category("weapon.crossbow"),
            rarity: Some(ItemRarity::Rare),
            ..Default::default()
        };
        assert_eq!(
            filtered_scope_of(&item, &[]),
            SearchScope {
                category: Some("weapon.crossbow".to_owned()),
                rarity: Some(RarityFilter::Rare),
                misc: clean_gear(),
                ..SearchScope::default()
            }
        );
    }

    /// What an uncorrupted, unmirrored, unsanctified gear item leaves out of its search.
    fn clean_gear() -> MiscChoices {
        MiscChoices {
            corrupted: Some(false),
            mirrored: Some(false),
            sanctified: Some(false),
            ..MiscChoices::default()
        }
    }

    #[test]
    fn corruption_mirroring_and_sanctification_match_the_items_own_state() {
        let rare = |edit: fn(&mut ParsedItem)| {
            let mut item = ParsedItem {
                name: "Dragon Core".to_string(),
                category: category("weapon.crossbow"),
                rarity: Some(ItemRarity::Rare),
                ..Default::default()
            };
            edit(&mut item);
            filtered_scope_of(&item, &[]).misc
        };
        // A corrupted item among corrupted ones, which share what corruption did to it.
        let corrupted = rare(|item| item.is_corrupted = true);
        assert_eq!(
            (
                corrupted.corrupted,
                corrupted.mirrored,
                corrupted.sanctified
            ),
            (Some(true), Some(false), Some(false))
        );
        // A mirrored item among its mirrored copies.
        assert_eq!(rare(|item| item.is_mirrored = true).mirrored, Some(true));
        // An unmodifiable item's corruption changes nothing for a buyer.
        assert_eq!(rare(|item| item.is_unmodifiable = true).corrupted, None);
        // An uncorrupted waystone can still be modified, which is what its buyer rolls it for
        // (PoE Overlay II's "Можно изменить").
        let waystone = ParsedItem {
            category: category("map.waystone"),
            rarity: Some(ItemRarity::Rare),
            ..Default::default()
        };
        assert_eq!(
            filtered_scope_of(&waystone, &[]).misc.corrupted,
            Some(false)
        );
        // An unidentified item among unidentified ones, whatever its rarity: an identified one
        // shows its mods and sells for them.
        let unidentified = ParsedItem {
            is_unidentified: true,
            rarity: Some(ItemRarity::Magic),
            ..waystone
        };
        assert_eq!(
            filtered_scope_of(&unidentified, &[]).misc.identified,
            Some(false)
        );

        let scope = filtered_scope_of(
            &ParsedItem {
                category: category("weapon.crossbow"),
                rarity: Some(ItemRarity::Rare),
                ..Default::default()
            },
            &[],
        );
        let body = filtered_search_body(&scope, &[], ListingStatus::Online);
        let misc = &serde_json::to_value(body).expect("the body serializes")["query"]["filters"]["misc_filters"]
            ["filters"];
        assert_eq!(
            misc,
            &serde_json::json!({
                "corrupted": { "option": "false" },
                "mirrored": { "option": "false" },
                "sanctified": { "option": "false" },
            })
        );
    }

    #[test]
    fn a_rare_switches_to_its_base_and_back_and_a_unique_has_no_other_scope() {
        let rare = ParsedItem {
            name: "Разумное чистилище".to_string(),
            base_type: Some("Тератновская пушка".to_string()),
            category: category("weapon.crossbow"),
            rarity: Some(ItemRarity::Rare),
            ..Default::default()
        };
        let by_category = filtered_scope_of(&rare, &[]);
        let by_base = switched_scope(&by_category, &rare, &[]).expect("a base to switch to");
        assert_eq!(
            by_base,
            SearchScope {
                base_type: Some("Тератновская пушка".to_owned()),
                rarity: Some(RarityFilter::Rare),
                misc: clean_gear(),
                ..SearchScope::default()
            }
        );
        assert_eq!(switched_scope(&by_base, &rare, &[]), Some(by_category));

        let unique = ParsedItem {
            rarity: Some(ItemRarity::Unique),
            ..rare
        };
        assert_eq!(
            switched_scope(&filtered_scope_of(&unique, &[]), &unique, &[]),
            None
        );
    }

    #[test]
    fn normal_equipment_is_searched_by_base_type_but_a_waystone_by_category() {
        let belt = ParsedItem {
            name: "Тяжёлый ремень".to_string(),
            category: category("accessory.belt"),
            rarity: Some(ItemRarity::Normal),
            ..Default::default()
        };
        assert_eq!(
            filtered_scope_of(&belt, &[]),
            SearchScope {
                base_type: Some("Тяжёлый ремень".to_owned()),
                rarity: Some(RarityFilter::Normal),
                // EE2's exact preset also leaves fractured items out.
                misc: MiscChoices {
                    fractured: Some(false),
                    ..clean_gear()
                },
                ..SearchScope::default()
            }
        );

        let waystone = ParsedItem {
            name: "Waystone (Tier 15)".to_string(),
            category: category("map.waystone"),
            rarity: Some(ItemRarity::Normal),
            ..Default::default()
        };
        let scope = filtered_scope_of(&waystone, &[]);
        assert_eq!(scope.base_type, None);
        assert_eq!(scope.category.as_deref(), Some("map.waystone"));
    }

    #[test]
    fn magic_flask_is_searched_by_the_longest_base_type_inside_its_name() {
        let flask = ParsedItem {
            name: "Bubbling Ultimate Life Flask of the Opportunist".to_string(),
            category: category("flask.life"),
            rarity: Some(ItemRarity::Magic),
            ..Default::default()
        };
        let scope = filtered_scope_of(&flask, &item_types(&["Life Flask", "Ultimate Life Flask"]));
        assert_eq!(scope.base_type.as_deref(), Some("Ultimate Life Flask"));
        assert_eq!(scope.category, None);
        assert_eq!(scope.rarity, Some(RarityFilter::Magic));

        let unresolved = filtered_scope_of(&flask, &[]);
        assert_eq!(unresolved.base_type, None);
        assert_eq!(
            unresolved.category.as_deref(),
            Some("flask.life"),
            "an unknown base falls back to the category"
        );
    }

    #[test]
    fn the_rarity_chip_switches_between_the_items_own_rarity_and_every_non_unique() {
        let item = |rarity| ParsedItem {
            rarity: Some(rarity),
            ..ParsedItem::default()
        };
        let magic = item(ItemRarity::Magic);
        assert_eq!(
            other_rarity(RarityFilter::Magic, &magic),
            Some(RarityFilter::NonUnique)
        );
        assert_eq!(
            other_rarity(RarityFilter::NonUnique, &magic),
            Some(RarityFilter::Magic)
        );
        assert_eq!(
            other_rarity(RarityFilter::NonUnique, &item(ItemRarity::Rare)),
            Some(RarityFilter::Rare)
        );
        // An unidentified unique's `unique` has no other way.
        assert_eq!(
            other_rarity(RarityFilter::Unique, &item(ItemRarity::Unique)),
            None
        );
    }

    fn route_debug(route: &SearchRoute) -> &'static str {
        match route {
            SearchRoute::Market { .. } => "Market",
            SearchRoute::Exact { .. } => "Exact",
            SearchRoute::Filtered { .. } => "Filtered",
        }
    }
}

#[cfg(test)]
mod trade_api_error_tests {
    use std::time::Duration;

    use futures::future::BoxFuture;
    use http_client::http::HeaderValue;
    use http_client::{Request, Url};

    use super::*;

    /// Answers every request with one canned response.
    struct CannedClient {
        status: u16,
        headers: &'static [(&'static str, &'static str)],
        body: &'static str,
    }

    impl HttpClient for CannedClient {
        fn user_agent(&self) -> Option<&HeaderValue> {
            None
        }

        fn proxy(&self) -> Option<&Url> {
            None
        }

        fn send(
            &self,
            _request: Request<AsyncBody>,
        ) -> BoxFuture<'static, Result<Response<AsyncBody>>> {
            let mut response = Response::builder().status(self.status);
            for &(name, value) in self.headers {
                response = response.header(name, value);
            }
            let response = response
                .body(AsyncBody::from(self.body))
                .map_err(anyhow::Error::from);
            Box::pin(async move { response })
        }
    }

    #[test]
    fn refusal_surfaces_as_a_trade_api_error_and_arms_the_limiter() {
        // The refusal captured live 2026-09-22: no `x-rate-limit-*` headers at all.
        let client: Arc<dyn HttpClient> = Arc::new(CannedClient {
            status: 429,
            headers: &[("retry-after", "259"), ("server", "cloudflare")],
            body: r#"{"error":{"code":3,"message":"Rate limit exceeded"}}"#,
        });
        let mut limiter = RateLimiter::new();

        let err = futures::executor::block_on(search_exact(
            &client,
            TradeSite::International,
            "Forbidden Rites",
            "Divine Orb",
            ListingStatus::Online,
            &mut limiter,
        ))
        .err()
        .expect("a refusal must not parse as a search result");

        assert_eq!(
            err.downcast_ref::<TradeApiError>(),
            Some(&TradeApiError {
                status: 429,
                code: Some(3),
                message: "Rate limit exceeded".to_owned(),
                retry_after_secs: Some(259),
            })
        );
        let wait = limiter
            .required_wait()
            .expect("the refusal's Retry-After arms the limiter");
        assert!((Duration::from_secs(250)..=Duration::from_secs(259)).contains(&wait));
    }

    #[test]
    fn rate_limited_by_status_or_by_envelope_code() {
        let headers = HeaderMap::new();
        assert!(api_error(429, &headers, "Too Many Requests").is_rate_limited());
        let code_3 = r#"{"error":{"code":3,"message":"Rate limit exceeded"}}"#;
        assert!(api_error(400, &headers, code_3).is_rate_limited());
        let code_2 = r#"{"error":{"code":2,"message":"Invalid query"}}"#;
        assert!(!api_error(400, &headers, code_2).is_rate_limited());
    }

    #[test]
    fn a_too_complex_query_is_told_apart_from_other_invalid_queries_on_both_sites() {
        let headers = HeaderMap::new();
        // The bodies the sites answered an anonymous weighted sum with, 2026-09-23.
        let english = r#"{"error":{"code":2,"message":"Query is too complex. Please reduce the amount of filters used.\nLogging in will increase this limit."}}"#;
        let russian = r#"{"error":{"code":2,"message":"Запрос слишком сложный. Пожалуйста, сократите количество используемых фильтров.\nАвторизация увеличит данный лимит."}}"#;
        assert!(api_error(400, &headers, english).is_too_complex());
        assert!(api_error(400, &headers, russian).is_too_complex());
        let invalid = r#"{"error":{"code":2,"message":"Invalid query"}}"#;
        assert!(!api_error(400, &headers, invalid).is_too_complex());
    }

    #[test]
    fn non_envelope_body_falls_back_to_its_trimmed_text_then_to_the_status_reason() {
        let headers = HeaderMap::new();
        let page = api_error(502, &headers, "\n  <html>502 Bad Gateway</html>\n");
        assert_eq!(page.code, None);
        assert_eq!(page.message, "<html>502 Bad Gateway</html>");

        // Three-byte characters: the cap has to cut on a character boundary, not a byte offset.
        let long = api_error(500, &headers, &"€".repeat(1000));
        // `+ 1`: the ellipsis marking the cut.
        assert!(long.message.chars().count() <= MAX_RAW_MESSAGE_CHARS + 1);

        assert_eq!(
            api_error(503, &headers, " \n").message,
            "Service Unavailable"
        );
    }
}

#[cfg(test)]
mod search_request_tests {
    use stat_filters::{FilterTag, RollBound, SearchFilter, SearchFilterRoll};

    use super::*;

    fn strength_filter(min: Option<f64>, max: Option<f64>) -> SearchFilter {
        SearchFilter {
            trade_ids: vec!["explicit.stat_4080418644".to_owned()],
            stat_ref: "# to Strength".to_owned(),
            display_text: "# to Strength".to_owned(),
            tag: FilterTag::Explicit,
            tier: None,
            roll: Some(SearchFilterRoll {
                value: 25.0,
                min,
                max,
                bound: RollBound::Higher,
                dp: false,
            }),
            enabled: true,
            hidden: false,
            generation: None,
            inverted: false,
            score: None,
            tier_info: None,
            weighted_sum: false,
        }
    }

    fn entry_json(filter: &SearchFilter) -> serde_json::Value {
        let entries = stat_filter_entries(filter).expect("the filter has a trade id and a roll");
        serde_json::to_value(&entries[0]).expect("the entry serializes")
    }

    #[test]
    fn unset_bounds_are_omitted_not_defaulted() {
        assert_eq!(
            entry_json(&strength_filter(Some(22.0), None)),
            serde_json::json!({
                "id": "explicit.stat_4080418644",
                "value": { "min": 22.0 },
                "disabled": false,
            })
        );
        assert_eq!(
            entry_json(&strength_filter(None, Some(28.0)))["value"],
            serde_json::json!({ "max": 28.0 })
        );
        // A flag stat, rolled at nothing: the listing has it or not.
        let flag = SearchFilter {
            roll: None,
            ..strength_filter(None, None)
        };
        assert_eq!(entry_json(&flag)["value"], serde_json::json!({}));
        // A row in the item's own words: "at least 15% reduced" is the catalog's "at most -15%
        // increased".
        let reduced = SearchFilter {
            inverted: true,
            ..strength_filter(Some(15.0), None)
        };
        assert_eq!(
            entry_json(&reduced)["value"],
            serde_json::json!({ "max": -15.0 })
        );
    }

    fn property(trade_id: &str, min: f64, enabled: bool) -> SearchFilter {
        SearchFilter {
            trade_ids: vec![trade_id.to_owned()],
            stat_ref: String::new(),
            display_text: String::new(),
            tag: FilterTag::Property,
            tier: None,
            roll: Some(SearchFilterRoll {
                value: min,
                min: Some(min),
                max: None,
                bound: RollBound::AtLeast,
                dp: false,
            }),
            enabled,
            hidden: false,
            generation: None,
            inverted: false,
            score: None,
            tier_info: None,
            weighted_sum: false,
        }
    }

    #[test]
    fn properties_fill_their_query_filter_and_stats_the_stat_group() {
        let scope = SearchScope {
            category: Some("armour.chest".to_owned()),
            rarity: Some(RarityFilter::NonUnique),
            ..SearchScope::default()
        };
        let filters = [
            property("equipment_filters.ar", 90.0, true),
            property("equipment_filters.ev", 120.0, false),
            strength_filter(Some(22.0), None),
        ];

        let body = filtered_search_body(&scope, &filters, ListingStatus::Securable);

        assert_eq!(
            serde_json::to_value(body).expect("the body serializes"),
            serde_json::json!({
                "query": {
                    "status": { "option": "securable" },
                    "stats": [{
                        "type": "and",
                        "filters": [{
                            "id": "explicit.stat_4080418644",
                            "value": { "min": 22.0 },
                            "disabled": false,
                        }],
                    }],
                    "filters": {
                        "equipment_filters": { "filters": { "ar": { "min": 90.0 } } },
                        "trade_filters": { "filters": { "collapse": { "option": "true" } } },
                        "type_filters": { "filters": {
                            "category": { "option": "armour.chest" },
                            "rarity": { "option": "nonunique" },
                        } },
                    },
                },
                "sort": { "price": "asc" },
            })
        );
    }

    #[test]
    fn an_exact_scope_sends_name_and_type_instead_of_a_category() {
        let scope = SearchScope {
            name: Some("The Eternal Spark".to_owned()),
            base_type: Some("Crystal Focus".to_owned()),
            ..SearchScope::default()
        };

        let body = filtered_search_body(&scope, &[], ListingStatus::Online);

        let query = &serde_json::to_value(body).expect("the body serializes")["query"];
        assert_eq!(query["name"], "The Eternal Spark");
        assert_eq!(query["type"], "Crystal Focus");
        assert_eq!(query["status"]["option"], "online");
        // No category, rarity or property: a seller's listings collapsed, nothing more.
        assert_eq!(
            query["filters"],
            serde_json::json!({ "trade_filters": { "filters": { "collapse": { "option": "true" } } } })
        );
    }

    #[test]
    fn a_weighted_sum_is_a_weight2_group_of_its_stats_kept_out_of_a_relaxed_count() {
        // A ring's rarity as PoE Overlay II searches it: every mod type's rarity stat, weight 1.
        let rarity = SearchFilter {
            trade_ids: vec![
                "explicit.stat_3917489142".to_owned(),
                "implicit.stat_3917489142".to_owned(),
            ],
            tag: FilterTag::Pseudo,
            weighted_sum: true,
            ..strength_filter(Some(30.0), None)
        };
        let filters = [strength_filter(Some(22.0), None), rarity.clone()];
        let weight2 = serde_json::json!({
            "type": "weight2",
            "value": { "min": 30.0 },
            "filters": [
                { "id": "explicit.stat_3917489142", "value": { "weight": 1.0 }, "disabled": false },
                { "id": "implicit.stat_3917489142", "value": { "weight": 1.0 }, "disabled": false },
            ],
        });
        let stats = |scope: &SearchScope, filters: &[SearchFilter]| {
            let body = filtered_search_body(scope, filters, ListingStatus::Online);
            serde_json::to_value(body).expect("the body serializes")["query"]["stats"].clone()
        };

        let every = stats(&SearchScope::default(), &filters);
        assert_eq!(every[0]["filters"].as_array().map(Vec::len), Some(1));
        assert_eq!(every[1], weight2);
        // It is its own group in a relaxed search too, and no row of the count: one stat row
        // beside it leaves nothing to relax.
        assert_eq!(enabled_stat_rows(&filters), 1);
        assert_eq!(one_fewer_match(&filters), None);
        let relaxed = SearchScope {
            stat_match: StatMatch::AtLeast(1),
            ..SearchScope::default()
        };
        assert_eq!(stats(&relaxed, &filters)[1], weight2);
        // Unchecked, the group is off.
        let unchecked = [SearchFilter {
            enabled: false,
            ..rarity
        }];
        assert_eq!(
            stats(&SearchScope::default(), &unchecked)[1]["disabled"],
            true
        );
    }

    #[test]
    fn the_relaxed_search_asks_for_all_the_enabled_stats_but_one() {
        let mut filters = vec![
            strength_filter(Some(22.0), None),
            strength_filter(Some(22.0), None),
            strength_filter(Some(22.0), None),
            strength_filter(Some(22.0), None),
            // Neither an unchecked row nor a property counts toward what is asked for.
            SearchFilter {
                enabled: false,
                ..strength_filter(Some(22.0), None)
            },
            property("equipment_filters.ar", 90.0, true),
        ];
        assert_eq!(enabled_stat_rows(&filters), 4);
        assert_eq!(one_fewer_match(&filters), Some((3, 4)));

        let scope = SearchScope {
            stat_match: StatMatch::AtLeast(3),
            ..SearchScope::default()
        };
        let body = filtered_search_body(&scope, &filters, ListingStatus::Available);
        let stats = &serde_json::to_value(body).expect("the body serializes")["query"]["stats"];
        assert_eq!(stats[0]["type"], "count");
        assert_eq!(stats[0]["value"], serde_json::json!({ "min": 3.0 }));
        assert_eq!(stats[0]["filters"].as_array().map(Vec::len), Some(5));

        // Two rows relax to either; a single row has no likeness to relax to.
        filters.truncate(2);
        assert_eq!(one_fewer_match(&filters), Some((1, 2)));
        filters.truncate(1);
        assert_eq!(one_fewer_match(&filters), None);
    }

    #[test]
    fn a_stat_several_trade_ids_share_matches_through_any_of_them() {
        // `# to all Attributes`, which the site files under two stat ids.
        let attributes = SearchFilter {
            trade_ids: vec![
                "explicit.stat_1379411836".to_owned(),
                "explicit.stat_2897413282".to_owned(),
            ],
            ..strength_filter(Some(10.0), None)
        };
        let filters = [strength_filter(Some(22.0), None), attributes.clone()];
        let every = SearchScope::default();
        let body = filtered_search_body(&every, &filters, ListingStatus::Online);
        let stats = &serde_json::to_value(body).expect("the body serializes")["query"]["stats"];
        let ids = |group: &serde_json::Value| -> Vec<String> {
            group["filters"]
                .as_array()
                .map(|entries| {
                    let ids = entries.iter().filter_map(|entry| entry["id"].as_str());
                    ids.map(str::to_owned).collect()
                })
                .unwrap_or_default()
        };
        assert_eq!(stats[0]["type"], "and");
        assert_eq!(ids(&stats[0]), ["explicit.stat_4080418644"]);
        assert_eq!(stats[1]["type"], "count");
        assert_eq!(stats[1]["value"], serde_json::json!({ "min": 1.0 }));
        assert_eq!(stats[1].get("disabled"), None);
        assert_eq!(
            ids(&stats[1]),
            ["explicit.stat_1379411836", "explicit.stat_2897413282"]
        );

        // Unchecked, the row's own group must not require the stat.
        let unchecked = [SearchFilter {
            enabled: false,
            ..attributes
        }];
        let body = filtered_search_body(&every, &unchecked, ListingStatus::Online);
        let stats = &serde_json::to_value(body).expect("the body serializes")["query"]["stats"];
        assert_eq!(stats[1]["disabled"], true);

        // Relaxed, the row counts once: its ids sit in the one count group beside the others.
        assert_eq!(enabled_stat_rows(&filters), 2);
        let scope = SearchScope {
            stat_match: StatMatch::AtLeast(1),
            ..SearchScope::default()
        };
        let body = filtered_search_body(&scope, &filters, ListingStatus::Online);
        let stats = &serde_json::to_value(body).expect("the body serializes")["query"]["stats"];
        assert_eq!(stats.as_array().map(Vec::len), Some(1));
        assert_eq!(ids(&stats[0]).len(), 3);
    }
}
