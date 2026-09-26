//! `POST /api/v1/reports`: a report from the app's report window or the site's form, passed on as
//! a GitHub issue and a Telegram message to the owner.
//!
//! The report is checked ([`Report::check`], the same rules the sender ran) and rate-limited
//! ([`crate::limits`]) before anything leaves the service. Its body is read only when a report
//! from its sender could pass the limits, and only while one of the [`REPORTS_AT_ONCE`] places
//! for reports in memory is free; a report with a diagnostics zip keeps its place until the zip
//! has gone to Telegram, so zips waiting on a slow Telegram can't pile up either.
//! Then both channels get it: the issue first, so the message can link it; the diagnostics zip
//! and the item's or crash's text go to Telegram as files replying to the message, since an issue
//! can't carry files. The sender hears 200 when either channel took it, and 503 only when both
//! failed -- then nothing was kept, and trying again later is the right thing to do.

use std::fmt::{self, Display, Write as _};
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

use crate::github::Filing;
use crate::stats::{self, Stat};
use crate::telegram::{self, Document};
use crate::{App, REPORTS_AT_ONCE, issue, limits, moscow, short};

/// The longest a report's body may take to arrive: the largest report over a ~1 Mbit/s uplink.
/// The app waits 180 s for the upload and the answer together, and what the service does before
/// it answers -- [`GITHUB_LIMIT`], then one try at the Telegram message -- fits in the rest.
const BODY_TIMEOUT: Duration = Duration::from_secs(120);
/// The most GitHub gets to open an issue, labels included: the sender waits on it, and on the
/// Telegram message after it (one try, at most 8 s).
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

/// Passes `report` on to GitHub and Telegram. `answer` gets the issue's number (0 without one)
/// once either took it, `None` when both failed. When it was taken, a message that failed gets
/// more tries after that, and the report's files go to Telegram either way. `place`, the
/// report's place in memory when it has a zip, is given back once the files are settled.
async fn pass_on(
    app: Arc<App>,
    mut report: Report,
    answer: oneshot::Sender<Option<u64>>,
    place: Option<OwnedSemaphorePermit>,
) {
    let telegram_on = app.telegram.configured();
    let body = issue::body(&report, telegram_on);
    let title = issue::title(&report);
    let filing = match tokio::time::timeout(
        GITHUB_LIMIT,
        app.github.open_issue(&title, &body, issue::labels(&report)),
    )
    .await
    {
        Ok(filing) => filing,
        Err(_) => {
            warn!("GitHub took too long; the report goes to Telegram whole");
            Filing::Failed
        }
    };
    let message = telegram::report_message(&report, &filing);
    let sent = app.telegram.send_message(&message).await;
    if let Err(problem) = &sent {
        warn!(%problem, "Telegram didn't take the report's message");
    }

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
    let telegram = match (&sent, telegram_on) {
        (Ok(_), true) => "sent",
        (Ok(_), false) => "dry run",
        (Err(_), _) => "failed",
    };
    let taken = filing.took() || sent.is_ok();
    if taken {
        info!(kind = ?report.kind, source = ?report.source, version, issue = id, github, telegram, zip_bytes, "report passed on");
        stats::count(&app, Stat::Report(report.kind));
    } else {
        warn!(kind = ?report.kind, source = ?report.source, version, zip_bytes, "report lost: GitHub and Telegram both failed");
    }
    // The sender may have hung up; the report goes on regardless.
    let _ = answer.send(taken.then_some(id));
    if !taken {
        return;
    }

    // Nobody waits any more: the message gets more tries, and the files go either way -- as
    // replies to it, or on their own. Named after the issue or else the moment, they still say
    // which report they belong to.
    let reply_to = match sent {
        Ok(reply_to) => reply_to,
        Err(failure) => match app.telegram.resend_message(&message, failure).await {
            Ok(reply_to) => {
                info!(
                    issue = id,
                    "the report's message reached Telegram on another try"
                );
                reply_to
            }
            Err(problem) => {
                error!(issue = id, %problem, "the report's message never reached Telegram; its files go without it");
                None
            }
        },
    };
    let tag = match &filing {
        Filing::Opened(issue) => issue.number.to_string(),
        Filing::DryRun | Filing::Failed => moscow::stamp(moscow::now()),
    };
    let mut documents = Vec::new();
    if let Filing::Failed = filing {
        documents.push(Document {
            name: format!("report-{tag}.md"),
            mime: "text/markdown; charset=utf-8",
            bytes: Bytes::from(body),
            caption: "Отчёт целиком, как он ушёл бы в issue".to_owned(),
        });
    }
    if let Some(zip) = report.diagnostics.take() {
        documents.push(Document {
            name: format!("report-{tag}.zip"),
            mime: "application/zip",
            bytes: Bytes::from(zip),
            caption: format!("Диагностика, {}", issue::size(zip_bytes)),
        });
    }
    if let Some(item) = report.item.take() {
        documents.push(Document {
            name: format!("item-{tag}.txt"),
            mime: "text/plain; charset=utf-8",
            bytes: Bytes::from(item.text),
            caption: format!("Текст предмета «{}»", telegram::escape(&item.name)),
        });
    }
    if let Some(crash) = report.crash.take() {
        documents.push(Document {
            name: format!("crash-{tag}.txt"),
            mime: "text/plain; charset=utf-8",
            bytes: Bytes::from(crash),
            caption: "Что программа сообщила при вылете".to_owned(),
        });
    }
    for document in &documents {
        if let Err(problem) = app.telegram.send_document(document, reply_to).await {
            error!(issue = id, name = document.name, %problem, "a report's file never reached Telegram and is lost");
        }
    }
    drop(documents);
    drop(place);
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
