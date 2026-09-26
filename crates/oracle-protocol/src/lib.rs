//! The contract between PoE2 Oracle, the Windows app, and its web service at oracle.pushka.biz
//! (`crates/oracle-web`): where the service is, what a report carries, and the release answer the
//! updater reads. Both sides build against these types, so a field renamed on one side fails the
//! other side's build instead of a player's report.
//!
//! Reports: the app's report window and the site's form send one JSON [`Report`] to
//! [`REPORTS_PATH`]. The service answers [`ReportAccepted`] (200) or [`ReportRejected`] with 400
//! (invalid), 413 (too large), 429 (too many, `Retry-After` in seconds), 408 (the body came too
//! slowly) or 503 (busy with other reports, `Retry-After` in seconds, or it couldn't pass the
//! report on). [`Report::check`] holds the rules, and both sides run it.
//!
//! Updates: [`LATEST_RELEASE_PATH`] answers a [`Release`] -- the part of GitHub's release JSON the
//! updater reads -- whose assets are downloaded from the service under [`DOWNLOAD_PATH`].
//! [`SUMS_ASSET`] lists the installer's SHA-256, and [`SUMS_SIGNATURE_ASSET`] is the standard
//! base64 of an Ed25519 signature over that file's exact bytes, made by the release pipeline's key.
//! The updater checks it against the public key it carries, so neither the service nor anyone on
//! the way can hand out an installer the release pipeline didn't sign.
//!
//! Live updates: the app stays connected to [`EVENTS_PATH`], a Server-Sent Events stream whose
//! [`VERSIONS_EVENT`] events carry the latest published [`Versions`] -- the app release and the
//! data pack -- once on connecting and again whenever either changes. Two channels follow from it:
//! a new app version is the release above, installed by its signed installer; a new data pack
//! ([`LATEST_DATA_PATH`], [`DataManifest`]) replaces the game tables the app carries without a new
//! exe, signed the same way.

use serde::{Deserialize, Serialize};

/// The production service.
pub const PRODUCTION_BASE: &str = "https://oracle.pushka.biz";

/// The service this build talks to: `POE2_ORACLE_API_BASE` at build time, else production. It must
/// be https -- a report carries the player's logs, and the release answer names the installer the
/// app runs -- unless the `dev-endpoints` feature lets a test build use a plain-http service. No
/// trailing slash: the paths below start with one.
pub const API_BASE: &str = match option_env!("POE2_ORACLE_API_BASE") {
    Some(base) => base,
    None => PRODUCTION_BASE,
};

const _: () = assert!(
    base_allowed(API_BASE, cfg!(feature = "dev-endpoints")),
    "POE2_ORACLE_API_BASE must be https:// with no trailing slash; plain http needs the \
     dev-endpoints feature"
);

/// Where reports go.
pub const REPORTS_PATH: &str = "/api/v1/reports";
/// Where the updater asks for the latest release.
pub const LATEST_RELEASE_PATH: &str = "/api/v1/releases/latest";
/// Release downloads: `/download/<tag>/<asset name>`, and `/download/latest` for the current
/// installer, which the site's download button links.
pub const DOWNLOAD_PATH: &str = "/download";

/// `path` on this build's service.
pub fn url(path: &str) -> String {
    format!("{API_BASE}{path}")
}

/// Whether `base` can be the service's address: https, or http when `plain_http`; never ending in
/// a slash.
const fn base_allowed(base: &str, plain_http: bool) -> bool {
    let bytes = base.as_bytes();
    if bytes.is_empty() || bytes[bytes.len() - 1] == b'/' {
        return false;
    }
    starts_with(bytes, b"https://") || (plain_http && starts_with(bytes, b"http://"))
}

const fn starts_with(text: &[u8], prefix: &[u8]) -> bool {
    if text.len() < prefix.len() {
        return false;
    }
    let mut at = 0;
    while at < prefix.len() {
        if text[at] != prefix[at] {
            return false;
        }
        at += 1;
    }
    true
}

// --- Reports ------------------------------------------------------------------------------------

