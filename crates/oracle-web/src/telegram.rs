//! The owner's Telegram: reports and the morning digest go to the chat `TELEGRAM_CHAT_ID` through
//! the owner's tg-inbox bot (`TELEGRAM_TOKEN`).
//!
//! The tg-inbox daemon long-polls that same bot. A `getUpdates` from here would take its updates
//! away, and `setWebhook` or `deleteWebhook` would break its polling outright, so the only Bot API
//! methods this module can name are the two that send ([`Method`]).

use std::fmt::{self, Write as _};
use std::time::Duration;

use axum::body::Bytes;
use oracle_protocol::{Report, ReportKind, ReportSource};
use reqwest::Client;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::github::Filing;
use crate::issue::kind_name;
use crate::short;
use crate::upstream::describe;

/// The tries a call nobody waits on gets after its first, each after the wait Telegram asked for
/// ([`Failure::retry_after`]).
const RETRIES: u32 = 2;
/// The longest wait a "too many requests" answer gets honoured with. Longer means Telegram is
/// limiting the bot hard, and the call gives up.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);
/// The wait before another try after a failure Telegram named no wait for: no answer, or its
/// own trouble.
const RETRY_PAUSE: Duration = Duration::from_secs(5);
/// How much of a report's text the message shows; the issue has all of it.
const TEXT_CHARS: usize = 1500;

pub struct Telegram {
    http: Client,
    api: String,
    bot: Option<Bot>,
}

struct Bot {
    token: String,
    chat: String,
}

/// The Bot API methods the service calls: sending, never receiving (see the module's docs).
#[derive(Clone, Copy)]
enum Method {
    SendMessage,
    SendDocument,
}

impl Method {
    fn name(self) -> &'static str {
        match self {
            Method::SendMessage => "sendMessage",
            Method::SendDocument => "sendDocument",
        }
    }

    /// A message is small and a player may be waiting on it (with GitHub's issue before it); a
    /// file is at most the 8 MiB diagnostics zip, sent after the player has the answer.
    fn timeout(self) -> Duration {
        match self {
            Method::SendMessage => Duration::from_secs(8),
            Method::SendDocument => Duration::from_secs(60),
        }
    }
}

/// Why a call failed, and whether another try could go better.
#[derive(Debug)]
pub struct Failure {
    problem: String,
    /// How long to wait before trying again: what a "too many requests" asked for, or
    /// [`RETRY_PAUSE`] when it named no wait, after no answer, or after Telegram's own trouble.
    /// `None` when another try would be refused the same way.
    retry_after: Option<Duration>,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.problem)
    }
}

enum Payload {
    Json(Value),
    Form(Form),
}

/// A file to send with a report, as a reply to its message.
pub struct Document {
    pub name: String,
    pub mime: &'static str,
    pub bytes: Bytes,
    /// Telegram HTML.
    pub caption: String,
}

impl Telegram {
    pub fn new(http: Client, api: String, token: Option<String>, chat: Option<String>) -> Telegram {
        let bot = token.zip(chat).map(|(token, chat)| Bot { token, chat });
        Telegram { http, api, bot }
    }

    pub fn configured(&self) -> bool {
        self.bot.is_some()
    }

    /// Sends `html`, Telegram HTML with every outside string passed through [`escape`]. The
    /// message's id, or `None` in a dry run. One try: someone waits on it, so a "too many
    /// requests" isn't waited out here but left to [`Telegram::resend_message`].
    pub async fn send_message(&self, html: &str) -> Result<Option<i64>, Failure> {
        let Some(bot) = &self.bot else {
            info!(text = html, "dry run: would send a Telegram message");
            return Ok(None);
        };
        let result = self
            .call(bot, Method::SendMessage, message(bot, html))
            .await?;
        Ok(result.get("message_id").and_then(Value::as_i64))
    }

    /// Sends `html` again after [`Telegram::send_message`] failed with `failure`, once nobody
    /// waits on it: up to [`RETRIES`] tries, each after the wait the last failure asked for.
    pub async fn resend_message(
        &self,
        html: &str,
        failure: Failure,
    ) -> Result<Option<i64>, Failure> {
        let Some(bot) = &self.bot else {
            return Ok(None);
        };
        let result = self
            .retry(bot, Method::SendMessage, || message(bot, html), failure)
            .await?;
        Ok(result.get("message_id").and_then(Value::as_i64))
    }

