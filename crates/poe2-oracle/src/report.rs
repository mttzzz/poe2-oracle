//! Reporting to the developer from inside the app. The report window (`ui::report_view`) sends
//! one [`Report`] -- the player's words, how to answer them if they want an answer, what the app
//! runs as, and the item, the crash or the diagnostics it attaches -- to the app's service at
//! oracle.pushka.biz (`oracle_protocol`), which passes it on to the developer. Nothing leaves
//! before the player presses Send, and no account is needed.
//!
//! Here are the rules the window goes by: how its fields make a report ([`Draft::report`]), what
//! other requests and the service's answer do to the report it holds ([`Form`]), how it goes and
//! what the service's answer means ([`send`], [`answer`]), how long a crash the last run left
//! stays worth reporting ([`recent_crash`], [`crash_is_recent`]), and how the diagnostics stay
//! under the service's limit ([`DiagnosticsZip`], [`log_tail`]) -- and the zip a report the
//! service didn't take is kept as ([`unsent_zip`]).
//!
//! What would name the player is masked in everything the app attaches ([`Masker`]): a Windows
//! user name is often a real name, and the report is read by someone else.

use std::io::{Cursor, Write as _};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, Result};
use futures::AsyncReadExt as _;
use http_client::http::header::{CONTENT_TYPE, RETRY_AFTER};
use http_client::{AsyncBody, HttpClient, HttpRequestExt as _, RedirectPolicy};
use item_parser::ItemLanguage;
use oracle_protocol::{
    AppContext, MAX_CONTEXT_CHARS, MAX_CRASH_BYTES, MAX_ITEM_NAME_CHARS, MAX_ITEM_TEXT_BYTES,
    REPORTS_PATH, Report, ReportAccepted, ReportItem, ReportKind, ReportRejected, ReportSource,
};
use reqwest_client::ReqwestClient;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::i18n::Lang;
use crate::paths;
use crate::tr;

// --- The report ---------------------------------------------------------------------------------

/// What the report window opens for (`app::open_report`): the kind it starts on, and the item or
/// the crash it can attach.
#[cfg(target_os = "windows")]
pub struct Request {
    pub kind: ReportKind,
    pub item: Option<ReportItem>,
    pub crash: Option<Crash>,
}

#[cfg(target_os = "windows")]
impl Request {
    /// A problem or an idea: the settings window's «Написать разработчику».
    pub fn general() -> Request {
        Request {
            kind: ReportKind::Bug,
            item: None,
            crash: None,
        }
    }

    /// The item just checked, read or priced wrong: its name as the panel shows it, and its text
    /// as the game copied it.
    pub fn item(name: String, text: String) -> Request {
        Request {
            kind: ReportKind::Item,
            item: Some(ReportItem { name, text }),
            crash: None,
        }
    }

    /// The crash the last run left ([`recent_crash`]).
    pub fn crash(crash: Crash) -> Request {
        Request {
            kind: ReportKind::Crash,
            item: None,
            crash: Some(crash),
        }
    }
}

/// The report window's fields as the player left them ([`Draft::report`]).
pub struct Draft<'a> {
    pub kind: ReportKind,
    pub text: &'a str,
    pub contact: &'a str,
    /// The item the window was opened for, whichever kind is picked.
    pub item: Option<&'a ReportItem>,
    /// What the last run's panic said, when the window was opened for it.
    pub crash: Option<&'a str>,
}

impl Draft<'_> {
    /// The report these fields make, from the app `app` describes, with `diagnostics` when the
    /// player attached them: the text and the contact trimmed, a blank contact left out; the item
    /// only in an item report and the crash only in a crash report, each cut to the service's
    /// limit, so that a giant clipboard can't keep the report from going.
    pub fn report(&self, app: AppContext, diagnostics: Option<Vec<u8>>) -> Report {
        let contact = self.contact.trim();
        let item = self
            .item
            .filter(|_| self.kind == ReportKind::Item)
            .map(|item| ReportItem {
                name: item.name.chars().take(MAX_ITEM_NAME_CHARS).collect(),
                text: cut(&item.text, MAX_ITEM_TEXT_BYTES).to_owned(),
            });
        let crash = self
            .crash
            .filter(|_| self.kind == ReportKind::Crash)
            .map(|crash| cut(crash, MAX_CRASH_BYTES).to_owned());
        Report {
            kind: self.kind,
            source: ReportSource::App,
            text: self.text.trim().to_owned(),
            contact: (!contact.is_empty()).then(|| contact.to_owned()),
            app: Some(app),
            item,
            crash,
            diagnostics,
            website: String::new(),
        }
    }
}

/// The start of `text` that fits in `max` bytes, cut at a character.
fn cut(text: &str, max: usize) -> &str {
    &text[..text.floor_char_boundary(max)]
}