/// The longest text a player can write, in characters.
pub const MAX_TEXT_CHARS: usize = 8000;
/// The longest contact, in characters.
pub const MAX_CONTACT_CHARS: usize = 200;
/// The longest item name, in characters.
pub const MAX_ITEM_NAME_CHARS: usize = 200;
/// The largest item text, in bytes: the longest real item texts are a few kilobytes.
pub const MAX_ITEM_TEXT_BYTES: usize = 32 * 1024;
/// The largest crash text (panic message and backtrace), in bytes.
pub const MAX_CRASH_BYTES: usize = 64 * 1024;
/// The largest diagnostics zip, in bytes.
pub const MAX_DIAGNOSTICS_BYTES: usize = 8 * 1024 * 1024;
/// The longest value of the app's context ([`AppContext`]: its version, the languages, the
/// Windows name and the league), in characters.
pub const MAX_CONTEXT_CHARS: usize = 200;
/// The largest request body the service reads: everything above, with base64 and JSON around it.
pub const MAX_BODY_BYTES: usize = 12 * 1024 * 1024;

/// What a report is about.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    /// Something doesn't work.
    Bug,
    /// A suggestion.
    Idea,
    /// An item the app misread or mispriced: [`Report::item`] carries it.
    Item,
    /// The app closed on a panic last time: [`Report::crash`] carries what the panic said.
    Crash,
}

/// Where a report was written.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportSource {
    /// The app's report window.
    App,
    /// The site's form.
    Site,
}

/// One report, as the app or the site sends it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Report {
    pub kind: ReportKind,
    pub source: ReportSource,
    /// What the player wrote. May be empty only for a crash.
    pub text: String,
    /// How to answer the player, if they want an answer: a Telegram or Discord name, an email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    /// The app that sent it; `None` from the site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<AppContext>,
    /// The item an item report is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<ReportItem>,
    /// What the panic said, backtrace included, for a crash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crash: Option<String>,
    /// The diagnostics zip the app collects (logs, settings, unread item texts, a system
    /// summary, the player's folders and Windows name masked), when the player attached it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "base64_bytes"
    )]
    pub diagnostics: Option<Vec<u8>>,
    /// The site form's honeypot, a field people never see: anything in it marks a bot.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub website: String,
}

/// What the app sending a report runs as.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AppContext {
    /// The app's version, `0.1.0`.
    pub version: String,
    /// The interface language in effect: `ru` or `en`.
    pub interface_language: String,
    /// The game client's language when known: `ru` or `en`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_language: Option<String>,
    /// Windows as the registry names it: `Windows 11 Pro 24H2 (26100.4061)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<String>,
    /// The league searches go to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub league: Option<String>,
    /// The interface scale, 0.8 to 1.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_scale: Option<f32>,
}

/// The item an item report is about.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ReportItem {
    /// Its name, as the price panel shows it.
    pub name: String,
    /// Its text, as the game copied it.
    pub text: String,
}

/// The first rule a report breaks ([`Report::check`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// Nothing written, and it isn't a crash.
    EmptyText,
    TextTooLong,
    ContactTooLong,
    /// An item report without its item.
    ItemMissing,
    ItemTooLarge,
    /// A crash report without what the panic said.
    CrashMissing,
    CrashTooLarge,
    DiagnosticsTooLarge,
    /// A value of the app's context over [`MAX_CONTEXT_CHARS`].
    ContextTooLong,
    /// A report from the app without the app's context.
    AppMissing,
    /// A report from the site carrying what only the app sends (its context, an item, a crash,
    /// diagnostics), or of a kind only the app sends.
    NotFromTheApp,
    /// The honeypot was filled in: a bot.
    Honeypot,
}

impl Report {
    /// Whether the report may be sent: [`Problem`] names the first rule it breaks.
    pub fn check(&self) -> Result<(), Problem> {
        if !self.website.is_empty() {
            return Err(Problem::Honeypot);
        }
        let text = self.text.trim();
        if text.is_empty() && self.kind != ReportKind::Crash {
            return Err(Problem::EmptyText);
        }
        if text.chars().count() > MAX_TEXT_CHARS {
            return Err(Problem::TextTooLong);
        }
        if self
            .contact
            .as_ref()
            .is_some_and(|contact| contact.trim().chars().count() > MAX_CONTACT_CHARS)
        {
            return Err(Problem::ContactTooLong);
        }
        match (&self.item, self.kind) {
            (None, ReportKind::Item) => return Err(Problem::ItemMissing),
            (Some(item), _)
                if item.name.chars().count() > MAX_ITEM_NAME_CHARS
                    || item.text.len() > MAX_ITEM_TEXT_BYTES =>
            {
                return Err(Problem::ItemTooLarge);
            }
            _ => {}
        }
        match (&self.crash, self.kind) {
            (None, ReportKind::Crash) => return Err(Problem::CrashMissing),
            (Some(crash), _) if crash.len() > MAX_CRASH_BYTES => {
                return Err(Problem::CrashTooLarge);
            }
            _ => {}
        }
        if self
            .diagnostics
            .as_ref()
            .is_some_and(|zip| zip.len() > MAX_DIAGNOSTICS_BYTES)
        {
            return Err(Problem::DiagnosticsTooLarge);
        }
        if self.app.as_ref().is_some_and(AppContext::too_long) {
            return Err(Problem::ContextTooLong);
        }
        match self.source {
            ReportSource::App if self.app.is_none() => Err(Problem::AppMissing),
            ReportSource::Site
                if self.app.is_some()
                    || self.item.is_some()
                    || self.crash.is_some()
                    || self.diagnostics.is_some()
                    || !matches!(self.kind, ReportKind::Bug | ReportKind::Idea) =>
            {
                Err(Problem::NotFromTheApp)
            }
            ReportSource::App | ReportSource::Site => Ok(()),
        }
    }
}

