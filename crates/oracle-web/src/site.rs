//! The site, laid out as GitHub Pages published it: the landing pages from `SITE_DIR` at the root
//! (English, Russian under `/ru/`), the guide's two books from `GUIDE_DIR` under `/guide/en/` and
//! `/guide/ru/`, and the guide's pictures from `IMAGES_DIR`: under `/images/` for the landing pages,
//! and under `/guide/images/`, where the books' `../images/<lang>/` links lead.
//!
//! Paths resolve the way Pages resolved them, so the pages' relative links keep working: a
//! directory is its `index.html`, a directory named without the trailing slash redirects to it
//! (relative links in the page need it), and a directory without an `index.html` is a 404, as is
//! any file or directory whose name starts with a dot. A missing page is answered with a 404 page
//! when there is one: under `/guide/en/` and `/guide/ru/` the book's own, elsewhere
//! `SITE_DIR/404.html`. tower-http's `ServeFile` sends the files, with `ETag`/`Last-Modified`,
//! conditional requests and ranges.
//!
//! The ways in lead to the reader's language: `/` keeps an English reader and sends a Russian one
//! on to `/ru/`, and `/guide/`, which has no page of its own, sends each reader to their book. The
//! reader's language is the one they last picked on a language switch (the `lang` cookie the
//! pages' script sets), else the first of the two their browser asks for (`Accept-Language`, which
//! browsers fill from the system's languages), else English. A path that names its language is
//! served as it is, whatever the reader's: that is how a link, or the switch itself, reaches the
//! other language.
//!
//! A page loaded counts a view, and a visitor if it is a new one that day ([`crate::usage`]); one
//! loaded from a link tagged with where it was published, `?from=reddit` (the tag of a [`Source`]),
//! counts a visit for that tag and its visitor too, and any other tag counts nothing. Only a GET of
//! an HTML page counts: not the files a page uses, not a missing page, not a HEAD, which only
//! checks the link, and not the guide's `toc.html`, the frame each of its pages loads. A page counts
//! where it's served, so the redirects on the way keep the query -- `/` to `/ru/` too -- and the
//! page's script can carry the tag on to the download links.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use percent_encoding::percent_decode_str;
use tower_http::services::ServeFile;

use crate::App;
use crate::distinct::Who;
use crate::stats::Source;
use crate::usage;

/// Pages must show an edit at once: browsers ask again every time (a 304 when nothing changed).
const PAGE_CACHE: &str = "no-cache";
/// Styles, scripts, fonts and pictures may wait a little, as they did on Pages.
const ASSET_CACHE: &str = "public, max-age=600";
/// What a way in's answer depends on ([`preferred`]): a cache must keep one per reader's headers,
/// or it would send one reader on to another's language.
const LANGUAGE_VARY: &str = "Accept-Language, Cookie";

pub struct Site {
    site: PathBuf,
    guide: PathBuf,
    images: PathBuf,
}

/// Where a request path leads.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    File(PathBuf),
    /// A directory with an `index.html`, named without its trailing slash.
    Directory,
    Missing,
}

/// The two languages of the site and of the guide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Language {
    En,
    Ru,
}

impl Language {
    /// The language `tag` names, in any case: a cookie's value, or a language range's primary
    /// subtag.
    fn named(tag: &[u8]) -> Option<Language> {
        if tag.eq_ignore_ascii_case(b"en") {
            Some(Language::En)
        } else if tag.eq_ignore_ascii_case(b"ru") {
            Some(Language::Ru)
        } else {
            None
        }
    }

    /// Its code, which names the directories of its pages: `/ru/`, `/guide/en/`.
    fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Ru => "ru",
        }
    }
}

impl Site {
    pub fn new(site: PathBuf, guide: PathBuf, images: PathBuf) -> Site {
        Site {
            site,
            guide,
            images,
        }
    }

    /// The directory a request path's files come from, and the rest of the path under it.
    /// `/guide/images` comes before `/guide`: the books' pictures are `IMAGES_DIR`'s, as the
    /// landing pages' are.
    fn mount<'a>(&'a self, path: &'a str) -> (&'a Path, &'a str) {
        for (prefix, root) in [
            ("/guide/images", &self.images),
            ("/guide", &self.guide),
            ("/images", &self.images),
        ] {
            if let Some(rest) = path.strip_prefix(prefix)
                && (rest.is_empty() || rest.starts_with('/'))
            {
                return (root, rest);
            }
        }
        (&self.site, path)
    }
}