/// What the app runs as, for a report: this version, the interface language, the game client's
/// language as far as the app knows it (`PriceCheckApp::item_language`), Windows, the league
/// searches go to -- none before the trade site has listed its leagues -- and the UI scale. What
/// comes from outside the app, Windows' name and the league's, is cut to the service's limit.
pub fn app_context(
    interface: Lang,
    client: Option<ItemLanguage>,
    windows: Option<String>,
    league: &str,
    ui_scale: f32,
) -> AppContext {
    let client = client.map(|language| match language {
        ItemLanguage::English => "en",
        ItemLanguage::Russian => "ru",
    });
    let fit = |mut value: String| {
        if let Some((end, _)) = value.char_indices().nth(MAX_CONTEXT_CHARS) {
            value.truncate(end);
        }
        value
    };
    AppContext {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        interface_language: interface.code().to_owned(),
        client_language: client.map(str::to_owned),
        windows: windows.map(fit),
        league: (!league.is_empty()).then(|| fit(league.to_owned())),
        ui_scale: Some(ui_scale),
    }
}

// --- Sending ------------------------------------------------------------------------------------

/// How long a report may take to go, its answer included: with the diagnostics a report is up to
/// about 11 MB, which takes a minute and a half to upload at 1 Mbit/s, and the service passes the
/// report on before it answers.
const SEND_TIMEOUT: Duration = Duration::from_secs(180);

/// The most of the service's answer read: `{"id": …}`, or its refusal's reason.
const MAX_ANSWER_BYTES: u64 = 64 * 1024;

/// The wait said when a refusal for too many reports doesn't give one (it should, in
/// `Retry-After`): an hour, to be safe.
const UNSAID_RETRY_MINUTES: u64 = 60;

/// Why the service didn't take a report, as the window words it ([`Failure::reason`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The service couldn't be reached, or stopped answering.
    NoConnection,
    /// It couldn't pass the report on (503), or answered what it never should.
    Unavailable,
    /// Too many reports from the player's address lately (429): this many minutes to wait.
    RateLimited { minutes: u64 },
    /// The report is bigger than the service reads (413).
    TooLarge,
    /// The service found the report invalid (400): this app and the service disagree on the
    /// rules, and sending it again won't change that.
    Refused,
}

impl Failure {
    /// Why, in the interface language, as it follows «Не удалось отправить: ».
    pub fn reason(self) -> String {
        match self {
            Failure::NoConnection => tr!("no connection to oracle.pushka.biz").to_owned(),
            Failure::Unavailable => tr!("the service is unavailable, try later").to_owned(),
            Failure::RateLimited { minutes } => tr!(
                "too many reports from your address, try again in {minutes} min",
                minutes = minutes
            ),
            Failure::TooLarge => tr!("the attachment is too large").to_owned(),
            Failure::Refused => tr!("the service refused the report").to_owned(),
        }
    }
}

/// Sends a report, `body` its JSON, to the service -- following no redirect, carrying no cookie --
/// and reads the answer ([`answer`]). Through a client of its own, whose only limit is
/// [`SEND_TIMEOUT`]: the app's (`cx.http_client()`) has a read timeout (`app::READ_TIMEOUT`) that
/// also runs from a request's start until its answer begins, and a report's upload and the
/// service's passing it on can take longer than that.
pub async fn send(body: Vec<u8>) -> Result<u64, Failure> {
    let client = match ReqwestClient::proxy_and_user_agent(None, crate::brand::USER_AGENT) {
        Ok(client) => client,
        Err(err) => {
            log::warn!("building the client reports go through failed: {err:#}");
            return Err(Failure::NoConnection);
        }
    };
    let request = http_client::Request::post(oracle_protocol::url(REPORTS_PATH))
        .header(CONTENT_TYPE, "application/json")
        .follow_redirects(RedirectPolicy::NoFollow)
        .timeout(SEND_TIMEOUT)
        .body(AsyncBody::from(body))
        .expect("the service's address and the headers are valid");
    let mut response = match client.send(request).await {
        Ok(response) => response,
        Err(err) => {
            log::warn!("sending the report failed: {err:#}");
            return Err(Failure::NoConnection);
        }
    };
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    // Only a 200's says which report it made; a cut-off one still means the report went.
    let mut body = Vec::new();
    if let Err(err) = response
        .body_mut()
        .take(MAX_ANSWER_BYTES)
        .read_to_end(&mut body)
        .await
    {
        log::warn!("reading the service's answer ({status}) failed: {err}");
    }
    let taken = answer(status, retry_after.as_deref(), &body);
    match &taken {
        Ok(id) => log::info!("report sent: #{id}"),
        Err(failure) => {
            let said = serde_json::from_slice::<ReportRejected>(&body).map_or_else(
                |_| String::from_utf8_lossy(&body).into_owned(),
                |rejected| rejected.message,
            );
            log::warn!("the service didn't take the report ({status}, {failure:?}): {said}");
        }
    }
    taken
}

/// What the service's answer -- its `status`, its `Retry-After` in seconds and its `body` -- says:
/// the report's number (0 when it went out without a GitHub issue, or the service's 200 didn't
/// say which), or why it wasn't taken.
pub fn answer(status: u16, retry_after: Option<&str>, body: &[u8]) -> Result<u64, Failure> {
    match status {
        200..=299 => Ok(serde_json::from_slice::<ReportAccepted>(body).map_or(0, |taken| taken.id)),
        400 => Err(Failure::Refused),
        413 => Err(Failure::TooLarge),
        429 => {
            let seconds = retry_after.and_then(|seconds| seconds.trim().parse::<u64>().ok());
            let minutes =
                seconds.map_or(UNSAID_RETRY_MINUTES, |seconds| seconds.div_ceil(60).max(1));
            Err(Failure::RateLimited { minutes })
        }
        _ => Err(Failure::Unavailable),
    }
}

