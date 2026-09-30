//! What a request says of itself, as far as the counts go: whether it is the app's, which version
//! of the app, and -- on the app's first connection of a run -- the flags of a start.
//!
//! The app's updater names itself in every request it makes, `User-Agent: PoE2-Oracle/<version>`;
//! the report window adds ` (+https://oracle.pushka.biz)` after the version. Nothing else the
//! service serves sends that product token, so it tells the app from a browser. Anyone can write
//! it, and everything made of it is checked: a version counts under its own name only when it is
//! one of the newest published releases ([`Version`]), a language only when it is `en` or `ru`.

use axum::http::{HeaderMap, header};
use oracle_protocol::{DEV_PARAM, FIRST_PARAM, FROM_PARAM, LANG_PARAM, START_PARAM};

/// The product token the app's requests start with, before the `/` and its version.
pub const APP_PRODUCT: &str = "PoE2-Oracle";
/// The most bytes of a User-Agent kept: browsers send a few hundred.
const MAX_AGENT_BYTES: usize = 512;

/// The `User-Agent` of a request, cut at [`MAX_AGENT_BYTES`]; empty when it has none or isn't text.
pub fn user_agent(headers: &HeaderMap) -> &str {
    let agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let mut end = agent.len().min(MAX_AGENT_BYTES);
    while !agent.is_char_boundary(end) {
        end -= 1;
    }
    &agent[..end]
}

/// A request from the app, as its User-Agent names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppAgent {
    /// The version it names, when that is a plain version ([`plain_version`]).
    version: Option<String>,
}

impl AppAgent {
    /// The app behind `headers`; `None` when the request isn't from it.
    pub fn of(headers: &HeaderMap) -> Option<AppAgent> {
        let rest = user_agent(headers)
            .strip_prefix(APP_PRODUCT)?
            .strip_prefix('/')?;
        let token = rest.split(' ').next().unwrap_or_default();
        Some(AppAgent {
            version: plain_version(token).map(str::to_owned),
        })
    }

    /// The version, when it is a plain one.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

/// `text` when it is a plain version: three numbers joined by dots, `^\d+\.\d+\.\d+$`, in a
/// name's worth of characters. A pre-release (`0.2.0-rc.1`) or anything else is not.
pub fn plain_version(text: &str) -> Option<&str> {
    let mut parts = text.split('.');
    let numbers = [parts.next()?, parts.next()?, parts.next()?];
    let plain = parts.next().is_none()
        && text.len() <= 24
        && numbers
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    plain.then_some(text)
}

/// A version as a counter's name has it: the version itself when it is one of the newest published
/// releases the service knows, else `other`. A stranger's User-Agent thus can't make counters up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version(String);

impl Version {
    /// What a version that isn't a known release counts under.
    pub const OTHER: &'static str = "other";

    /// `version` as a name: itself when `known` -- plain versions -- holds it, else [`Version::OTHER`].
    pub fn bound(version: Option<&str>, known: &[String]) -> Version {
        match version {
            Some(version) if known.iter().any(|known| known == version) => {
                Version(version.to_owned())
            }
            _ => Version(Version::OTHER.to_owned()),
        }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The interface language a start reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
    /// Any other value, or none.
    Other,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Ru, Lang::Other];

    /// Its name in a counter's: `app_start_lang_<name>`.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
            Lang::Other => "other",
        }
    }

    fn of(value: Option<&str>) -> Lang {
        match value {
            Some("en") => Lang::En,
            Some("ru") => Lang::Ru,
            _ => Lang::Other,
        }
    }
}

/// What the app's first connection of a run says of the start ([`START_PARAM`] and the others
/// beside it, in `oracle-protocol`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    /// The installation's first start.
    pub first: bool,
    /// The version the last start ran, when it was another one and is a plain version.
    pub from: Option<String>,
    pub lang: Lang,
    /// A build the developer made for testing: its start is counted as that and as nothing else.
    pub dev: bool,
}

