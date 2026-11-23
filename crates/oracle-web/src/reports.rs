//! `POST /api/v1/reports`: a report from the app's report window or the site's form, passed on as
//! a GitHub issue and a Telegram message to the owner.
//!
//! The report is checked ([`Report::check`], the same rules the sender ran) and rate-limited
//! ([`crate::limits`]) before anything leaves the service. Its body is read only when a report
//! from its sender could pass the limits, and only while one of the [`REPORTS_AT_ONCE`] places
//! for reports in memory is free; a report with a diagnostics zip keeps its place until the zip
//! has gone -- to the reports' repository or, failing that, to Telegram -- so zips waiting on a
//! slow upload can't pile up either.
//! The issue goes first, for the report's text and context. With it open, the report's files --
//! the diagnostics zip, the item's or crash's text, which an issue can't carry whole -- are
//! committed to the reports' repository under `reports/<issue>/` and linked in a comment, and one
//! post (2026-10-18: one message per report) goes to the owner's Telegram: the report's message
//! alone. Without an issue, without a token, or when GitHub refuses the files, they go to Telegram
//! instead, as that one post's file. The sender hears 200 once the issue is open, or, when GitHub
//! failed, once Telegram took the post; 503 only when both failed -- then nothing was kept, and
//! trying again later is the right thing to do.

use std::fmt::{self, Display, Write as _};
use std::io::{Cursor, Write as _};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt as _;
use oracle_protocol::{
    MAX_BODY_BYTES, Problem, RejectReason, Report, ReportAccepted, ReportRejected,
};
use tokio::sync::{OwnedSemaphorePermit, oneshot};
use tracing::{Instrument as _, Span, error, info, warn};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::github::{Filing, GitHub};
use crate::stats::{self, Stat};
use crate::telegram::{self, Attachment, Post};
use crate::{App, REPORTS_AT_ONCE, issue, limits, moscow, short};

/// The longest a report's body may take to arrive: the largest report over a ~1 Mbit/s uplink.
/// The app waits 180 s for the upload and the answer together, and what the service does before
/// it answers -- [`GITHUB_LIMIT`], then, when GitHub failed, one try at the Telegram post (at most
/// 40 s) -- fits in the rest.
const BODY_TIMEOUT: Duration = Duration::from_secs(120);
/// The most GitHub gets to open an issue, labels included: the sender waits on it.
const GITHUB_LIMIT: Duration = Duration::from_secs(12);
/// The wait a sender is told while [`REPORTS_AT_ONCE`] other reports are in memory: a body takes
/// seconds on a usual connection, and so does a zip's way to Telegram.
const BUSY_RETRY_AFTER: u64 = 30;
/// The most of a parse error the sender is told: it may quote whatever the body holds.
const ERROR_CHARS: usize = 300;