// --- The window's report ------------------------------------------------------------------------

/// Where the report window's report is.
pub enum Stage {
    /// Being written.
    Compose,
    /// On its way: the form stays, out of reach, and so does the window, for the answer.
    Sending,
    /// Taken, as this number (0: taken without one).
    Sent(u64),
    /// Not taken, and why: the report as it went, for Try again and Save to desktop.
    Failed(Arc<Report>, Failure),
}

/// What another request did to the report window's report ([`Form::take`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takeover {
    /// Nothing: a report on its way, or one the service didn't take, keeps the window.
    Refused,
    /// It joined the report being written, whose words stay.
    Joined,
    /// The report before was sent: this is a new one, and the words before go.
    Restarted,
}

/// The report window's report apart from its words, which the window's text box and contact field
/// hold (`ui::report_view`): where it is, its kind, the item and the crash it can attach -- `C`,
/// the crash as the window keeps it -- and the diagnostics toggle; and what other requests and the
/// service's answer do to them.
pub struct Form<C> {
    pub stage: Stage,
    pub kind: ReportKind,
    /// The item the window can attach: the last one an item report was asked for.
    pub item: Option<ReportItem>,
    /// The crash the window was opened for at launch, until the window is done with it: the
    /// report went, or the player closed the window ([`forget_crash`]).
    pub crash: Option<C>,
    /// The diagnostics toggle as the player set it; `None` follows the kind.
    diagnostics: Option<bool>,
}

impl<C> Form<C> {
    /// A report of `kind` to be written, with the item and the crash it can attach.
    pub fn new(kind: ReportKind, item: Option<ReportItem>, crash: Option<C>) -> Form<C> {
        Form {
            stage: Stage::Compose,
            kind,
            item,
            crash,
            diagnostics: None,
        }
    }

    /// Takes another request -- its item and its crash; the window picks its kind -- over
    /// (`app::open_report`). While a report is being written, the request's item replaces the one
    /// before, and what the player wrote and chose stays, the diagnostics toggle included; after a
    /// report was sent, a new one starts. A report on its way, or one the service didn't take,
    /// keeps the window to it.
    pub fn take(&mut self, item: Option<ReportItem>, crash: Option<C>) -> Takeover {
        let takeover = match self.stage {
            Stage::Compose => {
                if item.is_some() {
                    self.item = item;
                }
                Takeover::Joined
            }
            Stage::Sent(_) => {
                self.stage = Stage::Compose;
                self.item = item;
                self.diagnostics = None;
                Takeover::Restarted
            }
            Stage::Sending | Stage::Failed(..) => return Takeover::Refused,
        };
        if crash.is_some() {
            self.crash = crash;
        }
        takeover
    }

    /// The kinds the window offers: an item report only with an item to attach, a crash report
    /// only for a crash.
    pub fn kinds(&self) -> Vec<ReportKind> {
        let mut kinds = vec![ReportKind::Bug, ReportKind::Idea];
        if self.item.is_some() {
            kinds.push(ReportKind::Item);
        }
        if self.crash.is_some() {
            kinds.push(ReportKind::Crash);
        }
        kinds
    }

    /// Whether the diagnostics go with the report: as the player set the toggle, or by the kind --
    /// all but an idea's, which is about what the app doesn't do yet.
    pub fn attaches_diagnostics(&self) -> bool {
        self.diagnostics.unwrap_or(self.kind != ReportKind::Idea)
    }

    /// The player flipped the diagnostics toggle.
    pub fn toggle_diagnostics(&mut self) {
        self.diagnostics = Some(!self.attaches_diagnostics());
    }

    /// Whether the window may close: not while its report is on its way. The answer shows in the
    /// window -- a failure with Try again and Save to desktop -- and the crash stays until then.
    pub fn closable(&self) -> bool {
        !matches!(self.stage, Stage::Sending)
    }

    /// The service's `answer` to `sent`. Taken: the crash the window was opened for is done with,
    /// and returned for the window to forget ([`forget_crash`]). Not taken: the report stays, for
    /// Try again and Save to desktop.
    pub fn answered(&mut self, sent: Arc<Report>, answer: Result<u64, Failure>) -> Option<C> {
        match answer {
            Ok(id) => {
                self.stage = Stage::Sent(id);
                self.crash.take()
            }
            Err(failure) => {
                self.stage = Stage::Failed(sent, failure);
                None
            }
        }
    }
}

// --- Crashes ------------------------------------------------------------------------------------

/// How long a crash stays worth reporting: a launch within it offers the report, and an older
/// crash is just deleted.
const CRASH_KEPT: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// What the last run's panic left (`logging` writes it to `paths::crash_file`): what it said, and
/// when, on the player's clock.
#[cfg(target_os = "windows")]
pub struct Crash {
    pub text: String,
    pub at: windows::Win32::Foundation::SYSTEMTIME,
}