impl AppContext {
    /// Whether one of its strings is over [`MAX_CONTEXT_CHARS`]. Counted only that far: an
    /// oversized value can be megabytes.
    fn too_long(&self) -> bool {
        [
            Some(&self.version),
            Some(&self.interface_language),
            self.client_language.as_ref(),
            self.windows.as_ref(),
            self.league.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| value.chars().nth(MAX_CONTEXT_CHARS).is_some())
    }
}

/// The service took the report.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportAccepted {
    /// The report's number: its GitHub issue's, or 0 when only the Telegram message went out.
    pub id: u64,
}

/// The service refused the report, or couldn't pass it on.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ReportRejected {
    pub error: RejectReason,
    /// The service's own words, for the log; the app words the reason itself.
    pub message: String,
}

/// Why a report was refused.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// It breaks a rule of [`Report::check`] or isn't a report at all: 400.
    Invalid,
    /// Its body is over [`MAX_BODY_BYTES`]: 413.
    TooLarge,
    /// Too many reports from this address lately: 429, with `Retry-After`.
    RateLimited,
    /// Not taken now, and worth trying again later: the service was busy reading other reports
    /// (503, with `Retry-After`), the body came too slowly (408), or neither GitHub nor Telegram
    /// took it (503).
    Unavailable,
}

// --- Releases -----------------------------------------------------------------------------------

/// The latest release, as [`LATEST_RELEASE_PATH`] answers it: the fields of GitHub's release JSON
/// the updater reads.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// `vX.Y.Z`.
    pub tag_name: String,
    /// The release's page.
    pub html_url: String,
    pub assets: Vec<ReleaseAsset>,
}

/// One file of a release.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    /// Its exact size in bytes.
    pub size: u64,
}

/// The release's checksum list, in `sha256sum` format.
pub const SUMS_ASSET: &str = "SHA256SUMS";
/// The Ed25519 signature of [`SUMS_ASSET`]'s exact bytes, standard base64.
pub const SUMS_SIGNATURE_ASSET: &str = "SHA256SUMS.sig";

/// The installer's asset name for `version` (`0.1.0`, no `v`).
pub fn installer_asset(version: &str) -> String {
    format!("PoE2-Oracle-Setup-{version}.exe")
}

// --- Live updates -------------------------------------------------------------------------------

/// The service's event stream: `text/event-stream`, no authentication. The first
/// [`VERSIONS_EVENT`] event comes right after connecting and the next whenever a version changes;
/// in between, a comment line (`: ping`) every [`EVENTS_PING_SECS`] seconds keeps proxies from
/// closing the idle connection and tells the app it is still alive.
pub const EVENTS_PATH: &str = "/api/v1/events";
/// The name of the event whose data is a [`Versions`] JSON.
pub const VERSIONS_EVENT: &str = "versions";
/// How often the service sends a keep-alive comment on the event stream.
pub const EVENTS_PING_SECS: u64 = 25;

/// The latest published versions, as the event stream announces them.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Versions {
    /// The latest app release's version (`0.1.1`, no `v`); `None` while none is published.
    pub app: Option<String>,
    /// The latest data pack's version; `None` while none is published.
    pub data: Option<DataVersion>,
}

// --- Data packs ---------------------------------------------------------------------------------