/// Resolves `path` (as the request has it, percent-encoded) under `root`.
async fn resolve(root: &Path, path: &str) -> Target {
    let Ok(path) = percent_decode_str(path).decode_utf8() else {
        return Target::Missing;
    };
    let mut file = root.to_path_buf();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        let mut components = Path::new(segment).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(_)), None) if !segment.starts_with('.') => file.push(segment),
            _ => return Target::Missing,
        }
    }
    let Ok(metadata) = tokio::fs::metadata(&file).await else {
        return Target::Missing;
    };
    let named_as_directory = path.ends_with('/');
    if metadata.is_dir() {
        let index = file.join("index.html");
        match tokio::fs::metadata(&index).await {
            Ok(index_metadata) if index_metadata.is_file() => {
                if named_as_directory {
                    Target::File(index)
                } else {
                    Target::Directory
                }
            }
            _ => Target::Missing,
        }
    } else if metadata.is_file() && !named_as_directory {
        Target::File(file)
    } else {
        Target::Missing
    }
}

/// Every path the API doesn't claim.
pub async fn serve(State(app): State<Arc<App>>, request: Request) -> Response {
    let path = request.uri().path();
    // `/` only for a page load: another method gets the page's 405, whatever the language.
    let home = path == "/" && matches!(*request.method(), Method::GET | Method::HEAD);
    let guide = matches!(path, "/guide" | "/guide/" | "/guide/index.html");
    if !home && !guide {
        return answer(&app, request).await;
    }
    let language = preferred(request.headers());
    let location = if guide {
        Some(format!("/guide/{}/", language.code()))
    } else {
        (language == Language::Ru).then(|| "/ru/".to_owned())
    };
    // A 302, not a 301: the way leads elsewhere for another reader, or after the next pick.
    let mut response = match location {
        // The page it leads to counts the visit and its script reads the tag: the query goes along.
        Some(location) => redirect(StatusCode::FOUND, location, request.uri().query()),
        None => answer(&app, request).await,
    };
    let headers = response.headers_mut();
    headers.insert(header::VARY, HeaderValue::from_static(LANGUAGE_VARY));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(PAGE_CACHE));
    response
}

/// `request` answered from the directories: the file its path names, the redirect of a directory
/// named without its slash, or a 404. A page it serves counts a view and, when its link is tagged,
/// a visit.
async fn answer(app: &Arc<App>, request: Request) -> Response {
    let uri = request.uri().clone();
    let (root, rest) = app.site.mount(uri.path());
    match resolve(root, rest).await {
        Target::File(file) => {
            if is_page(&file) {
                page_loaded(app, &request, &file);
            }
            send(file, request).await
        }
        Target::Directory => redirect(
            StatusCode::MOVED_PERMANENTLY,
            format!("{}/", uri.path()),
            uri.query(),
        ),
        Target::Missing => not_found(&app.site, uri.path(), request.method()).await,
    }
}

/// A redirect to `location`, with the request's `query` when it's kept.
fn redirect(status: StatusCode, mut location: String, query: Option<&str>) -> Response {
    if let Some(query) = query {
        location.push('?');
        location.push_str(query);
    }
    (status, [(header::LOCATION, location)]).into_response()
}

/// Counts a page load: a GET of an HTML page other than the guide's `toc.html`, which each of its
/// pages loads as a frame. A HEAD doesn't count: browsers load pages with a GET, and a HEAD only
/// checks the link.
fn page_loaded(app: &Arc<App>, request: &Request, file: &Path) {
    if request.method() != Method::GET || file.file_name().is_some_and(|name| name == "toc.html") {
        return;
    }
    let source = request.uri().query().and_then(Source::in_query);
    usage::page_loaded(app, Who::of(request), source);
}

/// The language a request's reader prefers: the one they last picked on a language switch, else
/// the first of the two their browser asks for, else English.
fn preferred(headers: &HeaderMap) -> Language {
    picked(headers)
        .or_else(|| asked(headers))
        .unwrap_or(Language::En)
}

/// The `lang` cookie: the language the reader last picked on a language switch. Only the pages'
/// script sets it, and a value it doesn't write is no pick. Read as bytes: a cookie another page
/// set may hold anything, and must not hide this one.
fn picked(headers: &HeaderMap) -> Option<Language> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .flat_map(|cookies| cookies.as_bytes().split(|&byte| byte == b';'))
        .find_map(|cookie| {
            let (name, value) = name_value(cookie)?;
            if name != b"lang" {
                return None;
            }
            // A value may come quoted (RFC 6265, 4.1.1).
            let value = value
                .strip_prefix(b"\"")
                .and_then(|value| value.strip_suffix(b"\""))
                .unwrap_or(value);
            Language::named(value)
        })
}