/// The crash the last run left, when it's under a week old ([`crash_is_recent`]); an older one
/// is deleted. It stays until the report window is done with it ([`forget_crash`]): a launch that
/// quits before then offers it again.
#[cfg(target_os = "windows")]
pub fn recent_crash() -> Option<Crash> {
    use std::os::windows::fs::MetadataExt as _;
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

    let path = paths::crash_file();
    let meta = std::fs::metadata(&path).ok()?;
    if !meta
        .modified()
        .is_ok_and(|written| crash_is_recent(written, SystemTime::now()))
    {
        log::info!("the last crash is over a week old: not offered for a report");
        forget_crash();
        return None;
    }
    let text = match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(err) => {
            log::warn!("reading the last crash failed: {err}");
            return None;
        }
    };
    // Its time on the player's clock, by the time zone rules of that date: FILETIME counts
    // 100 ns ticks since 1601-01-01 UTC.
    let ticks = meta.last_write_time();
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let (mut utc, mut at) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    // Neither fails for a file's own time; should the zone rules, UTC it is.
    unsafe {
        let _ = FileTimeToSystemTime(&file_time, &mut utc);
        if SystemTimeToTzSpecificLocalTime(None, &utc, &mut at).is_err() {
            at = utc;
        }
    }
    log::info!("the last run crashed: offering a report");
    Some(Crash { text, at })
}

/// Whether a crash written at `written` is still worth a report at `now`: under a week old. One
/// dated after `now` -- the clock was set back since -- counts as new.
pub fn crash_is_recent(written: SystemTime, now: SystemTime) -> bool {
    now.duration_since(written)
        .map_or(true, |age| age < CRASH_KEPT)
}

