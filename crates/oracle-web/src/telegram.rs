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
/// The longest text a message may have, and a file's caption, once Telegram has parsed its HTML:
/// in the UTF-16 units Telegram counts them in.
const MESSAGE_LIMIT: usize = 4096;
const CAPTION_LIMIT: usize = 1024;

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

    /// How long one try may take. A message is small. A file is at most a report's 8 MiB of
    /// diagnostics: 60 s once nobody waits on it, 40 s while a sender does -- with the body's
    /// upload and GitHub's wait before it, still within the 180 s the app waits.
    fn timeout(self, waited: bool) -> Duration {
        match (self, waited) {
            (Method::SendMessage, _) => Duration::from_secs(8),
            (Method::SendDocument, true) => Duration::from_secs(40),
            (Method::SendDocument, false) => Duration::from_secs(60),
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

/// What a report sends to the owner's chat: one post (owner 2026-10-18: not a message and then a
/// file per part), a message or, when the report has files, a file with the message as its
/// caption.
pub struct Post {
    /// Telegram HTML: the message's text, or the file's caption.
    pub html: String,
    pub file: Option<Attachment>,
}

/// The file of a [`Post`].
pub struct Attachment {
    pub name: String,
    pub mime: &'static str,
    pub bytes: Bytes,
}

impl Telegram {
    pub fn new(http: Client, api: String, token: Option<String>, chat: Option<String>) -> Telegram {
        let bot = token.zip(chat).map(|(token, chat)| Bot { token, chat });
        Telegram { http, api, bot }
    }

    pub fn configured(&self) -> bool {
        self.bot.is_some()
    }

    /// Sends `html`, Telegram HTML with every outside string passed through [`escape`], once.
    pub async fn send_message(&self, html: &str) -> Result<(), Failure> {
        let Some(bot) = &self.bot else {
            info!(text = html, "dry run: would send a Telegram message");
            return Ok(());
        };
        self.call(bot, Method::SendMessage, message(bot, html), true)
            .await
            .map(drop)
    }

    /// Sends `post` once, for a sender waiting on it: a "too many requests" isn't waited out.
    pub async fn send_post(&self, post: &Post) -> Result<(), Failure> {
        let Some(bot) = &self.bot else {
            dry_run(post);
            return Ok(());
        };
        let (method, payload) = post_payload(bot, post);
        self.call(bot, method, payload, true).await.map(drop)
    }

    /// Sends `post` once nobody waits on it: a failure gets up to [`RETRIES`] more tries, each
    /// after the wait it asked for.
    pub async fn deliver_post(&self, post: &Post) -> Result<(), Failure> {
        let Some(bot) = &self.bot else {
            dry_run(post);
            return Ok(());
        };
        let (method, payload) = post_payload(bot, post);
        match self.call(bot, method, payload, false).await {
            Ok(_) => Ok(()),
            Err(failure) => self
                .retry(bot, method, || post_payload(bot, post).1, failure)
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
            match self.call(bot, method, payload(), false).await {
                Ok(result) => return Ok(result),
                Err(again) => failure = again,
            }
        }
        Err(failure)
    }

    /// Calls `method` with `payload`, once; `waited` when a sender waits on its answer.
    async fn call(
        &self,
        bot: &Bot,
        method: Method,
        payload: Payload,
        waited: bool,
    ) -> Result<Value, Failure> {
        // The token is in the path: this URL is never logged, and errors drop it (`describe`).
        let url = format!("{}/bot{}/{}", self.api, bot.token, method.name());
        let request = self.http.post(&url).timeout(method.timeout(waited));
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

/// How `post` goes: a message, or its file with the message as the caption.
fn post_payload(bot: &Bot, post: &Post) -> (Method, Payload) {
    let Some(file) = &post.file else {
        return (Method::SendMessage, message(bot, &post.html));
    };
    let part = Part::stream_with_length(file.bytes.clone(), file.bytes.len() as u64)
        .file_name(file.name.clone())
        .mime_str(file.mime)
        .expect("the attachments' MIME types are valid");
    let form = Form::new()
        .text("chat_id", bot.chat.clone())
        .text("caption", post.html.clone())
        .text("parse_mode", "HTML")
        .part("document", part);
    (Method::SendDocument, Payload::Form(form))
}

fn dry_run(post: &Post) {
    info!(
        text = post.html,
        file = post.file.as_ref().map(|file| file.name.as_str()),
        bytes = post.file.as_ref().map(|file| file.bytes.len()),
        "dry run: would send a Telegram post"
    );
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

/// The most of each outside field a report's message shows, in characters; the issue has them
/// whole. A file's caption has a quarter of a message's room, so its fields are shorter: even all
/// at their longest, in characters of two UTF-16 units, they leave the text room in it.
struct Caps {
    version: usize,
    language: usize,
    item: usize,
    panic: usize,
    contact: usize,
}

const MESSAGE_CAPS: Caps = Caps {
    version: 40,
    language: 20,
    item: 200,
    panic: 300,
    contact: 200,
};
const CAPTION_CAPS: Caps = Caps {
    version: 20,
    language: 10,
    item: 60,
    panic: 100,
    contact: 80,
};

/// The message a report makes: its kind and origin, its issue and the app's version and languages,
/// the start of its text, the item, the contact, what its file holds (`attached`, plain text: the
/// message is then the file's caption), and a link to the issue. At most [`MESSAGE_LIMIT`], or
/// [`CAPTION_LIMIT`] as a caption, once Telegram has parsed it: the report's text gets what the
/// rest leaves, up to [`TEXT_CHARS`].
pub fn report_message(report: &Report, filing: &Filing, attached: Option<&str>) -> String {
    let (limit, caps) = match attached {
        Some(_) => (CAPTION_LIMIT, CAPTION_CAPS),
        None => (MESSAGE_LIMIT, MESSAGE_CAPS),
    };
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
    let mut head = format!("{emoji} <b>{}</b> · {source}\n", kind_name(report.kind));

    let mut facts = Vec::new();
    if let Filing::Opened(issue) = filing {
        facts.push(format!("#{}", issue.number));
    }
    if let Some(app) = &report.app {
        facts.push(format!("v{}", escape(&short(&app.version, caps.version))));
        let interface = short(&app.interface_language, caps.language);
        let mut languages = format!("интерфейс {}", escape(&interface));
        if let Some(client) = &app.client_language {
            let client = short(client, caps.language);
            let _ = write!(languages, ", клиент {}", escape(&client));
        }
        facts.push(languages);
    }
    if !facts.is_empty() {
        head.push_str(&facts.join(" · "));
        head.push('\n');
    }

    let mut tail = String::new();
    if let Some(item) = &report.item {
        let _ = write!(
            tail,
            "\nПредмет: <b>{}</b>",
            escape(&short(&item.name, caps.item))
        );
    }
    if let Some(panic) = report
        .crash
        .as_deref()
        .and_then(|crash| crash.lines().find(|line| !line.trim().is_empty()))
    {
        let _ = write!(
            tail,
            "\nПаника: <code>{}</code>",
            escape(&short(panic.trim(), caps.panic))
        );
    }
    match report
        .contact
        .as_deref()
        .map(str::trim)
        .filter(|contact| !contact.is_empty())
    {
        Some(contact) => {
            let contact = short(contact, caps.contact);
            let _ = write!(tail, "\nКонтакт: <code>{}</code>", escape(&contact));
        }
        None => tail.push_str("\nКонтакт не оставлен"),
    }
    if let Some(attached) = attached {
        let _ = write!(tail, "\n📎 {}", escape(attached));
    }
    match filing {
        Filing::Opened(issue) => {
            let _ = write!(
                tail,
                "\n<a href=\"{}\">Issue #{} на GitHub</a>",
                escape(&issue.html_url),
                issue.number
            );
        }
        Filing::Failed => tail.push_str("\n⚠️ GitHub не принял отчёт: он целиком в файле"),
        Filing::DryRun => {}
    }

    let text = report.text.trim();
    if text.is_empty() {
        return head + &tail;
    }
    // The text between two line breaks, in what the head and the tail leave.
    let room = limit.saturating_sub(parsed_len(&head) + parsed_len(&tail) + 2);
    let text = fit(&short(text, TEXT_CHARS), room);
    format!("{head}\n{}\n{tail}", escape(&text))
}

/// How long `html`, Telegram HTML as this module writes it, is once Telegram has parsed it, in
/// UTF-16 units: a tag counts nothing and an entity one character. Every `<` and `&` in it is
/// markup, since outside text goes through [`escape`].
fn parsed_len(html: &str) -> usize {
    let mut units = 0;
    let mut chars = html.chars();
    while let Some(char) = chars.next() {
        match char {
            '<' => chars
                .by_ref()
                .take_while(|&char| char != '>')
                .for_each(drop),
            '&' => {
                chars
                    .by_ref()
                    .take_while(|&char| char != ';')
                    .for_each(drop);
                units += 1;
            }
            char => units += char.len_utf16(),
        }
    }
    units
}

/// `text` in at most `units` UTF-16 units: whole when it fits, else its start, whole characters
/// only, and `…`.
fn fit(text: &str, units: usize) -> String {
    if text.encode_utf16().count() <= units {
        return text.to_owned();
    }
    let mut kept = String::new();
    let mut used = 1; // the `…`
    for char in text.chars() {
        used += char.len_utf16();
        if used > units {
            break;
        }
        kept.push(char);
    }
    format!("{}…", kept.trim_end())
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
        let message = report_message(&report, &opened(), Some("текст предмета"));
        assert_eq!(
            message,
            "🐞 <b>Ошибка</b> · из программы\n\
             #42 · v0.1.0 · интерфейс ru, клиент en\n\
             \n\
             &lt;b&gt;жирный&lt;/b&gt; &amp; &lt;a href=&quot;https://evil&quot;&gt;ссылка&lt;/a&gt;\n\
             \n\
             Предмет: <b>&lt;i&gt;Кольцо&lt;/i&gt;</b>\n\
             Контакт: <code>&lt;tg&gt; @player &amp; co</code>\n\
             📎 текст предмета\n\
             <a href=\"https://github.com/mttzzz/poe2-oracle/issues/42\">Issue #42 на GitHub</a>"
        );
    }

    #[test]
    fn a_long_text_shows_its_start_whole_characters_only() {
        let text = format!("{}<>", "ё".repeat(TEXT_CHARS - 1));
        let message = report_message(&bug(&text), &opened(), None);
        // Cut after the 1500th character, before the second of `<>`: nothing half-escaped.
        assert!(
            message.contains(&format!("\n{}&lt;…\n", "ё".repeat(TEXT_CHARS - 1))),
            "{message}"
        );
    }

    #[test]
    fn telegram_counts_a_tag_as_nothing_an_entity_as_one_and_an_emoji_as_two() {
        assert_eq!(parsed_len("<b>a&amp;b</b>"), 3);
        assert_eq!(
            parsed_len("<a href=\"https://x/?a=1&amp;b=2\">#42</a> 💎"),
            6
        );
    }

    #[test]
    fn a_caption_fits_telegrams_limit_whatever_the_report_holds() {
        // Every field at its longest (`Report::check`), in characters that take the most room:
        // emoji, two UTF-16 units each, and markup that becomes entities.
        let mut report = bug(&"💥<".repeat(4000));
        report.kind = ReportKind::Crash;
        report.contact = Some("😀".repeat(200));
        report.app = Some(AppContext {
            version: "💥".repeat(100),
            interface_language: "💥".repeat(100),
            client_language: Some("💥".repeat(100)),
            windows: None,
            league: None,
            ui_scale: None,
        });
        report.item = Some(ReportItem {
            name: "💎".repeat(200),
            text: String::new(),
        });
        report.crash = Some(format!("{}\nstack", "😱".repeat(1000)));
        let attached = "отчёт целиком · диагностика, 8,0 МБ · текст предмета · текст вылета";
        let message = report_message(&report, &opened(), Some(attached));
        let length = parsed_len(&message);
        assert!(length <= CAPTION_LIMIT, "{length}: {message}");
        // The text took what the rest left, and the rest is whole: the file and the issue.
        assert!(length > CAPTION_LIMIT - 3, "{length}: {message}");
        assert!(message.contains("…\n"), "{message}");
        assert!(
            message.ends_with(&format!("📎 {attached}\n<a href=\"https://github.com/mttzzz/poe2-oracle/issues/42\">Issue #42 на GitHub</a>")),
            "{message}"
        );
    }

    #[test]
    fn without_an_issue_the_message_says_where_the_rest_is() {
        let mut crash = bug("");
        crash.kind = ReportKind::Crash;
        crash.contact = None;
        crash.crash = Some("\nthread 'main' panicked at src/app.rs:1:1:\n<boom>".to_owned());
        let attached = "отчёт целиком · текст вылета";
        let message = report_message(&crash, &Filing::Failed, Some(attached));
        assert_eq!(
            message,
            "💥 <b>Вылет</b> · из программы\n\
             v0.1.0 · интерфейс ru, клиент en\n\
             \n\
             Паника: <code>thread 'main' panicked at src/app.rs:1:1:</code>\n\
             Контакт не оставлен\n\
             📎 отчёт целиком · текст вылета\n\
             ⚠️ GitHub не принял отчёт: он целиком в файле"
        );
    }
}
