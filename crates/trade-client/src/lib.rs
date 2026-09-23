//! PoE2 trade API client (leagues -> search -> fetch). Endpoints and request/response shapes are
//! migrated from the real, independently-verified POC in
//! `crates/poe2-oracle/examples/trade_api.rs` (see `docs/dev/poc-findings.md`'s "Capability 5"
//! section for the original cross-check against a separate Python `urllib` call) into a real
//! library API here -- a move-and-refactor of already-proven logic, not a rewrite. The exact
//! endpoint URLs, request bodies, and rate-limit-relevant `GET`/`POST` shapes are unchanged from
//! the POC.
//!
//! Deliberately UI/runtime-agnostic: takes an already-constructed `Arc<dyn HttpClient>` rather
//! than building one itself (`reqwest_client::ReqwestClient` construction, including the
//! `USER_AGENT` choice, stays in `crates/poe2-oracle` -- the one place that actually knows what
//! kind of app is making the request). Depends only on `poe2-domain` (for the eventual shared
//! item/currency shapes -- still an empty skeleton as of this migration, see this crate's
//! `Cargo.toml`) and `http_client` (the trait, not a concrete client).
//!
//! `catalog`/`cache`/`rate_limit` extend this crate beyond trade *listings*: `catalog` fetches
//! the stat/item/currency catalogs those listings get filtered against
//! (`/api/trade2/data/{stats,items,static}`), `cache` gives that slow-changing data a disk
//! cache, and `rate_limit` tracks the real rate-limit response headers so a caller that owns an
//! executor can hold off until the trade API will take its next request -- this crate itself
//! never sleeps. `live` speaks the live search socket's protocol (the socket is the caller's), and
//! `account` asks whether the session the caller's client sends is signed in.
//!
//! Every trade API response, `catalog`'s included, goes through `checked_body`: a refusal (a `429`
//! while rate-limited, a rejected query, a Cloudflare error page) reaches the caller as a
//! [`TradeApiError`], never as a JSON parse error from feeding its body to a success-shape
//! parser. The account page is the exception: its 401 is an answer, not a refusal.

pub mod account;
pub mod cache;
pub mod catalog;
pub mod live;
pub mod ninja;
pub mod rate_limit;
pub mod rates;
pub mod scout;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
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

    /// The site's host name: the one the player's session cookie belongs to, and the live search
    /// socket's (`live::live_url`).
    pub fn host(self) -> &'static str {
        match self {
            TradeSite::International => "www.pathofexile.com",
            TradeSite::Russian => "ru.pathofexile.com",
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

/// One league as returned by `GET /api/trade2/data/leagues`.
#[derive(Debug, Deserialize)]
pub struct League {
    pub id: String,
}

#[derive(Deserialize)]
struct LeaguesResponse {
    result: Vec<League>,
}

/// `GET /api/trade2/data/leagues` -- every currently-active league, most-current first (the POC's
/// proven assumption: taking `leagues[0]` gives the current top league, e.g. "Forbidden Rites").
/// Always queried on `www`: league ids are shared across subdomains (only the display `text`
/// differs), and this crate only ever needs the id.
pub async fn leagues(client: &Arc<dyn HttpClient>) -> Result<Vec<League>> {
    let url = format!("{}/data/leagues", TradeSite::International.api_base());
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
    /// found nothing, relaxed -- at least some.
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
            let bounds = StatFilterValue { min, max: None };
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
/// min-only default leaves `roll.max` `undefined`.
#[derive(Serialize)]
struct StatFilterValue {
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<f64>,
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
/// group), or at least some -- its `"count"` group, what a search that finds nothing is relaxed
/// to.
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

/// The relaxed searches to try, in order, when matching every enabled stat row finds nothing:
/// all but one, then half of them rounded up. Fewer than three rows leave nothing worth relaxing
/// to -- one matching row is no likeness at all.
pub fn relaxed_matches(filters: &[stat_filters::SearchFilter]) -> Vec<StatMatch> {
    let wanted = enabled_stat_rows(filters);
    if wanted < 3 {
        return Vec::new();
    }
    let mut steps = vec![wanted - 1];
    let half = wanted.div_ceil(2);
    if half < wanted - 1 {
        steps.push(half);
    }
    steps.into_iter().map(StatMatch::AtLeast).collect()
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
/// bounds actually set (`roll.min`/`roll.max`): an unset bound stays open, never filled in from
/// `default_min`/`default_max`.
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
    if let Some(option) = scope.price.option() {
        let group = query_filters.entry("trade_filters").or_default();
        group
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
/// into its one `"count"` group instead, where a listing matches a row through any of them.
fn stat_groups(
    filters: &[stat_filters::SearchFilter],
    stat_match: StatMatch,
) -> Vec<StatGroup<'_>> {
    let rows = filters
        .iter()
        .filter_map(|filter| Some((filter, stat_filter_entries(filter)?)));
    if let StatMatch::AtLeast(count) = stat_match {
        return vec![StatGroup {
            kind: "count",
            value: Some(StatFilterValue {
                min: Some(f64::from(count)),
                max: None,
            }),
            disabled: false,
            filters: rows.flat_map(|(_, entries)| entries).collect(),
        }];
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
                }),
                disabled: !filter.enabled,
                filters: entries,
            });
        }
    }
    std::iter::once(every).chain(any_of).collect()
}