/// The first of the two languages the browser asks for: of the ranges naming them, the one it
/// weighs highest, the earlier of equals. The region doesn't matter (`ru-RU` is `ru`); `q=0`
/// refuses a language rather than asks for it; `*` names neither; and a range whose weight isn't
/// one can't be ranked, so it is skipped.
fn asked(headers: &HeaderMap) -> Option<Language> {
    let ranges = headers
        .get_all(header::ACCEPT_LANGUAGE)
        .iter()
        .flat_map(|ranges| ranges.as_bytes().split(|&byte| byte == b','));
    let mut first: Option<(f32, Language)> = None;
    for range in ranges {
        let mut parts = range.split(|&byte| byte == b';');
        let tag = parts.next().unwrap_or_default().trim_ascii();
        let primary = tag
            .split(|&byte| byte == b'-' || byte == b'_')
            .next()
            .unwrap_or_default();
        let Some(language) = Language::named(primary) else {
            continue;
        };
        let Some(weight) = weight(parts) else {
            continue;
        };
        if weight > 0.0 && first.is_none_or(|(highest, _)| weight > highest) {
            first = Some((weight, language));
        }
    }
    first.map(|(_, language)| language)
}

/// A language range's weight, from its parameters (RFC 9110, 12.4.2): 1 without a `q`, `None`
/// when its `q` isn't a number from 0 to 1.
fn weight<'a>(mut parameters: impl Iterator<Item = &'a [u8]>) -> Option<f32> {
    let Some(q) = parameters.find_map(|parameter| {
        let (name, value) = name_value(parameter)?;
        name.eq_ignore_ascii_case(b"q").then_some(value)
    }) else {
        return Some(1.0);
    };
    std::str::from_utf8(q)
        .ok()?
        .parse::<f32>()
        .ok()
        .filter(|q| (0.0..=1.0).contains(q))
}

/// `pair` split at its first `=`, both sides trimmed: a cookie, or a parameter.
fn name_value(pair: &[u8]) -> Option<(&[u8], &[u8])> {
    let equals = pair.iter().position(|&byte| byte == b'=')?;
    Some((pair[..equals].trim_ascii(), pair[equals + 1..].trim_ascii()))
}

/// Whether `file` is a page, which a reader loads, rather than a file a page uses: HTML.
fn is_page(file: &Path) -> bool {
    file.extension()
        .is_some_and(|extension| extension == "html")
}