impl Start {
    /// The start `query` reports; `None` unless it has `start=1`, which the reconnections'
    /// requests lack. Of a parameter given twice the first counts; values are taken as written,
    /// none is percent-decoded: what the app sends needs no escapes, and what does isn't one.
    pub fn of(query: Option<&str>) -> Option<Start> {
        let query = query?;
        let value = |name: &str| {
            query
                .split('&')
                .filter_map(|parameter| parameter.split_once('='))
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value)
        };
        if value(START_PARAM) != Some("1") {
            return None;
        }
        Some(Start {
            first: value(FIRST_PARAM) == Some("1"),
            from: value(FROM_PARAM).and_then(plain_version).map(str::to_owned),
            lang: Lang::of(value(LANG_PARAM)),
            dev: value(DEV_PARAM) == Some("1"),
        })
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(agent: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::USER_AGENT, HeaderValue::from_str(agent).unwrap());
        headers
    }

    fn app_version(agent: &str) -> Option<Option<String>> {
        AppAgent::of(&headers(agent)).map(|app| app.version)
    }

    #[test]
    fn the_app_is_told_by_its_product_token_and_its_version_by_the_digits() {
        assert_eq!(
            app_version("PoE2-Oracle/0.1.3"),
            Some(Some("0.1.3".to_owned()))
        );
        // The report window's client adds the site's address after the version.
        assert_eq!(
            app_version("PoE2-Oracle/0.1.3 (+https://oracle.pushka.biz)"),
            Some(Some("0.1.3".to_owned()))
        );
        assert_eq!(
            app_version("PoE2-Oracle/12.345.6789"),
            Some(Some("12.345.6789".to_owned()))
        );
        // The app, whose version is nothing a counter can carry.
        for odd in [
            "PoE2-Oracle/",
            "PoE2-Oracle/0.1",
            "PoE2-Oracle/0.1.3.4",
            "PoE2-Oracle/0.2.0-rc.1",
            "PoE2-Oracle/0.1.x",
            "PoE2-Oracle/.1.3",
            "PoE2-Oracle/0..3",
            "PoE2-Oracle/v0.1.3",
            "PoE2-Oracle/0.1.3+build",
            "PoE2-Oracle/123456789012345678901234567890.1.3",
        ] {
            assert_eq!(app_version(odd), Some(None), "{odd}");
        }
    }

    #[test]
    fn nothing_else_is_the_app() {
        for other in [
            "",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Gecko/20100101 Firefox/130.0",
            "curl/8.5.0",
            // Another tool's, whose name only begins with the app's.
            "PoE2-Oracle-Tool/0.1.2 (+https://oracle.pushka.biz)",
            "poe2-oracle/0.1.3",
            "PoE2-Oracle 0.1.3",
            "Not PoE2-Oracle/0.1.3",
            "PoE2-Oraclee/0.1.3",
            // Not text a header can carry as text.
            "PoE2-Oracle/1.٣.3",
        ] {
            assert_eq!(app_version(other), None, "{other}");
        }
        assert!(AppAgent::of(&HeaderMap::new()).is_none());
    }

    #[test]
    fn a_version_counts_by_name_only_as_a_known_release_else_as_other() {
        let known = ["0.1.3".to_owned(), "0.1.2".to_owned()];
        for (version, name) in [
            (Some("0.1.3"), "0.1.3"),
            (Some("0.1.2"), "0.1.2"),
            (Some("0.1.4"), "other"),
            (Some("9.9.9"), "other"),
            (Some("0.1.03"), "other"),
            (None, "other"),
        ] {
            assert_eq!(
                Version::bound(version, &known).to_string(),
                name,
                "{version:?}"
            );
        }
        // With no release known -- no GitHub -- every version is `other`.
        assert_eq!(Version::bound(Some("0.1.3"), &[]).to_string(), "other");
    }

    #[test]
    fn a_stranger_cannot_make_up_more_names_than_there_are_releases() {
        let known: Vec<String> = (0..16).map(|patch| format!("0.1.{patch}")).collect();
        let names: std::collections::HashSet<String> = (0..10_000)
            .map(|number| format!("{number}.{}.{}", number % 7, number % 13))
            .map(|forged| Version::bound(Some(&forged), &known).to_string())
            .chain(
                known
                    .iter()
                    .map(|version| Version::bound(Some(version), &known).to_string()),
            )
            .collect();
        assert_eq!(names.len(), 16 + 1, "the releases, and `other`");
    }

    #[test]
    fn a_start_is_only_a_query_with_start_1() {
        let start = |query: &str| Start::of(Some(query));
        assert_eq!(
            start("start=1&first=1&from=0.1.2&lang=ru"),
            Some(Start {
                first: true,
                from: Some("0.1.2".to_owned()),
                lang: Lang::Ru,
                dev: false,
            })
        );
        assert_eq!(
            start("start=1&lang=en&dev=1"),
            Some(Start {
                first: false,
                from: None,
                lang: Lang::En,
                dev: true,
            })
        );
        // Order doesn't matter, and the first of a repeated parameter counts.
        assert_eq!(
            start("lang=ru&start=1&lang=en&first=0&first=1").map(|start| (start.lang, start.first)),
            Some((Lang::Ru, false))
        );
        // A reconnection carries no query; flags without the start are nobody's.
        assert_eq!(Start::of(None), None);
        for query in [
            "",
            "first=1&from=0.1.2&lang=ru",
            "start=0",
            "start=2",
            "start=",
            "start=1x",
            "Start=1",
            "xstart=1",
            "dev=1",
        ] {
            assert_eq!(start(query), None, "{query}");
        }
    }

    #[test]
    fn a_starts_values_are_checked() {
        let start = |query: &str| Start::of(Some(query)).unwrap();
        // The language is `en` or `ru`, else `other`, missing included.
        for (query, lang) in [
            ("start=1&lang=en", Lang::En),
            ("start=1&lang=ru", Lang::Ru),
            ("start=1&lang=de", Lang::Other),
            ("start=1&lang=RU", Lang::Other),
            ("start=1&lang=", Lang::Other),
            ("start=1", Lang::Other),
        ] {
            assert_eq!(start(query).lang, lang, "{query}");
        }
        // A version only in its plain form.
        for from in ["0.1.2", "10.20.30"] {
            assert_eq!(
                start(&format!("start=1&from={from}")).from.as_deref(),
                Some(from)
            );
        }
        for from in ["", "0.1", "0.2.0-rc.1", "latest", "0.1.2%20", "../.."] {
            assert_eq!(start(&format!("start=1&from={from}")).from, None, "{from}");
        }
        // `first` and `dev` are 1 or nothing.
        assert!(!start("start=1&first=true").first);
        assert!(!start("start=1&dev=yes").dev);
        assert!(start("start=1&first=1&dev=1").dev);
    }
}