/// Deletes the crash the last run left: reported, or the player closed its report window.
pub fn forget_crash() {
    match std::fs::remove_file(paths::crash_file()) {
        Ok(()) => log::info!("the last crash is forgotten"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log::warn!("deleting the last crash failed: {err}"),
    }
}

// --- Zips ---------------------------------------------------------------------------------------

/// The most of each log the diagnostics carry: its newest end, where what the player reports
/// happened. Both logs at this size leave half the zip's room for the rest.
pub const LOG_TAIL_BYTES: usize = 2 * 1024 * 1024;

/// A zip's end record, with no comment.
const END_RECORD: usize = 22;
/// A file's headers in a zip: its local header and its central directory entry, 30 and 46 bytes,
/// each followed by its name; with room to spare for extra fields.
const FILE_HEADERS: usize = 30 + 46 + 64;

/// The newest end of a log, `bytes`: at most `cap` bytes, starting at a line -- or, when that
/// end holds no line break, the last `cap` bytes as they are.
pub fn log_tail(bytes: &[u8], cap: usize) -> &[u8] {
    if bytes.len() <= cap {
        return bytes;
    }
    let end = &bytes[bytes.len() - cap..];
    match end.iter().position(|&byte| byte == b'\n') {
        Some(newline) => &end[newline + 1..],
        None => end,
    }
}

/// The diagnostics zip, built in memory under a size limit whatever its files compress to: a
/// file goes in only when it fits even if it doesn't compress at all.
pub struct DiagnosticsZip {
    zip: ZipWriter<Cursor<Vec<u8>>>,
    /// What's left of the limit for more files.
    room: usize,
}

impl DiagnosticsZip {
    pub fn new(limit: usize) -> DiagnosticsZip {
        DiagnosticsZip {
            zip: ZipWriter::new(Cursor::new(Vec::new())),
            room: limit.saturating_sub(END_RECORD),
        }
    }

    /// Adds `bytes`, deflated, as the file `name` when they fit in what's left of the limit;
    /// whether they went in.
    pub fn add(&mut self, name: &str, bytes: &[u8]) -> Result<bool> {
        let most = most_zipped(name, bytes.len());
        if most > self.room {
            return Ok(false);
        }
        self.room -= most;
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        self.zip
            .start_file(name, options)
            .with_context(|| format!("adding {name} to the diagnostics"))?;
        self.zip
            .write_all(bytes)
            .with_context(|| format!("writing {name} into the diagnostics"))?;
        Ok(true)
    }

    /// The finished zip's bytes.
    pub fn finish(self) -> Result<Vec<u8>> {
        let zip = self.zip.finish().context("finishing the diagnostics zip")?;
        Ok(zip.into_inner())
    }
}

/// The most `len` bytes named `name` can take in a zip. Deflate keeps data that doesn't compress
/// as it is, in blocks with a few bytes of header each: zlib's own bound adds under a byte per
/// kilobyte, and this adds a byte per kilobyte and a few more.
fn most_zipped(name: &str, len: usize) -> usize {
    FILE_HEADERS + 2 * name.len() + len + len / 1024 + 64
}

/// A report the service didn't take, as one zip for the player to keep or pass on another way:
/// `report.txt` -- the kind, the player's words and contact, and what the app runs as -- the
/// item's or the crash's text, and the diagnostics zip as it was attached.
pub fn unsent_zip(report: &Report) -> Result<Vec<u8>> {
    use std::fmt::Write as _;

    let mut text = String::from("PoE2 Oracle report, not sent\n\n");
    let _ = writeln!(text, "kind: {:?}", report.kind);
    if let Some(contact) = &report.contact {
        let _ = writeln!(text, "contact: {contact}");
    }
    if let Some(app) = &report.app {
        let _ = writeln!(text, "version: {}", app.version);
        let _ = writeln!(text, "interface language: {}", app.interface_language);
        let known = [
            ("client language", app.client_language.clone()),
            ("windows", app.windows.clone()),
            ("league", app.league.clone()),
            ("UI scale", app.ui_scale.map(|scale| scale.to_string())),
        ];
        for (name, value) in known {
            if let Some(value) = value {
                let _ = writeln!(text, "{name}: {value}");
            }
        }
    }
    let _ = write!(text, "\n{}\n", report.text);

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut files = vec![("report.txt", text.as_bytes())];
    if let Some(item) = &report.item {
        files.push(("item.txt", item.text.as_bytes()));
    }
    if let Some(crash) = &report.crash {
        files.push(("crash.txt", crash.as_bytes()));
    }
    for (name, bytes) in files {
        zip.start_file(name, deflated)?;
        zip.write_all(bytes)?;
    }
    if let Some(diagnostics) = &report.diagnostics {
        // A zip already: stored as it is.
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("diagnostics.zip", stored)?;
        zip.write_all(diagnostics)?;
    }
    Ok(zip
        .finish()
        .context("finishing the report zip")?
        .into_inner())
}

// --- Masking ------------------------------------------------------------------------------------

/// Either slash: Windows takes both in a path.
const SEPARATORS: [char; 2] = ['\\', '/'];

/// A user name shorter than this is part of too many other words to mask on its own.
const MIN_MASKED_NAME: usize = 3;

/// Replaces what would name the player in a text: their user folder with `%USERPROFILE%`; their
/// Desktop, Documents and AppData folders -- wherever Windows keeps them, another drive or a
/// OneDrive folder named after an employer -- with `%DESKTOP%`, `%DOCUMENTS%`, `%APPDATA%` and
/// `%LOCALAPPDATA%`; and their user name, wherever else it stands, with `%USERNAME%`. In any
/// letter case and with either slash, as Windows reads a path. A match counts only as a whole
/// word: the user name "anna" leaves "Hanna" alone.
pub(crate) struct Masker {
    /// Each text with its stand-in, longest first: at one place, the most specific one masks.
    masks: Vec<(String, &'static str)>,
}

impl Masker {
    /// This Windows user's own folders and names.
    #[cfg(target_os = "windows")]
    pub(crate) fn for_this_user() -> Masker {
        use std::path::{Path, PathBuf};

        let base = directories::BaseDirs::new();
        let user = directories::UserDirs::new();
        let profile = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| Some(base.as_ref()?.home_dir().to_path_buf()));
        let text = |folder: Option<&Path>| Some(folder?.to_string_lossy().into_owned());
        let folders = [
            (text(profile.as_deref()), "%USERPROFILE%"),
            (
                text(base.as_ref().map(|dirs| dirs.home_dir())),
                "%USERPROFILE%",
            ),
            (text(base.as_ref().map(|dirs| dirs.data_dir())), "%APPDATA%"),
            (
                text(base.as_ref().map(|dirs| dirs.data_local_dir())),
                "%LOCALAPPDATA%",
            ),
            (
                text(user.as_ref().and_then(|dirs| dirs.desktop_dir())),
                "%DESKTOP%",
            ),
            (
                text(user.as_ref().and_then(|dirs| dirs.document_dir())),
                "%DOCUMENTS%",
            ),
        ];
        // The user folder's own name too: a renamed account keeps its old folder.
        let names = [
            std::env::var("USERNAME").ok(),
            profile
                .as_deref()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned()),
        ];
        Masker::new(
            folders
                .into_iter()
                .filter_map(|(folder, stand_in)| Some((folder?, stand_in))),
            names.into_iter().flatten(),
        )
    }

    fn new<F: AsRef<str>, N: AsRef<str>>(
        folders: impl IntoIterator<Item = (F, &'static str)>,
        names: impl IntoIterator<Item = N>,
    ) -> Masker {
        let folders = folders.into_iter().filter_map(|(folder, stand_in)| {
            let folder = folder.as_ref().trim_end_matches(SEPARATORS);
            // A drive's root is in every path on that drive.
            folder
                .contains(SEPARATORS)
                .then(|| (folder.to_owned(), stand_in))
        });
        let names = names.into_iter().filter_map(|name| {
            let name = name.as_ref();
            (name.chars().count() >= MIN_MASKED_NAME).then(|| (name.to_owned(), "%USERNAME%"))
        });
        let mut masks: Vec<_> = folders.chain(names).collect();
        masks.sort_by_key(|(text, _)| std::cmp::Reverse(text.chars().count()));
        Masker { masks }
    }

    pub(crate) fn mask(&self, text: &str) -> String {
        let mut masked = String::with_capacity(text.len());
        let mut rest = text;
        // Whether `rest` follows a letter or a digit, where no match may start.
        let mut in_word = false;
        'text: while let Some(c) = rest.chars().next() {
            if !in_word {
                for (mask, stand_in) in &self.masks {
                    if let Some(len) = match_len(rest, mask)
                        && !rest[len..].starts_with(char::is_alphanumeric)
                    {
                        masked.push_str(stand_in);
                        rest = &rest[len..];
                        continue 'text;
                    }
                }
            }
            masked.push(c);
            in_word = c.is_alphanumeric();
            rest = &rest[c.len_utf8()..];
        }
        masked
    }
}