pub async fn submit(State(app): State<Arc<App>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    // A cross-site form can post text/plain or form data without asking; only a same-origin page
    // or a client that isn't a browser can post JSON.
    if !is_json(&parts.headers) {
        return rejected(
            StatusCode::BAD_REQUEST,
            RejectReason::Invalid,
            "a report is application/json",
        );
    }
    let declared = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|length| length.to_str().ok()?.parse::<u64>().ok());
    if declared.is_some_and(|length| length > MAX_BODY_BYTES as u64) {
        return too_large();
    }
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    let client = limits::client_key(&parts.headers, peer);
    if let Some(wait) = limits::closed_to(&app.store, &client, moscow::now()).await {
        info!(wait, "report over the rate limit, refused unread");
        return rate_limited(wait);
    }

    // Held until the body is parsed: the body, and the zip decoded from it, are the report's
    // megabytes.
    let Ok(place) = app.reports_in_memory.clone().try_acquire_owned() else {
        info!("report refused: {REPORTS_AT_ONCE} others are in memory");
        return busy();
    };
    let bytes = match tokio::time::timeout(BODY_TIMEOUT, read(body, declared)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(Unread::TooLarge)) => return too_large(),
        Ok(Err(Unread::Broken(error))) => {
            return rejected(
                StatusCode::BAD_REQUEST,
                RejectReason::Invalid,
                format!("unreadable body: {error}"),
            );
        }
        Err(_) => {
            info!("report body not in after {} s", BODY_TIMEOUT.as_secs());
            return rejected(
                StatusCode::REQUEST_TIMEOUT,
                RejectReason::Unavailable,
                format!("the body didn't arrive in {} s", BODY_TIMEOUT.as_secs()),
            );
        }
    };
    // The zip is decoded straight from the body (`oracle_protocol`'s base64), and an error is cut
    // down while the body still holds the slot.
    let parsed = serde_json::from_slice::<Report>(&bytes)
        .map_err(|error| format!("not a report: {}", bounded(&error, ERROR_CHARS)));
    drop(bytes);
    let report = match parsed {
        Ok(report) => report,
        Err(message) => return rejected(StatusCode::BAD_REQUEST, RejectReason::Invalid, message),
    };
    // The zip is the part of a report that stays megabytes after the answer: its report keeps the
    // place until the zip has gone. Any other report gives it back now.
    let place = report.diagnostics.is_some().then_some(place);
    if let Err(problem) = report.check() {
        info!(?problem, kind = ?report.kind, source = ?report.source, "report refused");
        return rejected(
            StatusCode::BAD_REQUEST,
            RejectReason::Invalid,
            explain(problem),
        );
    }

    let now = moscow::now();
    if let Err(wait) = app
        .store
        .admit(&limits::counters(report.source, &client, now), now)
        .await
    {
        info!(kind = ?report.kind, source = ?report.source, wait, "report over the rate limit");
        return rate_limited(wait);
    }

    // Passed on in a task of its own, which the service waits for when it shuts down: a sender
    // hanging up halfway can't leave a report with GitHub but not with Telegram. The answer comes
    // as soon as the issue and the message are settled; the files follow in that task.
    let (answer, answered) = oneshot::channel();
    app.background
        .spawn(pass_on(app.clone(), report, answer, place).instrument(Span::current()));
    match answered.await {
        Ok(Some(id)) => Json(ReportAccepted { id }).into_response(),
        Ok(None) => rejected(
            StatusCode::SERVICE_UNAVAILABLE,
            RejectReason::Unavailable,
            "neither GitHub nor Telegram took the report",
        ),
        Err(_) => {
            error!("passing a report on failed before it could be answered");
            rejected(
                StatusCode::SERVICE_UNAVAILABLE,
                RejectReason::Unavailable,
                "the report was lost",
            )
        }
    }
}