/// Where the updater asks for the latest data pack: a [`Release`], as [`LATEST_RELEASE_PATH`]
/// answers one, for the newest published release tagged [`DATA_TAG_PREFIX`]`<version>`. Its assets
/// are the pack ([`data_pack_asset`]), [`SUMS_ASSET`] and [`SUMS_SIGNATURE_ASSET`], downloaded from
/// the service under [`DOWNLOAD_PATH`] and signed by the same key as app releases.
pub const LATEST_DATA_PATH: &str = "/api/v1/data/latest";
/// Data pack releases are tagged `data-<version>`; app releases `v<semver>`.
pub const DATA_TAG_PREFIX: &str = "data-";
/// A data pack's version: `YYYYMMDDNN`, the day the pack was made and that day's number, so a
/// later pack always compares greater.
pub type DataVersion = u64;
/// The pack layout this build reads ([`DataManifest::format`]).
pub const DATA_FORMAT: u32 = 1;
/// The manifest's name inside a data pack zip.
pub const DATA_MANIFEST: &str = "manifest.json";
/// The tables a data pack carries, by file name: the game data the app otherwise has built in.
pub const DATA_FILES: [&str; 5] = [
    "stat-matchers-en.tsv",
    "stat-matchers-ru.tsv",
    "mod-tiers.tsv",
    "cx-items.tsv",
    "item-refs.tsv",
];

/// The data pack's asset name for `version`.
pub fn data_pack_asset(version: DataVersion) -> String {
    format!("PoE2-Oracle-Data-{version}.zip")
}

/// What a data pack zip says about itself, in [`DATA_MANIFEST`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DataManifest {
    /// [`DATA_FORMAT`] when the pack was made; an app reads only its own format.
    pub format: u32,
    pub version: DataVersion,
    /// The oldest app version that can use the pack (semver, no `v`).
    pub min_app: String,
    /// Every table in the pack by file name ([`DATA_FILES`]), with its SHA-256 in lowercase hex.
    pub files: std::collections::BTreeMap<String, String>,
}

/// The diagnostics zip as standard base64 in the JSON. Read, it's decoded straight from the text
/// the deserializer has -- for the service, the request's body -- rather than from a copy of its
/// megabytes.
mod base64_bytes {
    use std::fmt;

    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::Serializer;
    use serde::de::{self, Deserializer, Visitor};