/// The length of the start of `text` that reads as `mask`, in any letter case and with either
/// slash for a separator.
fn match_len(text: &str, mask: &str) -> Option<usize> {
    let mut chars = text.char_indices();
    for expected in mask.chars() {
        let (_, actual) = chars.next()?;
        let same = actual == expected
            || (SEPARATORS.contains(&actual) && SEPARATORS.contains(&expected))
            || actual.to_lowercase().eq(expected.to_lowercase());
        if !same {
            return None;
        }
    }
    Some(chars.next().map_or(text.len(), |(index, _)| index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oracle_protocol::{MAX_TEXT_CHARS, Problem};

    fn context() -> AppContext {
        app_context(
            Lang::Russian,
            Some(ItemLanguage::English),
            Some("Windows 10 Pro 24H2 (build 26100.4061)".to_owned()),
            "Rise of the Abyssal",
            1.25,
        )
    }

    #[test]
    fn the_app_s_context_names_its_languages_and_no_league_before_there_is_one() {
        let context = context();
        assert_eq!(context.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(context.interface_language, "ru");
        assert_eq!(context.client_language.as_deref(), Some("en"));
        assert_eq!(context.league.as_deref(), Some("Rise of the Abyssal"));
        let early = app_context(Lang::English, None, None, "", 1.);
        assert_eq!(early.interface_language, "en");
        assert_eq!((early.client_language, early.league), (None, None));
    }

    #[test]
    fn windows_and_the_league_are_cut_to_what_the_service_takes() {
        // Two bytes a letter: the limit counts characters.
        let long = "я".repeat(MAX_CONTEXT_CHARS + 20);
        let context = app_context(Lang::English, None, Some(long.clone()), &long, 1.);
        let bug = Draft {
            kind: ReportKind::Bug,
            text: "Панель не открылась",
            contact: "",
            item: None,
            crash: None,
        };
        assert_eq!(bug.report(context.clone(), None).check(), Ok(()));
        let cut = "я".repeat(MAX_CONTEXT_CHARS);
        assert_eq!(context.windows.as_ref(), Some(&cut));
        assert_eq!(context.league.as_ref(), Some(&cut));
    }

    /// A report as it went, for [`Form::answered`].
    fn sent() -> Arc<Report> {
        let bug = Draft {
            kind: ReportKind::Bug,
            text: "Панель не открылась",
            contact: "",
            item: None,
            crash: None,
        };
        Arc::new(bug.report(context(), None))
    }

    /// What the price panel asks a report about.
    fn checked_item() -> ReportItem {
        ReportItem {
            name: "Вампирский захват".to_owned(),
            text: "Редкость: Редкий\nВампирский захват\nКольцо с сапфиром\n".to_owned(),
        }
    }

    #[test]
    fn the_diagnostics_choice_stays_with_the_report_it_was_made_for() {
        let mut form = Form::<&str>::new(ReportKind::Bug, None, None);
        form.toggle_diagnostics();
        assert!(!form.attaches_diagnostics());
        // The price panel asks for an item report while the player writes: the window picks its
        // kind, whose diagnostics go by default, and the player's choice still stands.
        assert_eq!(form.take(Some(checked_item()), None), Takeover::Joined);
        form.kind = ReportKind::Item;
        assert!(!form.attaches_diagnostics());
        // Once it's sent, the next request starts a new report, with the kind's default again.
        form.stage = Stage::Sending;
        form.answered(sent(), Ok(7));
        assert_eq!(form.take(None, None), Takeover::Restarted);
        assert!(form.attaches_diagnostics());
    }

    #[test]
    fn a_report_on_its_way_keeps_the_window_and_its_crash_until_the_answer() {
        let crash = "panicked at src/app.rs:1:1";
        let mut form = Form::new(ReportKind::Crash, None, Some(crash));
        form.stage = Stage::Sending;
        assert!(!form.closable());
        assert_eq!(form.take(Some(checked_item()), None), Takeover::Refused);
        assert_eq!(form.item, None);
        // Not taken: the crash stays, for Try again.
        assert_eq!(form.answered(sent(), Err(Failure::NoConnection)), None);
        assert!(form.closable());
        assert_eq!(form.crash, Some(crash));
        // Taken: the crash is done with.
        form.stage = Stage::Sending;
        assert_eq!(form.answered(sent(), Ok(42)), Some(crash));
        assert_eq!(form.crash, None);
        assert!(matches!(form.stage, Stage::Sent(42)));
    }

    #[test]
    fn a_report_carries_the_player_s_words_trimmed_and_the_app_s_context() {
        let draft = Draft {
            kind: ReportKind::Bug,
            text: "\n  Панель не открылась  \n",
            contact: "   ",
            item: None,
            crash: None,
        };
        let report = draft.report(context(), None);
        assert_eq!(report.text, "Панель не открылась");
        assert_eq!(report.contact, None);
        assert_eq!(report.source, ReportSource::App);
        assert_eq!(report.app, Some(context()));
        assert_eq!(report.check(), Ok(()));
        let answerable = Draft {
            contact: " @kiril ",
            ..draft
        }
        .report(context(), None);
        assert_eq!(answerable.contact.as_deref(), Some("@kiril"));
        let blank = Draft {
            text: " \n ",
            ..draft
        };
        assert_eq!(
            blank.report(context(), None).check(),
            Err(Problem::EmptyText)
        );
    }

    #[test]
    fn an_item_or_a_crash_goes_only_with_its_own_kind() {
        let item = ReportItem {
            name: "Вампирский захват".to_owned(),
            text: "Редкость: Редкий\nВампирский захват\nКольцо с сапфиром\n".to_owned(),
        };
        let draft = |kind| Draft {
            kind,
            text: "",
            contact: "",
            item: Some(&item),
            crash: Some("panicked at src/app.rs:1:1"),
        };
        let about_item = draft(ReportKind::Item).report(context(), None);
        assert_eq!(about_item.item.as_ref(), Some(&item));
        assert_eq!(about_item.crash, None);
        let crash = draft(ReportKind::Crash).report(context(), None);
        assert_eq!(crash.item, None);
        assert_eq!(crash.crash.as_deref(), Some("panicked at src/app.rs:1:1"));
        // A crash needs no words; an idea carries neither attachment.
        assert_eq!(crash.check(), Ok(()));
        let idea = draft(ReportKind::Idea).report(context(), None);
        assert_eq!((idea.item, idea.crash), (None, None));
    }

    #[test]
    fn an_attachment_over_the_service_s_limit_is_cut_at_a_character() {
        // Two bytes a letter: a cut in the middle of one would break it.
        let item = ReportItem {
            name: "Я".repeat(MAX_ITEM_NAME_CHARS + 5),
            text: format!("x{}", "Я".repeat(MAX_ITEM_TEXT_BYTES)),
        };
        let crash = format!("x{}", "ё".repeat(MAX_CRASH_BYTES));
        let draft = |kind| Draft {
            kind,
            text: "что-то не так",
            contact: "",
            item: Some(&item),
            crash: Some(&crash),
        };
        let report = draft(ReportKind::Item).report(context(), None);
        let sent = report.item.as_ref().expect("the item");
        assert_eq!(sent.name.chars().count(), MAX_ITEM_NAME_CHARS);
        assert_eq!(sent.text.len(), MAX_ITEM_TEXT_BYTES - 1);
        assert_eq!(report.check(), Ok(()));
        let report = draft(ReportKind::Crash).report(context(), None);
        assert_eq!(
            report.crash.as_ref().map(String::len),
            Some(MAX_CRASH_BYTES - 1)
        );
        assert_eq!(report.check(), Ok(()));
        // The player's own words are the text box's to keep in bounds: it takes no more.
        let long = "a".repeat(MAX_TEXT_CHARS + 1);
        let too_long = Draft {
            text: &long,
            ..draft(ReportKind::Idea)
        };
        assert_eq!(
            too_long.report(context(), None).check(),
            Err(Problem::TextTooLong)
        );
    }

    #[test]
    fn the_service_s_answers_become_the_window_s_reasons() {
        assert_eq!(answer(200, None, br#"{"id":42}"#), Ok(42));
        assert_eq!(answer(200, None, br#"{"id":0}"#), Ok(0));
        // Taken, though it didn't say as what.
        assert_eq!(answer(200, None, b"<html>"), Ok(0));
        assert_eq!(answer(400, None, b"{}"), Err(Failure::Refused));
        assert_eq!(answer(413, None, b""), Err(Failure::TooLarge));
        for (retry_after, minutes) in [
            (Some("120"), 2),
            (Some("30"), 1),
            (Some("0"), 1),
            (Some(" 3601 "), 61),
            (None, UNSAID_RETRY_MINUTES),
            (Some("Wed, 21 Oct 2026 07:28:00 GMT"), UNSAID_RETRY_MINUTES),
        ] {
            assert_eq!(
                answer(429, retry_after, b""),
                Err(Failure::RateLimited { minutes }),
                "{retry_after:?}"
            );
        }
        for status in [503, 500, 502, 404, 301] {
            assert_eq!(
                answer(status, None, b""),
                Err(Failure::Unavailable),
                "{status}"
            );
        }
    }

    #[test]
    fn a_crash_is_offered_for_a_week() {
        let crashed = SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000);
        let day = Duration::from_secs(24 * 60 * 60);
        assert!(crash_is_recent(crashed, crashed));
        assert!(crash_is_recent(
            crashed,
            crashed + 7 * day - Duration::from_secs(1)
        ));
        assert!(!crash_is_recent(crashed, crashed + 7 * day));
        assert!(!crash_is_recent(crashed, crashed + 30 * day));
        // The clock was set back since the crash.
        assert!(crash_is_recent(crashed, crashed - day));
    }

    #[test]
    fn a_long_log_keeps_its_newest_lines_whole() {
        let log: String = (0..1000).map(|n| format!("line {n}\n")).collect();
        let tail = std::str::from_utf8(log_tail(log.as_bytes(), 100)).expect("whole lines");
        assert!(tail.len() <= 100, "{}", tail.len());
        assert!(tail.ends_with("line 999\n"), "{tail}");
        let first = tail.lines().next().expect("a line");
        assert!(log.lines().any(|line| line == first), "{first}");
        // A log within the cap stays whole.
        assert_eq!(log_tail(b"one\ntwo\n", 8), b"one\ntwo\n");
        // With no line break in its end, the end as it is.
        let line = "x".repeat(300);
        assert_eq!(log_tail(line.as_bytes(), 100), &line.as_bytes()[200..]);
    }

    /// `len` bytes that don't compress.
    fn noise(seed: u64, len: usize) -> Vec<u8> {
        let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        (0..len)
            .map(|_| {
                // xorshift64
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn the_diagnostics_zip_stays_under_its_limit_whatever_its_files_compress_to() {
        let limit = 64 * 1024;
        let mut zip = DiagnosticsZip::new(limit);
        let mut added = 0;
        while zip
            .add(&format!("unparsed/{added}.txt"), &noise(added, 7_000))
            .expect("added")
        {
            added += 1;
        }
        // A refused file takes no room: a small one still fits after a big one didn't.
        assert!(zip.add("small.txt", b"Rarity: Rare").expect("added"));
        let bytes = zip.finish().expect("finished");
        assert!(bytes.len() <= limit, "{} > {limit}", bytes.len());
        // The limit goes to files, not to its margin.
        assert!(
            added as usize * 7_000 >= limit * 4 / 5,
            "only {added} files fit"
        );
        let archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip");
        assert_eq!(archive.len(), added as usize + 1);
    }

    /// Kiril's folders: the user folder, AppData in it, the desktop moved to another drive and the
    /// documents in a work OneDrive.
    fn kirils_masker() -> Masker {
        Masker::new(
            [
                (r"C:\Users\Kiril", "%USERPROFILE%"),
                (r"C:\Users\Kiril\AppData\Roaming", "%APPDATA%"),
                (r"C:\Users\Kiril\AppData\Local\", "%LOCALAPPDATA%"),
                (r"D:\Kiril\Desktop", "%DESKTOP%"),
                (
                    r"C:\Users\Kiril\OneDrive - Contoso\Documents",
                    "%DOCUMENTS%",
                ),
            ],
            ["Kiril"],
        )
    }

    #[test]
    fn the_player_s_folders_are_masked_as_windows_reads_a_path() {
        let masker = kirils_masker();
        for (text, masked) in [
            // The most specific folder, whatever the letter case or the slash.
            (
                r"log: C:\Users\Kiril\AppData\Local\poe2-oracle\data\logs",
                r"log: %LOCALAPPDATA%\poe2-oracle\data\logs",
            ),
            (r"c:\users\KIRIL\Saved Games", r"%USERPROFILE%\Saved Games"),
            (
                "C:/Users/kiril/AppData/Roaming/poe2-oracle",
                "%APPDATA%/poe2-oracle",
            ),
            (
                r"written to D:\Kiril\Desktop\PoE2-Oracle-report.zip",
                r"written to %DESKTOP%\PoE2-Oracle-report.zip",
            ),
            (
                r"C:\Users\Kiril\OneDrive - Contoso\Documents\My Games\Path of Exile 2",
                r"%DOCUMENTS%\My Games\Path of Exile 2",
            ),
            // Another user's folder that merely starts the same.
            (r"C:\Users\Kirill\Desktop", r"C:\Users\Kirill\Desktop"),
        ] {
            assert_eq!(masker.mask(text), masked, "{text}");
        }
    }

    #[test]
    fn the_user_name_is_masked_wherever_it_stands_as_a_word() {
        let masker = kirils_masker();
        assert_eq!(
            masker.mask(r"E:\Games\kiril\PoE2 and backup_Kiril.zip"),
            r"E:\Games\%USERNAME%\PoE2 and backup_%USERNAME%.zip"
        );
        assert_eq!(
            masker.mask("Kirill, Kirilov and MrKiril keep their names"),
            "Kirill, Kirilov and MrKiril keep their names"
        );
        // Cyrillic letter case too: a Russian player's folder is often named in Russian.
        let masker = Masker::new([(r"C:\Users\Кирилл", "%USERPROFILE%")], ["Кирилл"]);
        assert_eq!(
            masker.mask(r"C:\USERS\КИРИЛЛ\Desktop, кирилл"),
            r"%USERPROFILE%\Desktop, %USERNAME%"
        );
        // Two letters are in too many words to be told apart.
        let masker = Masker::new([(r"C:\Users\Al", "%USERPROFILE%")], ["Al"]);
        assert_eq!(
            masker.mask(r"C:\Users\Al\Desktop: Al"),
            r"%USERPROFILE%\Desktop: Al"
        );
    }

    #[test]
    fn a_folder_at_a_drive_s_root_is_left_alone() {
        // Documents moved to the root of D: would otherwise mask every path on that drive.
        let masker = Masker::new([(r"D:\", "%DOCUMENTS%")], std::iter::empty::<&str>());
        assert_eq!(
            masker.mask(r"D:\Games\Path of Exile 2"),
            r"D:\Games\Path of Exile 2"
        );
    }
}