/// Passes `report` on. The issue goes first ([`GITHUB_LIMIT`]), for the report's text and context;
/// `answer` gets its number (0 without one) once GitHub or Telegram took the report, `None` when
/// both failed. With the issue open, the report's files -- the diagnostics zip, the item's or
/// crash's text -- are committed to the reports' repository under `reports/<issue>/` and linked in
/// a comment, and one Telegram post, the report's message alone, goes to the owner. Without an
/// issue, without a token, or when GitHub refuses the files, the files go to Telegram instead, as
/// that post's file; the sender waits on that one try only when GitHub failed. `place`, the
/// report's place in memory when it has a zip, is given back once everything is settled.
async fn pass_on(
    app: Arc<App>,
    mut report: Report,
    answer: oneshot::Sender<Option<u64>>,
    place: Option<OwnedSemaphorePermit>,
) {
    let body = issue::body(&report);
    let title = issue::title(&report);
    let filing = match tokio::time::timeout(
        GITHUB_LIMIT,
        app.github.open_issue(&title, &body, issue::labels(&report)),
    )
    .await
    {
        Ok(filing) => filing,
        Err(_) => {
            warn!("GitHub took too long; the report's files go to Telegram");
            Filing::Failed
        }
    };
    let id = match &filing {
        Filing::Opened(issue) => issue.number,
        Filing::DryRun | Filing::Failed => 0,
    };
    let version = report
        .app
        .as_ref()
        .map_or_else(|| "-".to_owned(), |app| short(&app.version, 40));
    let zip_bytes = report.diagnostics.as_ref().map_or(0, Vec::len);
    let github = match &filing {
        Filing::Opened(_) => "issue",
        Filing::DryRun => "dry run",
        Filing::Failed => "failed",
    };
    // Named after the issue or else the moment, the files still say which report they are.
    let tag = match &filing {
        Filing::Opened(issue) => issue.number.to_string(),
        Filing::DryRun | Filing::Failed => moscow::stamp(moscow::now()),
    };
    let kind = report.kind;
    let source = report.source;

    // The report's message, and -- for the files that may go to Telegram -- the caption that names
    // them: both built while the report still holds its item and crash.
    let descriptions = file_descriptions(&report, &filing);
    let attached = (!descriptions.is_empty()).then(|| descriptions.join(" · "));
    let message = telegram::report_message(&report, &filing, None);
    let caption = telegram::report_message(&report, &filing, attached.as_deref());

    // The files an issue body can't carry whole, taken out of the report.
    let files = report_files(&mut report, &filing, &body, &tag, descriptions);

    // Without an issue, the files and the post are all that keep the report: the sender waits on one
    // try at the post, which carries them.
    let waited = if filing.took() {
        None
    } else {
        Some(
            app.telegram
                .send_post(&telegram_post(&caption, &files, &tag))
                .await,
        )
    };
    if let Some(Err(problem)) = &waited {
        warn!(%problem, "Telegram didn't take the report's post");
    }

    let taken = filing.took() || matches!(waited, Some(Ok(())));
    if !taken {
        warn!(kind = ?kind, source = ?source, version, zip_bytes, "report lost: GitHub and Telegram both failed");
    }
    // The sender may have hung up; the report goes on regardless.
    let _ = answer.send(taken.then_some(id));
    if !taken {
        return;
    }
    stats::count(&app, Stat::Report(kind));

    if waited.is_some() {
        // GitHub didn't take the report: the post with the files already went, waited on.
        info!(kind = ?kind, source = ?source, version, issue = id, github, telegram = "with files", zip_bytes, "report passed on");
        return;
    }

    // With the issue open, the files go to the reports' repository, linked in a comment, and
    // Telegram gets the message alone. When that can't be done -- a dry run, or GitHub refusing a
    // file -- the files go to Telegram with the post instead, so nothing is lost. Nobody waits.
    let attached_to_github = match &filing {
        Filing::Opened(issue) if !files.is_empty() => {
            attach(&app.github, app.github.reports_repo(), issue.number, &files).await
        }
        _ => files.is_empty(),
    };
    let post = if attached_to_github {
        Post {
            html: message,
            file: None,
        }
    } else {
        telegram_post(&caption, &files, &tag)
    };
    let delivered = app.telegram.deliver_post(&post).await;
    if let Err(problem) = &delivered {
        error!(issue = id, %problem, "the report's post never reached Telegram");
    }
    info!(
        kind = ?kind,
        source = ?source,
        version,
        issue = id,
        github,
        attached = attached_to_github,
        posted = delivered.is_ok(),
        zip_bytes,
        "report passed on"
    );
    drop(place);
}

/// Commits the report's `files` to `repo` under `reports/<number>/` and comments the links on issue
/// `number`. `false` when a commit or the comment failed: then the files go to Telegram instead, so
/// nothing is lost (a file committed before the failure is simply there twice).
async fn attach(github: &GitHub, repo: &str, number: u64, files: &[ReportFile]) -> bool {
    let mut links = Vec::with_capacity(files.len());
    for file in files {
        let path = format!("reports/{number}/{}", file.name);
        let message = format!("report #{number}: {}", file.name);
        match github.commit_file(repo, &path, &message, &file.bytes).await {
            Ok(url) => links.push((file, url)),
            Err(problem) => {
                warn!(issue = number, file = %file.name, %problem, "couldn't commit a report file; the files go to Telegram");
                return false;
            }
        }
    }
    match github
        .comment(repo, number, &attachments_comment(&links))
        .await
    {
        Ok(()) => true,
        Err(problem) => {
            warn!(issue = number, %problem, "couldn't comment the attachments; the files go to Telegram");
            false
        }
    }
}

/// The issue comment that links the committed `files`.
fn attachments_comment(links: &[(&ReportFile, String)]) -> String {
    let mut out = String::from("📎 Вложения отчёта (в репозитории):\n\n");
    for (file, url) in links {
        let _ = writeln!(out, "- [{}]({}) — {}", file.name, url, file.holds);
    }
    out
}