    /// Sends `document`, as a reply to the message `reply_to` when there is one. Nobody waits on
    /// it: a failure gets up to [`RETRIES`] more tries, each after the wait it asked for.
    pub async fn send_document(
        &self,
        document: &Document,
        reply_to: Option<i64>,
    ) -> Result<(), Failure> {
        let Some(bot) = &self.bot else {
            info!(
                name = document.name,
                bytes = document.bytes.len(),
                "dry run: would send a file to Telegram"
            );
            return Ok(());
        };
        let payload = || {
            let file =
                Part::stream_with_length(document.bytes.clone(), document.bytes.len() as u64)
                    .file_name(document.name.clone())
                    .mime_str(document.mime)
                    .expect("the documents' MIME types are valid");
            let mut form = Form::new()
                .text("chat_id", bot.chat.clone())
                .text("caption", document.caption.clone())
                .text("parse_mode", "HTML")
                .part("document", file);
            if let Some(message_id) = reply_to {
                let reply =
                    json!({ "message_id": message_id, "allow_sending_without_reply": true });
                form = form.text("reply_parameters", reply.to_string());
            }
            Payload::Form(form)
        };
        match self.call(bot, Method::SendDocument, payload()).await {
            Ok(_) => Ok(()),
            Err(failure) => self
                .retry(bot, Method::SendDocument, payload, failure)
                .await
                .map(drop),
        }
    }

    /// Tries `method` again after `failure`, up to [`RETRIES`] times, each after the wait the last
    /// failure asked for. Gives up at a failure another try can't mend, or a wait over
    /// [`MAX_RETRY_AFTER`].
    async fn retry(
        &self,
        bot: &Bot,
        method: Method,
        payload: impl Fn() -> Payload,
        mut failure: Failure,
    ) -> Result<Value, Failure> {
        for _ in 0..RETRIES {
            let Some(wait) = failure.retry_after.filter(|wait| *wait <= MAX_RETRY_AFTER) else {
                break;
            };
            warn!(
                method = method.name(),
                %failure,
                wait_secs = wait.as_secs(),
                "Telegram: trying again"
            );
            tokio::time::sleep(wait).await;
            match self.call(bot, method, payload()).await {
                Ok(result) => return Ok(result),
                Err(again) => failure = again,
            }
        }
        Err(failure)
    }

    /// Calls `method` with `payload`, once.
    async fn call(&self, bot: &Bot, method: Method, payload: Payload) -> Result<Value, Failure> {
        // The token is in the path: this URL is never logged, and errors drop it (`describe`).
        let url = format!("{}/bot{}/{}", self.api, bot.token, method.name());
        let request = self.http.post(&url).timeout(method.timeout());
        let request = match payload {
            Payload::Json(body) => request.json(&body),
            Payload::Form(form) => request.multipart(form),
        };
        let response = request.send().await.map_err(|error| Failure {
            problem: describe(error),
            retry_after: Some(RETRY_PAUSE),
        })?;
        let status = response.status();
        let answer: Answer = response.json().await.map_err(|error| Failure {
            problem: format!("{status}: {}", describe(error)),
            retry_after: status.is_server_error().then_some(RETRY_PAUSE),
        })?;
        if answer.ok {
            return Ok(answer.result);
        }
        let code = answer.error_code.unwrap_or(status.as_u16());
        let asked = answer
            .parameters
            .and_then(|parameters| parameters.retry_after);
        let retry_after = match code {
            429 => Some(asked.map_or(RETRY_PAUSE, Duration::from_secs)),
            500.. => Some(RETRY_PAUSE),
            _ => None,
        };
        Err(Failure {
            problem: format!("{code}: {}", answer.description),
            retry_after,
        })
    }
}

/// A message to the owner's chat: `html` is Telegram HTML.
fn message(bot: &Bot, html: &str) -> Payload {
    Payload::Json(json!({
        "chat_id": bot.chat,
        "text": html,
        "parse_mode": "HTML",
        "link_preview_options": { "is_disabled": true },
    }))
}

#[derive(Deserialize)]
struct Answer {
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error_code: Option<u16>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    parameters: Option<Parameters>,
}

#[derive(Deserialize)]
struct Parameters {
    #[serde(default)]
    retry_after: Option<u64>,
}

/// `text` for Telegram's HTML: the four characters it treats as markup become entities.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for char in text.chars() {
        match char {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            char => escaped.push(char),
        }
    }
    escaped
}

