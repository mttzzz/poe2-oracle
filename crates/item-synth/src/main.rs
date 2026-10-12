//! `item-synth`: makes items from trade listings and keeps them in the library.
//!
//! ```text
//! item-synth order <name> (--query <json|@file> | --from <saved search>) [--order <words>]
//!                         [--league <id>] [--pick <n>] [--class <English class>] [--note]
//! item-synth check [<name>...]
//! item-synth render <item.json> --lang en|ru [--class <English class>] [--note] [--check]
//!                   [--body]
//! item-synth stand-in <name> --lang en|ru --out <folder> [--art]
//! ```
//!
//! `order` searches the English trade site with the query (`query` of a search request), takes
//! the first listing it can write (or the `--pick`th), fetches it from both sites, writes its
//! texts, parses them back the way the app does, runs the app's own search for each and saves
//! the entry under `<name>`; `--from` takes the listings from a saved search's results instead
//! (`{"query": ..., "en": [...], "ru": [...]}`, fetch results). `check` writes and parses the
//! library's entries again against the live catalogs, and rebuilds the library's catalogs.
//! `render` writes one item JSON (a listing's `item`, or the whole listing); `--check` parses it
//! back, `--body` prints the app's search request for it. `stand-in` puts a library item where a
//! stand-in build of the app takes it from (`--out`: that build's `data\stand-in` folder, or a
//! folder to copy there), its art too with `--art`.
//!
//! Run it from the repository root, where it keeps the trade catalogs and RePoE's base item
//! table it downloads in `.tmp/item-synth`. The trade site's limits are shared with the app and
//! with whatever else runs from the same address, so every request waits at least
//! [`REQUEST_GAP`] after the one before, and a refusal (`429`) stops the tool.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail, ensure};
use http_client::HttpClient;
use item_parser::{IndexedCatalog, ItemLanguage, parse_clipboard};
use item_synth::check::round_trip;
use item_synth::classes::{self, Classes, REPOE_FILES};
use item_synth::library::{self, Entry, LANGUAGES, Languages, Meta};
use item_synth::listing::Item;
use item_synth::{Options, render};
use poe2_domain::StatCatalog;
use trade_client::catalog::{ItemTypeEntry, StaticCurrency};
use trade_client::rate_limit::RateLimiter;
use trade_client::{ListingStatus, SearchRoute, TradeSite};

/// The least time between two requests to the trade site.
const REQUEST_GAP: Duration = Duration::from_secs(20);
/// How old the cached trade catalogs may get before they are fetched again.
const CATALOG_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// How many of a search's listings `order` fetches to find one it can write.
const CANDIDATES: usize = 5;
const USER_AGENT: &str = concat!(
    "PoE2-Oracle-item-synth/",
    env!("CARGO_PKG_VERSION"),
    " (+https://oracle.pushka.biz)"
);