/// A stat row's `query.stats` entries, one per trade id, carrying the row's bounds -- none for a
/// row without a roll: a flag stat (`Enemies in your Presence are Blinded`), which a listing
/// matches by having it, sent with an empty `value` as EE2's `tradeIdToQuery` sends one. A row
/// in the item's own words (`SearchFilter::inverted`) has its bounds negated and swapped back
/// into the catalog's terms. `None` for a row without a trade id, and for a property row, which
/// `property_filter` places instead.
fn stat_filter_entries(filter: &stat_filters::SearchFilter) -> Option<Vec<StatFilterEntry<'_>>> {
    if filter.tag == stat_filters::FilterTag::Property || filter.trade_ids.is_empty() {
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
                value: StatFilterValue { min, max },
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
}

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
    /// priced from the market (poe.ninja, see `ninja`), never searched. `trade_id` is the item's
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
/// One `result[]` entry. Past the POC-verified basics (`name`, `typeLine`, `ilvl`, `price`,
/// `account.name`, `indexed`), fields follow EE2's `FetchResult` typing
/// (`renderer/src/web/price-check/trade/pathofexile-trade.ts`), whose `requestResults` reads them
/// into its results table; [`FetchedItem`] documents what each one means.
#[derive(Deserialize)]
struct FetchResultItem {
    item: FetchItem,
    listing: FetchListing,
    gone: Option<bool>,
}
#[derive(Deserialize)]
struct FetchItem {
    name: String,
    #[serde(rename = "typeLine")]
    type_line: String,
    ilvl: Option<u32>,
    /// Only its presence is read (`FetchedItem::has_note`), so the text is skipped unparsed.
    note: Option<serde::de::IgnoredAny>,
    #[serde(rename = "stackSize")]
    stack_size: Option<u32>,
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
#[derive(Deserialize)]
struct RawModTier {
    tier: Option<String>,
    level: Option<u32>,
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
}

/// One of a listed item's mods, as the trade site describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListedMod {
    pub kind: ModKind,
    /// The mod's text, the site's `[Link|Text]` markup reduced to the text it shows.
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
}

impl ListedMod {
    fn from_raw(kind: ModKind, raw: RawMod) -> ListedMod {
        match raw {
            RawMod::Text(text) => {
                let text = plain_mod_text(&text);
                ListedMod {
                    kind,
                    value: mod_value(&text),
                    text,
                    stat_id: None,
                    tier: None,
                    level: None,
                }
            }
            RawMod::Described {
                description,
                hash,
                mods,
            } => {
                let text = plain_mod_text(&description);
                ListedMod {
                    kind,
                    value: mod_value(&text),
                    text,
                    stat_id: hash.map(|hash| {
                        hash.strip_prefix("stat.")
                            .map_or(hash.clone(), str::to_owned)
                    }),
                    level: mods.iter().filter_map(|tier| tier.level).max(),
                    tier: mods.into_iter().find_map(|tier| tier.tier),
                }
            }
        }
    }
}

/// The roll a stat filter compares for a mod's `text`, read the way the trade site and EE2's
/// `getRollOrMinmaxAvg` read it: the mean of two or four numbers ("Adds 1 to 15" rolls 8), else
/// the first ("+93 to maximum Life", "-10% to Fire Resistance"). `None` for a mod without a
/// number, a flag.
fn mod_value(text: &str) -> Option<f64> {
    let numbers = numbers_in(text);
    match numbers.len() {
        0 => None,
        2 | 4 => Some(numbers.iter().sum::<f64>() / numbers.len() as f64),
        _ => Some(numbers[0]),
    }
}

/// The numbers in `text`, a `-` right before one making it negative, `.` or `,` its decimal
/// mark.
fn numbers_in(text: &str) -> Vec<f64> {
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
            numbers.push(if negative { -number } else { number });
        }
    }
    numbers
}