/// A file a report carries that the issue body can't hold whole: the diagnostics zip, the item's or
/// crash's text, or -- when GitHub didn't open an issue -- the whole report as Markdown. Committed
/// to the reports' repository, or sent to Telegram.
struct ReportFile {
    name: String,
    mime: &'static str,
    bytes: Bytes,
    /// What it holds, for the issue's comment and the Telegram caption: `диагностика, 54 КБ`.
    holds: String,
}

/// What each of the report's files holds, in the order [`report_files`] builds them: the whole
/// report (only without an issue), the diagnostics zip, the item's text, the crash's text.
fn file_descriptions(report: &Report, filing: &Filing) -> Vec<String> {
    let mut out = Vec::new();
    if let Filing::Failed = filing {
        out.push("отчёт целиком".to_owned());
    }
    if let Some(zip) = &report.diagnostics {
        out.push(format!("диагностика, {}", issue::size(zip.len())));
    }
    if report.item.is_some() {
        out.push("текст предмета".to_owned());
    }
    if report.crash.is_some() {
        out.push("текст вылета".to_owned());
    }
    out
}

/// The report's files, taken out of it, each paired with its description from [`file_descriptions`]
/// (same order). The diagnostics zip stays as it is; the item's and crash's text become `.txt`; the
/// whole report, only without an issue to hold it, becomes `.md`.
fn report_files(
    report: &mut Report,
    filing: &Filing,
    body: &str,
    tag: &str,
    descriptions: Vec<String>,
) -> Vec<ReportFile> {
    let mut holds = descriptions.into_iter();
    let mut files = Vec::new();
    if let Filing::Failed = filing {
        files.push(ReportFile {
            name: format!("report-{tag}.md"),
            mime: "text/markdown; charset=utf-8",
            bytes: Bytes::from(body.to_owned()),
            holds: holds.next().unwrap_or_default(),
        });
    }
    if let Some(zip) = report.diagnostics.take() {
        files.push(ReportFile {
            name: format!("diagnostics-{tag}.zip"),
            mime: "application/zip",
            bytes: Bytes::from(zip),
            holds: holds.next().unwrap_or_default(),
        });
    }
    if let Some(item) = report.item.take() {
        files.push(ReportFile {
            name: format!("item-{tag}.txt"),
            mime: "text/plain; charset=utf-8",
            bytes: Bytes::from(item.text),
            holds: holds.next().unwrap_or_default(),
        });
    }
    if let Some(crash) = report.crash.take() {
        files.push(ReportFile {
            name: format!("crash-{tag}.txt"),
            mime: "text/plain; charset=utf-8",
            bytes: Bytes::from(crash),
            holds: holds.next().unwrap_or_default(),
        });
    }
    files
}

/// The Telegram post for a report whose files go to Telegram: the diagnostics zip alone as it is, a
/// single text as it is, or several files bundled in one zip ([`bundle`]), with `caption` -- the
/// message naming what the file holds -- as the caption.
fn telegram_post(caption: &str, files: &[ReportFile], tag: &str) -> Post {
    let diagnostics = files.iter().find(|file| file.mime == "application/zip");
    let texts: Vec<&ReportFile> = files
        .iter()
        .filter(|file| file.mime != "application/zip")
        .collect();
    let file = match (diagnostics, texts.as_slice()) {
        (None, []) => None,
        (Some(zip), []) => Some(Attachment {
            name: format!("report-{tag}.zip"),
            mime: "application/zip",
            bytes: zip.bytes.clone(),
        }),
        (None, [text]) => Some(Attachment {
            name: text.name.clone(),
            mime: text.mime,
            bytes: text.bytes.clone(),
        }),
        (diagnostics, texts) => Some(bundle(texts, diagnostics, tag)),
    };
    Post {
        html: caption.to_owned(),
        file,
    }
}