fn main() {
    if let Err(err) = run(std::env::args().skip(1).collect()) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<()> {
    let mut args = Args::new(args);
    match args.next().as_deref() {
        Some("order") => order(args),
        Some("check") => check(args),
        Some("render") => render_file(args),
        Some("stand-in") => stand_in(args),
        _ => bail!(
            "usage: item-synth order|check|render|stand-in ... (see the source's doc comment)"
        ),
    }
}

/// The command line after the command: positional words and `--flag value` pairs.
struct Args {
    words: Vec<String>,
}

impl Args {
    fn new(words: Vec<String>) -> Args {
        Args { words }
    }

    /// The next positional word. Commands take their flags first, so what is left is positional.
    fn next(&mut self) -> Option<String> {
        let at = self.words.iter().position(|word| !word.starts_with("--"))?;
        Some(self.words.remove(at))
    }

    fn rest(&mut self) -> Vec<String> {
        let mut rest = Vec::new();
        while let Some(word) = self.next() {
            rest.push(word);
        }
        rest
    }

    fn flag(&mut self, name: &str) -> Option<String> {
        let at = self.words.iter().position(|word| word == name)?;
        self.words.remove(at);
        (at < self.words.len()).then(|| self.words.remove(at))
    }

    fn switch(&mut self, name: &str) -> bool {
        match self.words.iter().position(|word| word == name) {
            Some(at) => {
                self.words.remove(at);
                true
            }
            None => false,
        }
    }

    fn done(self) -> Result<()> {
        ensure!(
            self.words.is_empty(),
            "unexpected arguments: {:?}",
            self.words
        );
        Ok(())
    }
}

fn language_of(code: &str) -> Result<ItemLanguage> {
    match code {
        "en" => Ok(ItemLanguage::English),
        "ru" => Ok(ItemLanguage::Russian),
        other => bail!("a language is en or ru, not {other:?}"),
    }
}

fn site_of(language: ItemLanguage) -> TradeSite {
    match language {
        ItemLanguage::English => TradeSite::International,
        ItemLanguage::Russian => TradeSite::Russian,
    }
}

/// Where the tool keeps what it downloads.
fn cache_dir() -> PathBuf {
    PathBuf::from(".tmp/item-synth")
}

/// Writes a library item for a stand-in build (`poe2-oracle --features stand-in`, its
/// `src/stand_in.rs`): its text as `item.txt` in `--out`, and with `--art` the listing's art as
/// `art.png`.
fn stand_in(mut args: Args) -> Result<()> {
    let art = args.switch("--art");
    let language = language_of(&args.flag("--lang").context("--lang en|ru")?)?;
    let out = PathBuf::from(args.flag("--out").context("--out <folder>")?);
    let slug = args.next().context("a library item's name")?;
    args.done()?;
    let entry = Entry::load(&library::root().join(&slug))?;
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
    std::fs::write(out.join("item.txt"), entry.texts.get(language))?;
    eprintln!("wrote {}", out.join("item.txt").display());
    if art {
        let icon = &entry.items.get(language).icon;
        let tool = Tool::new()?;
        let png = futures::executor::block_on(tool.download(icon))?;
        std::fs::write(out.join("art.png"), png)?;
        eprintln!("wrote {}", out.join("art.png").display());
    }
    Ok(())
}

fn render_file(mut args: Args) -> Result<()> {
    let note = args.switch("--note");
    let checked = args.switch("--check");
    let body = args.switch("--body");
    let language = language_of(&args.flag("--lang").context("--lang en|ru")?)?;
    let class = args.flag("--class");
    let path = args.next().context("an item JSON file")?;
    args.done()?;
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
    let value: serde_json::Value = serde_json::from_str(&text).context("parsing the JSON")?;
    let value = value.get("item").cloned().unwrap_or(value);
    let item: Item = serde_json::from_value(value).context("reading the item")?;
    let tool = Tool::new()?;
    futures::executor::block_on(async {
        let class = match class {
            Some(class) => classes::named(&class)?,
            None => {
                ensure!(
                    language == ItemLanguage::English,
                    "an item from the Russian site needs --class: its base is named in Russian"
                );
                tool.classes().await?.of(&item.base_type)?
            }
        };
        let synthesized = render(
            &item,
            class.name(language),
            language,
            Options { with_note: note },
        )?;
        print!("{}", synthesized.text);
        for guess in &synthesized.guesses {
            eprintln!("guess: {guess}");
        }
        if checked || body {
            let catalogs = tool.catalogs(language).await?;
            if body {
                match app_search(&catalogs, language, &synthesized.text)? {
                    Ok(body) => eprintln!("{}", serde_json::to_string_pretty(&body)?),
                    Err(why) => eprintln!("{why}"),
                }
            }
            if checked {
                let problems = round_trip(
                    &item,
                    &synthesized.text,
                    language,
                    &catalogs.indexed,
                    &catalogs.stats,
                );
                for problem in &problems {
                    eprintln!("round trip: {problem}");
                }
                ensure!(
                    problems.is_empty(),
                    "the app doesn't read it back as the listing says"
                );
            }
        }
        Ok(())
    })
}

/// The HTTP client, its pace and what it has fetched.
struct Tool {
    client: Arc<dyn HttpClient>,
    search: RateLimiter,
    fetch: RateLimiter,
    last_request: Option<Instant>,
}

impl Tool {
    fn new() -> Result<Tool> {
        let client = reqwest_client::ReqwestClient::proxy_and_user_agent(None, USER_AGENT)?;
        Ok(Tool {
            client: Arc::new(client),
            search: RateLimiter::new(),
            fetch: RateLimiter::new(),
            last_request: None,
        })
    }

    /// Holds off until the next trade request may go: [`REQUEST_GAP`] after the last one, and
    /// whatever the site's own limits say.
    fn pace(&mut self) {
        let mut wait = self
            .last_request
            .map(|last| REQUEST_GAP.saturating_sub(last.elapsed()))
            .unwrap_or_default();
        for limiter in [&self.search, &self.fetch] {
            if let Some(more) = limiter.required_wait() {
                wait = wait.max(more);
            }
        }
        if !wait.is_zero() {
            eprintln!(
                "  (waiting {}s for the trade site's limits)",
                wait.as_secs()
            );
            std::thread::sleep(wait);
        }
        self.last_request = Some(Instant::now());
    }

    async fn classes(&self) -> Result<Classes> {
        let dir = cache_dir().join("repoe");
        std::fs::create_dir_all(&dir)?;
        for (name, url) in REPOE_FILES {
            let path = dir.join(name);
            if path.is_file() {
                continue;
            }
            eprintln!("downloading {url}");
            std::fs::write(&path, self.download(url).await?)?;
        }
        Classes::load(&dir)
    }

    /// A file off the web (not the trade API): RePoE's tables, an item's art.
    async fn download(&self, url: &str) -> Result<Vec<u8>> {
        let mut response = self
            .client
            .get(url, http_client::AsyncBody::default(), true)
            .await
            .with_context(|| format!("GET {url}"))?;
        ensure!(
            response.status().is_success(),
            "GET {url}: {}",
            response.status()
        );
        let mut body = Vec::new();
        futures::AsyncReadExt::read_to_end(response.body_mut(), &mut body).await?;
        Ok(body)
    }

    /// One site's catalogs the app parses and routes with, cached for a day.
    async fn catalogs(&self, language: ItemLanguage) -> Result<Catalogs> {
        let site = site_of(language);
        let code = library::code(language);
        let dir = cache_dir();
        let stats: StatCatalog = trade_client::cache::load_or_fetch(
            &dir.join(format!("stats.{code}.json")),
            CATALOG_AGE,
            trade_client::catalog::fetch_stat_catalog(&self.client, site),
        )
        .await?;
        let currencies: Vec<StaticCurrency> = trade_client::cache::load_or_fetch(
            &dir.join(format!("static.{code}.json")),
            CATALOG_AGE,
            trade_client::catalog::fetch_static_currencies(&self.client, site),
        )
        .await?;
        let item_types: Vec<ItemTypeEntry> = trade_client::cache::load_or_fetch(
            &dir.join(format!("items.{code}.json")),
            CATALOG_AGE,
            trade_client::catalog::fetch_item_types(&self.client, site),
        )
        .await?;
        Ok(Catalogs {
            indexed: IndexedCatalog::new(stats.clone()),
            stats,
            currencies,
            item_types,
        })
    }

    async fn search(
        &mut self,
        language: ItemLanguage,
        league: &str,
        body: &serde_json::Value,
    ) -> Result<trade_client::SearchOutcome> {
        self.pace();
        trade_client::search_body(
            &self.client,
            site_of(language),
            league,
            body,
            &mut self.search,
        )
        .await
    }

    async fn fetch(
        &mut self,
        language: ItemLanguage,
        ids: &[String],
        query_id: Option<&str>,
    ) -> Result<Vec<Option<serde_json::Value>>> {
        self.pace();
        trade_client::fetch_json(
            &self.client,
            site_of(language),
            ids,
            query_id,
            &mut self.fetch,
        )
        .await
    }
}

struct Catalogs {
    stats: StatCatalog,
    indexed: IndexedCatalog,
    currencies: Vec<StaticCurrency>,
    item_types: Vec<ItemTypeEntry>,
}

/// `at` as UTC, to the second: `2026-10-12T10:20:00Z`.
fn utc(at: SystemTime) -> String {
    let seconds = at
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// A saved search's results, which `order --from` takes instead of searching: `{"query": ...,
/// "en": [results from www], "ru": [the same results from ru]}`, each result a `fetch` one.
#[derive(serde::Deserialize)]
struct Saved {
    query: serde_json::Value,
    en: Vec<Option<serde_json::Value>>,
    ru: Vec<Option<serde_json::Value>>,
}

fn order(mut args: Args) -> Result<()> {
    let note = args.switch("--note");
    let query = args.flag("--query");
    let from = args.flag("--from");
    let words = args.flag("--order");
    let league = args
        .flag("--league")
        .unwrap_or_else(|| "Forbidden Rites".to_owned());
    let pick: Option<usize> = args.flag("--pick").map(|pick| pick.parse()).transpose()?;
    let class = args.flag("--class");
    let slug = args.next().context("a name for the entry")?;
    args.done()?;
    library::check_slug(&slug)?;
    let root = library::root();
    ensure!(!root.join(&slug).exists(), "the library already has {slug}");
    // When the listing was fetched: now, or when the saved search was.
    let mut fetched = SystemTime::now();
    let saved: Option<Saved> = from
        .map(|path| -> Result<Saved> {
            let text = std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
            fetched = std::fs::metadata(&path)?.modified()?;
            serde_json::from_str(&text).with_context(|| format!("{path} is no saved search"))
        })
        .transpose()?;
    let query: serde_json::Value = match (&saved, query) {
        (Some(saved), None) => saved.query.clone(),
        (None, Some(query)) => match query.strip_prefix('@') {
            Some(path) => serde_json::from_str(
                &std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?,
            )?,
            None => serde_json::from_str(&query).context("--query is no JSON")?,
        },
        _ => bail!("either --query <json|@file> or --from <saved search>"),
    };

    futures::executor::block_on(async {
        let mut tool = Tool::new()?;
        let classes = tool.classes().await?;
        let english = match &saved {
            Some(saved) => match pick {
                Some(pick) => saved.en.get(pick).cloned().into_iter().collect(),
                None => saved.en.clone(),
            },
            None => {
                let body = serde_json::json!({ "query": query, "sort": { "price": "asc" } });
                let found = tool.search(ItemLanguage::English, &league, &body).await?;
                eprintln!("{} listings", found.total);
                let ids: Vec<String> = match pick {
                    Some(pick) => found.listing_ids.get(pick).cloned().into_iter().collect(),
                    None => found.listing_ids.iter().take(CANDIDATES).cloned().collect(),
                };
                ensure!(!ids.is_empty(), "the search found nothing to take");
                tool.fetch(ItemLanguage::English, &ids, Some(&found.query_id))
                    .await?
            }
        };

        // The first listing that reads as an item and writes; why the others don't.
        let mut chosen = None;
        for result in english.into_iter().flatten() {
            let id = result["id"].as_str().unwrap_or_default().to_owned();
            let account = result["listing"]["account"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let attempt = (|| -> Result<(Item, &'static item_parser::categories::ItemClass)> {
                let item: Item = serde_json::from_value(result["item"].clone())
                    .context("reading the listing's item")?;
                let class = match &class {
                    Some(class) => classes::named(class)?,
                    None => classes.of(&item.base_type)?,
                };
                render(
                    &item,
                    class.name(ItemLanguage::English),
                    ItemLanguage::English,
                    Options { with_note: note },
                )?;
                Ok((item, class))
            })();
            match attempt {
                Ok((item, class)) => {
                    chosen = Some((id, account, item, class));
                    break;
                }
                Err(err) => eprintln!("  skipping {id}: {err:#}"),
            }
        }
        let (id, account, english, class) =
            chosen.context("none of the listings fetched can be written")?;
        let russian = match &saved {
            Some(saved) => saved
                .ru
                .iter()
                .flatten()
                .find(|result| result["id"] == id.as_str())
                .cloned(),
            None => tool
                .fetch(ItemLanguage::Russian, std::slice::from_ref(&id), None)
                .await?
                .into_iter()
                .flatten()
                .next(),
        }
        .context("the Russian site has no such listing")?;
        let russian: Item = serde_json::from_value(russian["item"].clone())
            .context("reading the Russian listing's item")?;

        let options = Options { with_note: note };
        let mut texts = Languages::<String>::default();
        let mut guesses = Languages::<Vec<String>>::default();
        let items = Languages {
            en: english,
            ru: russian,
        };
        for language in LANGUAGES {
            let synthesized = render(items.get(language), class.name(language), language, options)?;
            *texts.get_mut(language) = synthesized.text;
            *guesses.get_mut(language) = synthesized.guesses;
        }
        let mut entry = Entry {
            slug: slug.clone(),
            meta: Meta {
                order: words.unwrap_or_else(|| slug.clone()),
                query,
                league: league.clone(),
                fetched: utc(fetched),
                class: class.en.to_owned(),
                note,
                guesses,
                finds_itself: Languages::default(),
            },
            items,
            texts,
        };

        let mut failed = false;
        for language in LANGUAGES {
            let code = library::code(language);
            let catalogs = tool.catalogs(language).await?;
            let problems = round_trip(
                entry.items.get(language),
                entry.texts.get(language),
                language,
                &catalogs.indexed,
                &catalogs.stats,
            );
            for problem in &problems {
                eprintln!("{code}: {problem}");
            }
            failed |= !problems.is_empty();
            let result = finds_itself(
                &mut tool,
                &catalogs,
                language,
                &league,
                entry.texts.get(language),
                &id,
                &account,
            )
            .await;
            let result = match result {
                Ok(result) => result,
                // A refusal stops the tool; anything else is this check's answer.
                Err(err) if is_refusal(&err) => return Err(err),
                Err(err) => format!("the search failed: {err:#}"),
            };
            eprintln!("{code}: finds itself: {result}");
            *entry.meta.finds_itself.get_mut(language) = result;
        }
        entry.save(&root)?;
        refresh_catalogs(&mut tool, &root).await?;
        print!("{}", entry.texts.en);
        for language in LANGUAGES {
            for guess in entry.meta.guesses.get(language) {
                eprintln!("{} guess: {guess}", library::code(language));
            }
        }
        eprintln!("saved {}", root.join(&slug).display());
        ensure!(
            !failed,
            "the app doesn't read the item back as the listing says"
        );
        Ok(())
    })
}

fn is_refusal(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<trade_client::TradeApiError>()
            .is_some_and(trade_client::TradeApiError::is_rate_limited)
    })
}

/// The search the app runs for the item `text` reads as, on `language`'s site, with every seller
/// (`status` any): its request body, or why it runs none (an exchange item).
fn app_search(
    catalogs: &Catalogs,
    language: ItemLanguage,
    text: &str,
) -> Result<std::result::Result<serde_json::Value, String>> {
    let item = parse_clipboard(text, language, &catalogs.indexed)
        .map_err(|err| anyhow::anyhow!("the app can't parse it: {err}"))?;
    let profile = stat_filters::SearchProfile::default_for(&item);
    let filters = stat_filters::build_filters(
        &item,
        profile,
        &catalogs.stats,
        stat_filters::Session::Anonymous,
    );
    Ok(
        match trade_client::route_search(&item, &catalogs.currencies, &catalogs.item_types) {
            SearchRoute::Market { trade_id } => Err(format!(
                "priced from the exchange market ({trade_id}), no search to run"
            )),
            SearchRoute::Exact { exact_type } => Ok(trade_client::exact_search_request(
                &exact_type,
                ListingStatus::Any,
            )),
            SearchRoute::Filtered { scope } => Ok(trade_client::filtered_search_request(
                &scope,
                &filters,
                ListingStatus::Any,
            )),
        },
    )
}

/// The app's own search for the item `text` reads as, on `language`'s site, narrowed to the
/// listing's seller: `found` when it lists listing `id`, or what it did instead.
async fn finds_itself(
    tool: &mut Tool,
    catalogs: &Catalogs,
    language: ItemLanguage,
    league: &str,
    text: &str,
    id: &str,
    account: &str,
) -> Result<String> {
    let mut body = match app_search(catalogs, language, text)? {
        Ok(body) => body,
        Err(why) => return Ok(why),
    };
    ensure!(!account.is_empty(), "the listing names no seller");
    let trade_filters = body["query"]["filters"]
        .as_object_mut()
        .context("a search body without filters")?
        .entry("trade_filters")
        .or_insert_with(|| serde_json::json!({ "filters": {} }));
    let trade_filters = trade_filters["filters"]
        .as_object_mut()
        .context("trade_filters without filters")?;
    // The app folds a seller's listings of one item into one (`collapse`), which may stand for
    // the seller's twin of this listing rather than for it: seen live 2026-10-12 on a waystone
    // the English site's fold left out and the Russian one's didn't.
    trade_filters.remove("collapse");
    trade_filters.insert(
        "account".to_owned(),
        serde_json::json!({ "input": account }),
    );
    let found = tool.search(language, league, &body).await?;
    Ok(if found.listing_ids.iter().any(|listed| listed == id) {
        "found".to_owned()
    } else {
        format!(
            "not found: the app's search finds {} of the seller's listings, not this one",
            found.total
        )
    })
}

/// Rebuilds the library's catalogs from the cached live ones for every entry's lines.
async fn refresh_catalogs(tool: &mut Tool, root: &Path) -> Result<()> {
    let entries = library::entries(root)?;
    for language in LANGUAGES {
        let catalogs = tool.catalogs(language).await?;
        // A text's lines, each mod's header taken off its first stat; a stat that takes two
        // lines stays one, as the catalog has it.
        let lines: Vec<String> = entries
            .iter()
            .flat_map(|entry| entry.texts.get(language).split("\r\n"))
            .map(|line| match line.strip_prefix('{') {
                Some(headed) => headed.split_once('\n').map_or("", |(_, stat)| stat),
                None => line,
            })
            .map(str::to_owned)
            .collect();
        let subset = library::catalog_subset(&catalogs.stats, &lines, language);
        library::save_catalog(root, language, &subset)?;
    }
    Ok(())
}

fn check(mut args: Args) -> Result<()> {
    let only = args.rest();
    args.done()?;
    let root = library::root();
    futures::executor::block_on(async {
        let mut tool = Tool::new()?;
        let mut failed = 0;
        let entries = library::entries(&root)?;
        for language in LANGUAGES {
            let code = library::code(language);
            let catalogs = tool.catalogs(language).await?;
            for entry in &entries {
                if !only.is_empty() && !only.contains(&entry.slug) {
                    continue;
                }
                let options = Options {
                    with_note: entry.meta.note,
                };
                let item = entry.items.get(language);
                let synthesized = render(item, entry.class(language)?, language, options)?;
                let mut problems = Vec::new();
                if &synthesized.text != entry.texts.get(language) {
                    problems.push("written differently now than the library has it".to_owned());
                }
                problems.extend(round_trip(
                    item,
                    &synthesized.text,
                    language,
                    &catalogs.indexed,
                    &catalogs.stats,
                ));
                for problem in &problems {
                    eprintln!("{} {code}: {problem}", entry.slug);
                }
                if problems.is_empty() {
                    eprintln!("{} {code}: ok", entry.slug);
                } else {
                    failed += 1;
                }
            }
        }
        refresh_catalogs(&mut tool, &root).await?;
        ensure!(failed == 0, "{failed} texts don't hold up");
        Ok(())
    })
}