/// `+16% to [Resistances|Cold Resistance]` -> `+16% to Cold Resistance`, `[Lightning] damage` ->
/// `Lightning damage`: the text the site's links show.
fn plain_mod_text(text: &str) -> String {
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

/// One resolved listing from `fetch`. Presentation formatting (how to display the price, etc.)
/// is left to the caller -- this is raw parsed API data, not a UI-ready row.
#[derive(Debug)]
pub struct FetchedItem {
    pub name: String,
    pub type_line: String,
    /// `(amount, currency)`, e.g. `(1.0, "transmute")`; `None` if the listing has no set price.
    pub price: Option<(f64, String)>,
    pub account_name: String,
    /// The listed item's own item level (`item.ilvl`, verified live this session) -- `None` for
    /// item types with no item level at all (Currency, Divination Cards).
    pub item_level: Option<u32>,
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
    /// `item.note` is present: the item carries its own price. Without one, `price` comes from the
    /// stash tab's name -- a whole dump tab priced at once, which EE2 flags with a `?` after the
    /// price as "likely not real" (EE2 `docs/faq.md`).
    pub has_note: bool,
    /// `item.stackSize`; `None` for items that don't stack. [`group_listings`] sums it over a
    /// seller's folded listings.
    pub stack_size: Option<u32>,
    /// `listing.in_demand`, which EE2 renders as an "in demand" badge.
    pub in_demand: bool,
    /// The result's top-level `gone`, which EE2 renders as a red "Gone" badge.
    pub gone: bool,
    /// `listing.whisper`: the complete message to the seller, in the seller's language (live
    /// 2026-09-23: `@Virsavia Здравствуйте, хочу купить у вас Сердце Мина Кольцо с аметистом за 1
    /// exalted в лиге Standard (секция "~b/o 1 exalted"; позиция: 22 столбец, 21 ряд)` for a
    /// `ru_RU` seller). `None` for an instant-buyout listing, which needs no whisper.
    pub whisper: Option<String>,
    /// The item's mods in the order the site lists them: enchants, runes, implicits, fractured,
    /// explicits, desecrated.
    pub mods: Vec<ListedMod>,
}

/// `GET /api/trade2/fetch/{ids}?query={query_id}` for up to 10 listing ids at a time (the trade
/// API's own per-request limit; unenforced here since the POC never needed more than 5 -- pass a
/// pre-sliced `listing_ids` page). Entries the API returns as `null` (delisted between search and
/// fetch) are silently dropped, matching the POC's `.flatten()`.
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
        .map(|entry| {
            let item = entry.item;
            let mods = [
                (ModKind::Enchant, item.enchant_mods),
                (ModKind::Rune, item.rune_mods),
                (ModKind::Implicit, item.implicit_mods),
                (ModKind::Fractured, item.fractured_mods),
                (ModKind::Explicit, item.explicit_mods),
                (ModKind::Desecrated, item.desecrated_mods),
            ]
            .into_iter()
            .flat_map(|(kind, list)| {
                list.into_iter()
                    .map(move |raw| ListedMod::from_raw(kind, raw))
            })
            .collect();
            FetchedItem {
                name: item.name,
                type_line: item.type_line,
                price: entry.listing.price.map(|p| (p.amount, p.currency)),
                account_name: entry.listing.account.name,
                item_level: item.ilvl,
                indexed: entry.listing.indexed,
                account_status: match entry.listing.account.online {
                    None => AccountStatus::Offline,
                    Some(FetchOnline {
                        status: Some(FetchOnlineStatus::Afk),
                    }) => AccountStatus::Afk,
                    Some(_) => AccountStatus::Online,
                },
                instant_buyout: entry.listing.fee.is_some(),
                has_note: item.note.is_some(),
                stack_size: item.stack_size,
                in_demand: entry.listing.in_demand.unwrap_or(false),
                gone: entry.gone.unwrap_or(false),
                whisper: entry.listing.whisper.filter(|whisper| !whisper.is_empty()),
                mods,
            }
        })
        .collect())
}

/// One results-table row after [`group_listings`] folds a seller's repeats into it.
#[derive(Debug)]
pub struct GroupedListing {
    /// The row's first listing in search order, i.e. its cheapest. For a stackable item its
    /// `stack_size` is the stock of every listing folded into the row.
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
        match &mut group.listing.stack_size {
            Some(stock) => *stock += listing.stack_size.unwrap_or(0),
            None => group.listed_times += 1,
        }
    }
}