/// `file` as `ServeFile` sends it, with the site's caching and text charset.
async fn send(file: PathBuf, request: Request) -> Response {
    let page = is_page(&file);
    let mut response = match ServeFile::new(file).try_call(request).await {
        Ok(response) => response.map(Body::new),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(error) => {
            tracing::error!(%error, "couldn't read a site file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let status = response.status();
    let headers = response.headers_mut();
    // A 304 carries it too: the browser's copy gets its caching from the latest answer.
    if status.is_success() || status == StatusCode::NOT_MODIFIED {
        let cache = if page { PAGE_CACHE } else { ASSET_CACHE };
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    }
    // The site's texts are UTF-8, but a type without a charset leaves the browser guessing: a
    // Russian .txt would show as mojibake.
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.starts_with("text/") || value.starts_with("application/javascript"))
        .filter(|value| !value.contains("charset"))
        .and_then(|value| HeaderValue::from_str(&format!("{value}; charset=utf-8")).ok());
    if let Some(content_type) = content_type {
        headers.insert(header::CONTENT_TYPE, content_type);
    }
    response
}

/// A 404 for the missing `path`, with a 404 page when there is one: under `/guide/en/` and
/// `/guide/ru/` the book's own, in its language and with its sidebar, else the site's.
async fn not_found(site: &Site, path: &str, method: &Method) -> Response {
    if let Some(language) = book(path) {
        let mut page = site.guide.join(language.code());
        page.push("404.html");
        if let Some(response) = page_404(page, method).await {
            return response;
        }
    }
    page_404(site.site.join("404.html"), method)
        .await
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

/// The book a path is in: `/guide/ru/...` is in the Russian one.
fn book(path: &str) -> Option<Language> {
    let (code, _) = path.strip_prefix("/guide/")?.split_once('/')?;
    [Language::En, Language::Ru]
        .into_iter()
        .find(|language| language.code() == code)
}

/// `page` sent as a 404, without the validators and ranges that belong to the page rather than to
/// the missing path; `None` when it can't be sent.
async fn page_404(page: PathBuf, method: &Method) -> Option<Response> {
    let method = if method == Method::HEAD {
        Method::HEAD
    } else {
        Method::GET
    };
    let request = Request::builder()
        .method(method)
        .uri("/404.html")
        .body(Body::empty())
        .expect("a fixed request is valid");
    let mut response = match ServeFile::new(page).try_call(request).await {
        Ok(response) if response.status() == StatusCode::OK => response.map(Body::new),
        _ => return None,
    };
    *response.status_mut() = StatusCode::NOT_FOUND;
    let headers = response.headers_mut();
    for validator in [header::ETAG, header::LAST_MODIFIED, header::ACCEPT_RANGES] {
        headers.remove(validator);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(PAGE_CACHE));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    Some(response)
}

/// tower-http's validators are strong, and stay so on a gzip or br body; a compressed body is a
/// different representation, so its validator must be weak (RFC 9110, 8.8.1). Conditional
/// requests still match: `If-None-Match` compares weakly.
pub async fn weaken_encoded_etag(mut response: Response) -> Response {
    if response.headers().contains_key(header::CONTENT_ENCODING)
        && let Some(etag) = response.headers().get(header::ETAG)
        && !etag.as_bytes().starts_with(b"W/")
    {
        let mut weak = b"W/".to_vec();
        weak.extend_from_slice(etag.as_bytes());
        if let Ok(weak) = HeaderValue::from_bytes(&weak) {
            response.headers_mut().insert(header::ETAG, weak);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::stats::snapshots;
    use crate::{Config, moscow};

    #[tokio::test]
    async fn paths_resolve_as_on_github_pages() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        std::fs::create_dir_all(path.join("ru")).unwrap();
        std::fs::create_dir_all(path.join("ui")).unwrap();
        std::fs::create_dir_all(path.join(".git")).unwrap();
        std::fs::write(path.join("index.html"), "en").unwrap();
        std::fs::write(path.join("ru/index.html"), "ru").unwrap();
        std::fs::write(path.join("ui/base.css"), "css").unwrap();
        std::fs::write(path.join("ui/имя файла.txt"), "txt").unwrap();
        std::fs::write(path.join(".git/config"), "secret").unwrap();

        assert_eq!(
            resolve(path, "/").await,
            Target::File(path.join("index.html"))
        );
        assert_eq!(resolve(path, "").await, Target::Directory);
        assert_eq!(
            resolve(path, "/ru/").await,
            Target::File(path.join("ru/index.html"))
        );
        assert_eq!(resolve(path, "/ru").await, Target::Directory);
        assert_eq!(
            resolve(path, "/ui/base.css").await,
            Target::File(path.join("ui/base.css"))
        );
        assert_eq!(
            resolve(
                path,
                "/ui/%D0%B8%D0%BC%D1%8F%20%D1%84%D0%B0%D0%B9%D0%BB%D0%B0.txt"
            )
            .await,
            Target::File(path.join("ui/имя файла.txt"))
        );
        // A directory without index.html, a file named as a directory, and anything outside.
        for missing in [
            "/ui/",
            "/ui",
            "/ui/base.css/",
            "/nope.html",
            "/../index.html",
            "/ru/%2e%2e/%2e%2e/etc/passwd",
            "/.git/config",
            "/ui/..%2Fbase.css",
            "/%ff",
        ] {
            assert_eq!(resolve(path, missing).await, Target::Missing, "{missing}");
        }
    }

    /// The language [`preferred`] gives a request with `headers`, sent in this order.
    fn preferred_for(headers: &[(header::HeaderName, &str)]) -> Language {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.append(name, HeaderValue::from_bytes(value.as_bytes()).unwrap());
        }
        preferred(&map)
    }

    #[test]
    fn a_language_picked_on_a_switch_beats_the_browsers() {
        use Language::{En, Ru};
        use header::{ACCEPT_LANGUAGE, COOKIE};

        assert_eq!(
            preferred_for(&[(COOKIE, "lang=ru"), (ACCEPT_LANGUAGE, "en-US,en;q=0.9")]),
            Ru
        );
        assert_eq!(
            preferred_for(&[(COOKIE, "lang=en"), (ACCEPT_LANGUAGE, "ru-RU,ru;q=0.9")]),
            En
        );
        // Among other cookies however spaced, on any of several Cookie lines, next to a cookie
        // that isn't ASCII, quoted.
        for cookies in [
            &[(COOKIE, "theme=dark;lang=ru ;  seen=1")][..],
            &[(COOKIE, "theme=dark"), (COOKIE, " lang = ru ")],
            &[(COOKIE, "имя=значение; lang=ru")],
            &[(COOKIE, "lang=\"ru\"")],
        ] {
            let mut headers = cookies.to_vec();
            headers.push((ACCEPT_LANGUAGE, "en"));
            assert_eq!(preferred_for(&headers), Ru, "{cookies:?}");
        }
        // A value the switch doesn't write is no pick: the browser's language counts.
        for cookie in ["lang=de", "lang=", "lang", "xlang=ru"] {
            for (asked, language) in [("ru", Ru), ("en", En)] {
                assert_eq!(
                    preferred_for(&[(COOKIE, cookie), (ACCEPT_LANGUAGE, asked)]),
                    language,
                    "{cookie} {asked}"
                );
            }
        }
    }

    #[test]
    fn the_browsers_first_language_of_the_two_counts() {
        use Language::{En, Ru};
        use header::ACCEPT_LANGUAGE;

        for (asked, language) in [
            ("ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7", Ru),
            ("en-GB,en;q=0.9,ru;q=0.8", En),
            // By weight, other languages aside, the header's order breaking ties.
            ("en;q=0.5, ru;q=0.8", Ru),
            ("de-DE, de;q=0.9, ru;q=0.3, en;q=0.2", Ru),
            ("uk, en-US;q=0.5, ru;q=0.500", En),
            ("ru, en", Ru),
            ("en, ru", En),
            // Any region or script, in any case.
            ("RU-ua", Ru),
            ("ru_RU", Ru),
            ("sr-Latn-RS, ru-Cyrl-RU;q=0.4, en-Latn-US;q=0.3", Ru),
            // `*` is any other language, neither of the two.
            ("*, ru;q=0.5", Ru),
            // A range that can't be read doesn't hide the next.
            ("en;q=abc, ru;q=0.5", Ru),
            ("ру-РУ, ru;q=0.5", Ru),
            (",, ;q=1, ru ; q = 0.5 ,", Ru),
        ] {
            assert_eq!(
                preferred_for(&[(ACCEPT_LANGUAGE, asked)]),
                language,
                "{asked}"
            );
        }
        // Several Accept-Language lines are one list.
        assert_eq!(
            preferred_for(&[(ACCEPT_LANGUAGE, "de"), (ACCEPT_LANGUAGE, "ru")]),
            Ru
        );
    }

    #[test]
    fn a_reader_asking_for_neither_gets_english() {
        use header::ACCEPT_LANGUAGE;

        assert_eq!(preferred_for(&[]), Language::En);
        for asked in [
            "*",
            "de-DE,de;q=0.9,*;q=0.5",
            // Rusyn.
            "rue",
            "ru;q=0",
            "de, ru-RU;q=0",
            "ru;q=2, ru;q=-1, ru;q=, ru;q=NaN, ru;q=inf",
            "ру, ;;, =",
        ] {
            assert_eq!(
                preferred_for(&[(ACCEPT_LANGUAGE, asked)]),
                Language::En,
                "{asked}"
            );
        }
    }

    /// The tags the published links carry.
    const TAGS: [&str; 9] = [
        "reddit", "forum", "discord", "youtube", "steam", "wiki", "lists", "creators", "article",
    ];

    /// The service over throwaway directories: both landing pages, a stylesheet and the guide's
    /// two books.
    fn serving_pages() -> (Arc<App>, [tempfile::TempDir; 3]) {
        let dirs = [(); 3].map(|()| tempfile::tempdir().unwrap());
        for (dir, file) in [
            (0, "index.html"),
            (0, "ru/index.html"),
            (0, "ui/base.css"),
            (1, "en/index.html"),
            (1, "ru/index.html"),
            (1, "en/toc.html"),
        ] {
            let path = dirs[dir].path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, file).unwrap();
        }
        let app = App::new(Config {
            site_dir: dirs[0].path().to_owned(),
            guide_dir: dirs[1].path().to_owned(),
            images_dir: dirs[2].path().to_owned(),
            ..Config::default()
        })
        .unwrap();
        (app, dirs)
    }

    /// `path` loaded the way a browser sending `headers` loads it: its redirects followed to the
    /// page.
    async fn load(app: &Arc<App>, path: &str, headers: &[(&str, &str)]) -> Response {
        let mut path = path.to_owned();
        for _ in 0..3 {
            let mut request = Request::get(path.as_str());
            for (name, value) in headers {
                request = request.header(*name, *value);
            }
            let response = serve(State(app.clone()), request.body(Body::empty()).unwrap()).await;
            let Some(location) = response.headers().get(header::LOCATION) else {
                return response;
            };
            path = location.to_str().unwrap().to_owned();
        }
        panic!("still redirected at {path}");
    }

    /// The visits the links tagged `tag` brought today, as the morning digest counts them.
    async fn visits(app: &App, tag: &str) -> u64 {
        let now = moscow::now();
        let key = format!("oracle:stat:visit_{tag}:{}", moscow::Day::of(now));
        app.store.values(&[key], now).await.unwrap()[0]
    }

    #[tokio::test(start_paused = true)]
    async fn each_published_tag_counts_one_visit_however_its_link_leads_to_the_page() {
        let (app, _dirs) = serving_pages();
        let russian = ("accept-language", "ru-RU,ru;q=0.9,en;q=0.8");
        for (path, headers) in [
            // `/` sends a Russian reader on to `/ru/`, and keeps an English one.
            ("/?from=reddit", &[russian][..]),
            ("/?utm_source=feed&from=forum", &[][..]),
            // A directory named without its slash, and the guide's way in to a book.
            ("/ru?from=discord", &[][..]),
            ("/guide/?from=youtube", &[russian][..]),
            // A page's own address.
            ("/ru/?from=steam", &[][..]),
            ("/guide/en/?from=wiki", &[][..]),
            ("/index.html?from=lists", &[russian][..]),
            // Of several, the first `from` that names a source.
            ("/?from=evil&from=creators", &[][..]),
            ("/?from=article&from=reddit", &[][..]),
        ] {
            let page = load(&app, path, headers).await;
            assert_eq!(page.status(), StatusCode::OK, "{path}");
        }
        // The counts are made in the background: in once the runtime has nothing else to do.
        tokio::time::sleep(Duration::from_millis(1)).await;
        for tag in TAGS {
            assert_eq!(visits(&app, tag).await, 1, "{tag}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_unknown_tag_or_a_request_that_loads_no_page_counts_nothing() {
        let (app, _dirs) = serving_pages();
        for (method, path) in [
            (Method::GET, "/?from=evil"),
            (Method::GET, "/?source=reddit"),
            (Method::GET, "/?xfrom=reddit&q=from=reddit"),
            // A file a page uses, a missing page, and a check of the link.
            (Method::GET, "/ui/base.css?from=reddit"),
            (Method::GET, "/missing.html?from=reddit"),
            (Method::HEAD, "/?from=reddit"),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .unwrap();
            serve(State(app.clone()), request).await;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
        for tag in TAGS.into_iter().chain(["evil"]) {
            assert_eq!(visits(&app, tag).await, 0, "{tag}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_page_counts_a_view_and_a_visitor_but_no_file_frame_or_link_check_does() {
        let (app, _dirs) = serving_pages();
        // One visitor loads three pages, a book's frame and a stylesheet, and asks for a page that
        // isn't there; another loads a page; a third only checks a link.
        for (method, path, client) in [
            (Method::GET, "/index.html", "203.0.113.1"),
            (Method::GET, "/ru/", "203.0.113.1"),
            (Method::GET, "/guide/en/", "203.0.113.1"),
            (Method::GET, "/guide/en/toc.html", "203.0.113.1"),
            (Method::GET, "/ui/base.css", "203.0.113.1"),
            (Method::GET, "/missing.html", "203.0.113.1"),
            (Method::GET, "/index.html", "203.0.113.2"),
            (Method::HEAD, "/index.html", "203.0.113.3"),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .header("x-forwarded-for", client)
                .header("user-agent", "Mozilla/5.0 Firefox/130.0")
                .body(Body::empty())
                .unwrap();
            serve(State(app.clone()), request).await;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
        let now = moscow::now();
        let today = snapshots(&app.store, &[moscow::Day::of(now)], now)
            .await
            .unwrap()
            .remove(0);
        assert_eq!(today.get("page_view"), 4);
        assert_eq!(today.get("uniq_site_day"), 2);
        // No tag, no visit.
        assert_eq!(today.sum("visit_"), 0);
        assert_eq!(today.sum("uniq_site_day_from_"), 0);
    }
}