    pub fn serialize<S: Serializer>(
        bytes: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(bytes) => serializer.serialize_str(&STANDARD.encode(bytes)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        deserializer.deserialize_option(Zip)
    }

    /// The zip, if there is one, and then its text: borrowed from the input when the input
    /// allows, as `visit_str` sees either way.
    struct Zip;

    impl<'de> Visitor<'de> for Zip {
        type Value = Option<Vec<u8>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a zip in standard base64")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_str(Zip)
        }

        fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
            STANDARD.decode(text).map(Some).map_err(E::custom)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_bug(text: &str) -> Report {
        Report {
            kind: ReportKind::Bug,
            source: ReportSource::App,
            text: text.to_owned(),
            contact: None,
            app: Some(AppContext {
                version: "0.1.0".to_owned(),
                interface_language: "ru".to_owned(),
                client_language: Some("ru".to_owned()),
                windows: None,
                league: Some("Forbidden Rites".to_owned()),
                ui_scale: Some(0.9),
            }),
            item: None,
            crash: None,
            diagnostics: None,
            website: String::new(),
        }
    }

    fn site(kind: ReportKind) -> Report {
        Report {
            kind,
            source: ReportSource::Site,
            app: None,
            ..app_bug("Не открывается панель")
        }
    }

    /// The site's form writes this JSON by hand: its field names and values are the contract.
    #[test]
    fn a_report_travels_as_the_documented_json() {
        let mut report = app_bug("Панель не открылась");
        report.diagnostics = Some(vec![0x50, 0x4b, 0x03, 0x04]);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["kind"], "bug");
        assert_eq!(json["source"], "app");
        assert_eq!(json["app"]["interface_language"], "ru");
        assert_eq!(json["diagnostics"], "UEsDBA==");
        assert!(json.get("contact").is_none() && json.get("website").is_none());
        assert_eq!(
            serde_json::from_value::<Report>(json).unwrap(),
            report,
            "and reads back the same"
        );
        // As the service reads it: from the body's bytes, the zip decoded from them in place.
        let body = serde_json::to_vec(&report).unwrap();
        assert_eq!(serde_json::from_slice::<Report>(&body).unwrap(), report);
        let unattached: Report =
            serde_json::from_str(r#"{"kind":"bug","source":"app","text":"x","diagnostics":null}"#)
                .unwrap();
        assert_eq!(unattached.diagnostics, None);

        let form: Report = serde_json::from_str(
            r#"{"kind":"idea","source":"site","text":"Тёмная тема","contact":"@me"}"#,
        )
        .unwrap();
        assert_eq!(form.check(), Ok(()));
    }

    #[test]
    fn text_is_required_up_to_its_limit_except_for_a_crash() {
        assert_eq!(app_bug("  \n ").check(), Err(Problem::EmptyText));
        assert_eq!(app_bug(&"я".repeat(MAX_TEXT_CHARS)).check(), Ok(()));
        assert_eq!(
            app_bug(&"я".repeat(MAX_TEXT_CHARS + 1)).check(),
            Err(Problem::TextTooLong)
        );

        let mut crash = app_bug("");
        crash.kind = ReportKind::Crash;
        assert_eq!(crash.check(), Err(Problem::CrashMissing));
        crash.crash = Some("panicked at src/app.rs:1:1".to_owned());
        assert_eq!(crash.check(), Ok(()));
        crash.crash = Some("x".repeat(MAX_CRASH_BYTES + 1));
        assert_eq!(crash.check(), Err(Problem::CrashTooLarge));
    }

    #[test]
    fn attachments_stay_within_their_limits() {
        let mut item = app_bug("Цена не та");
        item.kind = ReportKind::Item;
        assert_eq!(item.check(), Err(Problem::ItemMissing));
        item.item = Some(ReportItem {
            name: "Крутящий ободок".to_owned(),
            text: "Класс предмета: Кольца".to_owned(),
        });
        assert_eq!(item.check(), Ok(()));
        item.item.as_mut().unwrap().text = "x".repeat(MAX_ITEM_TEXT_BYTES + 1);
        assert_eq!(item.check(), Err(Problem::ItemTooLarge));

        let mut bug = app_bug("Вылетает");
        bug.diagnostics = Some(vec![0; MAX_DIAGNOSTICS_BYTES]);
        assert_eq!(bug.check(), Ok(()));
        bug.diagnostics = Some(vec![0; MAX_DIAGNOSTICS_BYTES + 1]);
        assert_eq!(bug.check(), Err(Problem::DiagnosticsTooLarge));

        bug.diagnostics = None;
        bug.contact = Some("x".repeat(MAX_CONTACT_CHARS + 1));
        assert_eq!(bug.check(), Err(Problem::ContactTooLong));
    }

    #[test]
    fn every_context_value_is_bounded_in_characters() {
        let fields: [fn(&mut AppContext, String); 5] = [
            |app, value| app.version = value,
            |app, value| app.interface_language = value,
            |app, value| app.client_language = Some(value),
            |app, value| app.windows = Some(value),
            |app, value| app.league = Some(value),
        ];
        for (index, set) in fields.into_iter().enumerate() {
            let mut report = app_bug("Вылетает");
            // Two bytes a character: the limit counts characters.
            set(report.app.as_mut().unwrap(), "я".repeat(MAX_CONTEXT_CHARS));
            assert_eq!(report.check(), Ok(()), "field {index}");
            set(
                report.app.as_mut().unwrap(),
                "я".repeat(MAX_CONTEXT_CHARS + 1),
            );
            assert_eq!(
                report.check(),
                Err(Problem::ContextTooLong),
                "field {index}"
            );
        }
    }

    #[test]
    fn the_site_sends_only_what_a_person_types() {
        assert_eq!(site(ReportKind::Bug).check(), Ok(()));
        assert_eq!(site(ReportKind::Idea).check(), Ok(()));
        assert_eq!(site(ReportKind::Crash).check(), Err(Problem::CrashMissing));
        let mut with_zip = site(ReportKind::Bug);
        with_zip.diagnostics = Some(vec![1]);
        assert_eq!(with_zip.check(), Err(Problem::NotFromTheApp));
        let mut bot = site(ReportKind::Idea);
        bot.website = "http://spam".to_owned();
        assert_eq!(bot.check(), Err(Problem::Honeypot));

        let mut anonymous = app_bug("Нет контекста");
        anonymous.app = None;
        assert_eq!(anonymous.check(), Err(Problem::AppMissing));
    }

    #[test]
    fn the_service_is_https_unless_a_test_build_allows_http() {
        assert!(base_allowed("https://oracle.pushka.biz", false));
        assert!(!base_allowed("https://oracle.pushka.biz/", false));
        assert!(!base_allowed(
            "http://poe2-oracle-main.lanes.internal",
            false
        ));
        assert!(base_allowed("http://poe2-oracle-main.lanes.internal", true));
        assert!(!base_allowed("ftp://oracle.pushka.biz", true));
        assert_eq!(installer_asset("0.1.0"), "PoE2-Oracle-Setup-0.1.0.exe");
    }
}