/// The message a report makes: its kind and origin, its issue and the app's version and languages,
/// the start of its text, the item, the contact, and a link to the issue.
pub fn report_message(report: &Report, filing: &Filing) -> String {
    let emoji = match report.kind {
        ReportKind::Bug => "🐞",
        ReportKind::Idea => "💡",
        ReportKind::Item => "💎",
        ReportKind::Crash => "💥",
    };
    let source = match report.source {
        ReportSource::App => "из программы",
        ReportSource::Site => "с сайта",
    };
    let mut message = format!("{emoji} <b>{}</b> · {source}\n", kind_name(report.kind));

    let mut facts = Vec::new();
    if let Filing::Opened(issue) = filing {
        facts.push(format!("#{}", issue.number));
    }
    if let Some(app) = &report.app {
        facts.push(format!("v{}", escape(&short(&app.version, 40))));
        let mut languages = format!("интерфейс {}", escape(&short(&app.interface_language, 20)));
        if let Some(client) = &app.client_language {
            let _ = write!(languages, ", клиент {}", escape(&short(client, 20)));
        }
        facts.push(languages);
    }
    if !facts.is_empty() {
        message.push_str(&facts.join(" · "));
        message.push('\n');
    }

    let text = report.text.trim();
    if !text.is_empty() {
        let _ = write!(message, "\n{}\n", escape(&short(text, TEXT_CHARS)));
    }
    if let Some(item) = &report.item {
        let _ = write!(
            message,
            "\nПредмет: <b>{}</b>",
            escape(&short(&item.name, 200))
        );
    }
    if let Some(panic) = report
        .crash
        .as_deref()
        .and_then(|crash| crash.lines().find(|line| !line.trim().is_empty()))
    {
        let _ = write!(
            message,
            "\nПаника: <code>{}</code>",
            escape(&short(panic.trim(), 300))
        );
    }
    match report
        .contact
        .as_deref()
        .map(str::trim)
        .filter(|contact| !contact.is_empty())
    {
        Some(contact) => {
            let _ = write!(message, "\nКонтакт: <code>{}</code>", escape(contact));
        }
        None => message.push_str("\nКонтакт не оставлен"),
    }
    match filing {
        Filing::Opened(issue) => {
            let _ = write!(
                message,
                "\n<a href=\"{}\">Issue #{} на GitHub</a>",
                escape(&issue.html_url),
                issue.number
            );
        }
        Filing::Failed => message.push_str("\n⚠️ GitHub не принял отчёт: он целиком в файле ниже"),
        Filing::DryRun => {}
    }
    message
}

#[cfg(test)]
mod tests {
    use oracle_protocol::{AppContext, ReportItem};

    use super::*;
    use crate::github::Issue;

    fn bug(text: &str) -> Report {
        Report {
            kind: ReportKind::Bug,
            source: ReportSource::App,
            text: text.to_owned(),
            contact: Some("<tg> @player & co".to_owned()),
            app: Some(AppContext {
                version: "0.1.0".to_owned(),
                interface_language: "ru".to_owned(),
                client_language: Some("en".to_owned()),
                windows: None,
                league: None,
                ui_scale: None,
            }),
            item: None,
            crash: None,
            diagnostics: None,
            website: String::new(),
        }
    }

    fn opened() -> Filing {
        Filing::Opened(Issue {
            number: 42,
            html_url: "https://github.com/mttzzz/poe2-oracle/issues/42".to_owned(),
        })
    }

    #[test]
    fn markup_characters_become_entities() {
        assert_eq!(
            escape(r#"<a href="x">&amp;</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;amp;&lt;/a&gt;"
        );
        assert_eq!(escape("цена > 5 div"), "цена &gt; 5 div");
    }

    #[test]
    fn a_report_message_escapes_everything_the_sender_wrote() {
        let mut report = bug("<b>жирный</b> & <a href=\"https://evil\">ссылка</a>");
        report.item = Some(ReportItem {
            name: "<i>Кольцо</i>".to_owned(),
            text: String::new(),
        });
        let message = report_message(&report, &opened());
        assert_eq!(
            message,
            "🐞 <b>Ошибка</b> · из программы\n\
             #42 · v0.1.0 · интерфейс ru, клиент en\n\
             \n\
             &lt;b&gt;жирный&lt;/b&gt; &amp; &lt;a href=&quot;https://evil&quot;&gt;ссылка&lt;/a&gt;\n\
             \n\
             Предмет: <b>&lt;i&gt;Кольцо&lt;/i&gt;</b>\n\
             Контакт: <code>&lt;tg&gt; @player &amp; co</code>\n\
             <a href=\"https://github.com/mttzzz/poe2-oracle/issues/42\">Issue #42 на GitHub</a>"
        );
    }

    #[test]
    fn a_long_text_shows_its_start_whole_characters_only() {
        let text = format!("{}<>", "ё".repeat(TEXT_CHARS - 1));
        let message = report_message(&bug(&text), &opened());
        // Cut after the 1500th character, before the second of `<>`: nothing half-escaped.
        assert!(
            message.contains(&format!("\n{}&lt;…\n", "ё".repeat(TEXT_CHARS - 1))),
            "{message}"
        );
    }

    #[test]
    fn without_an_issue_the_message_says_where_the_rest_is() {
        let mut crash = bug("");
        crash.kind = ReportKind::Crash;
        crash.contact = None;
        crash.crash = Some("\nthread 'main' panicked at src/app.rs:1:1:\n<boom>".to_owned());
        let message = report_message(&crash, &Filing::Failed);
        assert_eq!(
            message,
            "💥 <b>Вылет</b> · из программы\n\
             v0.1.0 · интерфейс ru, клиент en\n\
             \n\
             Паника: <code>thread 'main' panicked at src/app.rs:1:1:</code>\n\
             Контакт не оставлен\n\
             ⚠️ GitHub не принял отчёт: он целиком в файле ниже"
        );
    }
}