/// Convenience wrapper: current league (first from `leagues`) -> `search_with_filters` (no stat
/// filters, category only) -> `fetch`, taking at most `limit` listings. Mirrors the POC's
/// `run_trade_flow` end to end. Owns two short-lived [`RateLimiter`]s for this one call (search
/// and fetch are independent rate-limit families -- see `rate_limit`'s own doc comment -- sharing
/// one instance between them would let whichever response arrived last silently overwrite the
/// other endpoint's real state); a caller that issues many search/fetch calls over time (the
/// price-check app) should own and reuse its own longer-lived `RateLimiter`s instead of going
/// through this wrapper.
pub async fn search_current_league(
    client: &Arc<dyn HttpClient>,
    category: &str,
    limit: usize,
) -> Result<(String, u64, Vec<FetchedItem>)> {
    let league = leagues(client)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("leagues response had no entries"))?
        .id;
    let site = TradeSite::International;
    let mut search_limiter = RateLimiter::new();
    let scope = SearchScope {
        category: Some(category.to_owned()),
        ..SearchScope::default()
    };
    let outcome = search_with_filters(
        client,
        site,
        &league,
        &scope,
        &[],
        ListingStatus::Online,
        &mut search_limiter,
    )
    .await?;
    let take_ids: Vec<String> = outcome.listing_ids.into_iter().take(limit).collect();
    let mut fetch_limiter = RateLimiter::new();
    let items = fetch(
        client,
        site,
        &take_ids,
        &outcome.query_id,
        &mut fetch_limiter,
    )
    .await?;
    Ok((league, outcome.total, items))
}

/// A league id as one URL path segment or query value, encoded the way the trade site's own
/// frontend encodes it (JavaScript's `encodeURIComponent`): letters, digits and `-_.!~*'()` stay as
/// they are, every other byte of its UTF-8 becomes `%XX`. League ids hold spaces ("Forbidden
/// Rites") and a private league's parentheses ("My League (PL12345)"), and a name the player typed
/// may hold anything; the trade API, poe.ninja and poe2scout all take this form.
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

    /// A fetch page shaped after EE2's `FetchResult` typing around the POC-verified basics: an
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
            .map(|item| (item.account_status, item.instant_buyout, item.has_note))
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
            (crossbow.stack_size, crossbow.in_demand, crossbow.gone),
            (None, false, false)
        );
        assert_eq!(
            (splinters.stack_size, splinters.in_demand, splinters.gone),
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
            items[0].mods,
            [
                ListedMod {
                    kind: ModKind::Implicit,
                    text: "+10% to Chaos Resistance".to_owned(),
                    stat_id: Some("implicit.stat_2923486259".to_owned()),
                    tier: None,
                    level: Some(25),
                    value: Some(10.0),
                },
                ListedMod {
                    kind: ModKind::Explicit,
                    text: "Adds 1 to 15 Lightning damage to Attacks".to_owned(),
                    stat_id: Some("explicit.stat_1754445556".to_owned()),
                    tier: Some("P8".to_owned()),
                    level: Some(16),
                    value: Some(8.0),
                },
                ListedMod {
                    kind: ModKind::Explicit,
                    text: "+12 to maximum Life".to_owned(),
                    stat_id: None,
                    tier: None,
                    level: None,
                    value: Some(12.0),
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

    /// A priced, noted, unflagged listing: only what grouping reads varies.
    fn listing(account: &str, amount: f64, stack_size: Option<u32>) -> FetchedItem {
        FetchedItem {
            name: String::new(),
            type_line: "Alloy Crossbow".to_owned(),
            price: Some((amount, "exalted".to_owned())),
            account_name: account.to_owned(),
            item_level: None,
            indexed: String::new(),
            account_status: AccountStatus::Online,
            instant_buyout: false,
            has_note: true,
            stack_size,
            in_demand: false,
            gone: false,
            whisper: None,
            mods: Vec::new(),
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
        assert_eq!(groups[0].listing.stack_size, Some(55));
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
    use stat_filters::{FilterTag, SearchFilter, SearchFilterRoll};

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
                default_min: 22.0,
                default_max: 28.0,
                dp: false,
            }),
            enabled: true,
            hidden: false,
            generation: None,
            inverted: false,
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
                default_min: min,
                default_max: min,
                dp: false,
            }),
            enabled,
            hidden: false,
            generation: None,
            inverted: false,
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
        assert!(
            query.get("filters").is_none(),
            "no category, rarity or property"
        );
    }

    #[test]
    fn a_relaxed_search_asks_for_some_of_the_enabled_stats() {
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
        assert_eq!(
            relaxed_matches(&filters),
            [StatMatch::AtLeast(3), StatMatch::AtLeast(2)]
        );

        let scope = SearchScope {
            stat_match: StatMatch::AtLeast(3),
            ..SearchScope::default()
        };
        let body = filtered_search_body(&scope, &filters, ListingStatus::Available);
        let stats = &serde_json::to_value(body).expect("the body serializes")["query"]["stats"];
        assert_eq!(stats[0]["type"], "count");
        assert_eq!(stats[0]["value"], serde_json::json!({ "min": 3.0 }));
        assert_eq!(stats[0]["filters"].as_array().map(Vec::len), Some(5));

        // Two stats matched in full or not at all: nothing in between to relax to.
        filters.truncate(2);
        assert!(relaxed_matches(&filters).is_empty());
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