/// `texts` and the `diagnostics` zip in one zip, `report-<tag>.zip`, every file stored as it is:
/// the diagnostics go in whole, already compressed and never read here.
fn bundle(texts: &[&ReportFile], diagnostics: Option<&ReportFile>, tag: &str) -> Attachment {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    // Into memory, a few files of at most 8 MiB together: nothing a zip refuses.
    for file in texts.iter().copied().chain(diagnostics) {
        zip.start_file(file.name.clone(), stored)
            .expect("a report's file names are valid in a zip");
        zip.write_all(&file.bytes)
            .expect("a zip in memory takes the bytes");
    }
    let bytes = zip.finish().expect("a zip in memory finishes").into_inner();
    Attachment {
        name: format!("report-{tag}.zip"),
        mime: "application/zip",
        bytes: Bytes::from(bytes),
    }
}

/// Why a body wasn't read whole.
enum Unread {
    TooLarge,
    Broken(axum::Error),
}

/// The whole of `body`, at most [`MAX_BODY_BYTES`], in one buffer. Sized as `declared` says when
/// it says: each chunk is copied once, and the whole never.
async fn read(mut body: Body, declared: Option<u64>) -> Result<Vec<u8>, Unread> {
    let capacity = declared.map_or(0, |length| length.min(MAX_BODY_BYTES as u64) as usize);
    let mut bytes = Vec::with_capacity(capacity);
    while let Some(frame) = body.frame().await {
        let Ok(data) = frame.map_err(Unread::Broken)?.into_data() else {
            continue;
        };
        if bytes.len() + data.len() > MAX_BODY_BYTES {
            return Err(Unread::TooLarge);
        }
        bytes.extend_from_slice(&data);
    }
    Ok(bytes)
}

/// `value` as it displays, cut to `max` characters: a parse error can quote as much of the body
/// as the body holds, which neither the answer nor memory should carry.
fn bounded(value: &impl Display, max: usize) -> String {
    struct Cut {
        text: String,
        room: usize,
    }
    impl fmt::Write for Cut {
        fn write_str(&mut self, part: &str) -> fmt::Result {
            for char in part.chars() {
                if self.room == 0 {
                    return Err(fmt::Error);
                }
                self.text.push(char);
                self.room -= 1;
            }
            Ok(())
        }
    }
    let mut cut = Cut {
        text: String::new(),
        room: max,
    };
    if write!(cut, "{value}").is_err() {
        cut.text.push('…');
    }
    cut.text
}

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

fn too_large() -> Response {
    rejected(
        StatusCode::PAYLOAD_TOO_LARGE,
        RejectReason::TooLarge,
        format!("a report is at most {} MiB", MAX_BODY_BYTES >> 20),
    )
}

/// 429, the seconds to wait in the message and in `Retry-After`.
fn rate_limited(wait: i64) -> Response {
    let mut response = rejected(
        StatusCode::TOO_MANY_REQUESTS,
        RejectReason::RateLimited,
        format!("too many reports from this address; try again in {wait} s"),
    );
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from(wait));
    response
}

/// 503 while [`REPORTS_AT_ONCE`] other reports are in memory, with `Retry-After`.
fn busy() -> Response {
    let mut response = rejected(
        StatusCode::SERVICE_UNAVAILABLE,
        RejectReason::Unavailable,
        "busy reading other reports; try again shortly",
    );
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from(BUSY_RETRY_AFTER));
    response
}

fn rejected(status: StatusCode, error: RejectReason, message: impl Into<String>) -> Response {
    (
        status,
        Json(ReportRejected {
            error,
            message: message.into(),
        }),
    )
        .into_response()
}

/// The service's words for a broken rule; the sender words the reason for the player itself.
fn explain(problem: Problem) -> &'static str {
    match problem {
        Problem::EmptyText => "the text is empty",
        Problem::TextTooLong => "the text is too long",
        Problem::ContactTooLong => "the contact is too long",
        Problem::ItemMissing => "an item report without its item",
        Problem::ItemTooLarge => "the item's name or text is too long",
        Problem::CrashMissing => "a crash report without the crash",
        Problem::CrashTooLarge => "the crash text is too long",
        Problem::DiagnosticsTooLarge => "the diagnostics zip is too large",
        Problem::ContextTooLong => "a value of the app's context is too long",
        Problem::AppMissing => "a report from the app without the app's context",
        Problem::NotFromTheApp => "the site sends problems and ideas only, with nothing attached",
        Problem::Honeypot => "not taken",
    }
}
