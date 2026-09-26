//! The service end to end: its router on a real socket, with GitHub and Telegram played by servers
//! on 127.0.0.1 that record every request they get.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::Request;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use oracle_protocol::{
    AppContext, MAX_BODY_BYTES, Release, Report, ReportItem, ReportKind, ReportSource,
};
use oracle_web::{App, Config, REPORTS_AT_ONCE, router};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

const TOKEN: &str = "github-test-token";
const BOT: &str = "123:bot-test-token";
const INSTALLER: &[u8] = b"MZ\x90\x00 the installer's bytes";
const SUMS: &[u8] = b"0123abcd  PoE2-Oracle-Setup-0.1.0.exe\n";

/// A request a stand-in got.
#[derive(Clone, Debug)]
struct Seen {
    method: Method,
    path: String,
    headers: HeaderMap,
    body: Bytes,
}

#[derive(Clone)]
struct StandIn {
    seen: Arc<Mutex<Vec<Seen>>>,
    down: bool,
}

impl StandIn {
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().clone()
    }

    fn requests_to(&self, method: Method, path: &str) -> Vec<Seen> {
        self.seen()
            .into_iter()
            .filter(|seen| seen.method == method && seen.path == path)
            .collect()
    }
}

/// Serves `router` on 127.0.0.1, answering its base URL.
async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    format!("http://{address}")
}

/// What answers a stand-in's requests.
trait Answer: Fn(&Seen) -> Response + Clone + Send + Sync + 'static {}

impl<F: Fn(&Seen) -> Response + Clone + Send + Sync + 'static> Answer for F {}

async fn stand_in(down: bool, answer: impl Answer) -> (String, StandIn) {
    let stand_in = StandIn {
        seen: Arc::default(),
        down,
    };
    let recorder = stand_in.clone();
    let router = Router::new().fallback(move |request: Request| {
        let recorder = recorder.clone();
        let answer = answer.clone();
        async move {
            let (parts, body) = request.into_parts();
            let body = axum::body::to_bytes(body, usize::MAX).await.unwrap();
            let seen = Seen {
                method: parts.method,
                path: parts.uri.to_string(),
                headers: parts.headers,
                body,
            };
            recorder.seen.lock().push(seen.clone());
            if recorder.down {
                return (StatusCode::INTERNAL_SERVER_ERROR, "down").into_response();
            }
            answer(&seen)
        }
    });
    (serve(router).await, stand_in)
}

/// GitHub as the service calls it, for the token [`TOKEN`] only.
fn github(seen: &Seen) -> Response {
    if seen
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some(&format!("Bearer {TOKEN}"))
        && !seen.path.starts_with("/storage/")
    {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({ "message": "Bad credentials" })),
        )
            .into_response();
    }
    let json = |status: StatusCode, value: Value| (status, axum::Json(value)).into_response();
    match (seen.method.as_str(), seen.path.as_str()) {
        ("GET", "/repos/mttzzz/poe2-oracle/labels?per_page=100") => json(
            StatusCode::OK,
            json!([{ "name": "bug" }, { "name": "enhancement" }]),
        ),
        ("POST", "/repos/mttzzz/poe2-oracle/labels") => json(StatusCode::CREATED, json!({})),
        ("POST", "/repos/mttzzz/poe2-oracle/issues") => json(
            StatusCode::CREATED,
            json!({ "number": 42, "html_url": "https://github.com/mttzzz/poe2-oracle/issues/42" }),
        ),
        ("GET", "/repos/mttzzz/poe2-oracle/releases/latest") => json(
            StatusCode::OK,
            json!({
                "tag_name": "v0.1.0",
                "html_url": "https://github.com/mttzzz/poe2-oracle/releases/tag/v0.1.0",
                "draft": false,
                "prerelease": false,
                "assets": [
                    { "id": 1, "name": "PoE2-Oracle-Setup-0.1.0.exe", "size": INSTALLER.len(), "state": "uploaded" },
                    { "id": 2, "name": "SHA256SUMS", "size": SUMS.len(), "state": "uploaded" },
                ],
            }),
        ),
        // A file: GitHub sends the octet-stream request on to its storage.
        (
            "GET",
            "/repos/mttzzz/poe2-oracle/releases/assets/1"
            | "/repos/mttzzz/poe2-oracle/releases/assets/2",
        ) => {
            if seen
                .headers
                .get(header::ACCEPT)
                .map(|value| value.as_bytes())
                != Some(b"application/octet-stream")
            {
                return json(
                    StatusCode::OK,
                    json!({ "id": 1, "name": "metadata, not the file" }),
                );
            }
            let id = seen.path.rsplit('/').next().unwrap();
            (
                StatusCode::FOUND,
                [(header::LOCATION, format!("/storage/{id}"))],
            )
                .into_response()
        }
        ("GET", "/storage/1") => INSTALLER.into_response(),
        ("GET", "/storage/2") => SUMS.into_response(),
        _ => json(StatusCode::NOT_FOUND, json!({ "message": "Not Found" })),
    }
}

/// Telegram's Bot API for the bot [`BOT`]: the two methods the service may call.
fn telegram(seen: &Seen) -> Response {
    let answer = match seen.path.strip_prefix(&format!("/bot{BOT}/")) {
        Some("sendMessage") => json!({ "ok": true, "result": { "message_id": 7 } }),
        Some("sendDocument") => json!({ "ok": true, "result": { "message_id": 8 } }),
        _ => json!({ "ok": false, "error_code": 404, "description": "Not Found" }),
    };
    axum::Json(answer).into_response()
}

/// Telegram refusing a call for flooding, asking for a `wait` of that many seconds.
fn flooded(wait: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        axum::Json(json!({
            "ok": false,
            "error_code": 429,
            "description": format!("Too Many Requests: retry after {wait}"),
            "parameters": { "retry_after": wait },
        })),
    )
        .into_response()
}

struct Service {
    url: String,
    github: StandIn,
    telegram: StandIn,
    http: reqwest::Client,
    _dirs: [tempfile::TempDir; 3],
}

/// The service with GitHub and Telegram stood in for (each up or down), or with neither
/// configured: a dry run.
async fn start(github_down: bool, telegram_down: bool, dry: bool) -> Service {
    start_with(github_down, telegram_down, dry, telegram).await
}

/// [`start`] with Telegram answering as `telegram_answers`.
async fn start_with(
    github_down: bool,
    telegram_down: bool,
    dry: bool,
    telegram_answers: impl Answer,
) -> Service {
    let (github_url, github) = stand_in(github_down, github).await;
    let (telegram_url, telegram) = stand_in(telegram_down, telegram_answers).await;
    let dirs = [
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    ];
    site(dirs[0].path(), dirs[1].path(), dirs[2].path());
    let config = Config {
        public_url: "https://oracle.example".to_owned(),
        site_dir: dirs[0].path().to_owned(),
        guide_dir: dirs[1].path().to_owned(),
        images_dir: dirs[2].path().to_owned(),
        github_token: (!dry).then(|| TOKEN.to_owned()),
        telegram_token: (!dry).then(|| BOT.to_owned()),
        telegram_chat_id: (!dry).then(|| "1001".to_owned()),
        github_api: github_url,
        telegram_api: telegram_url,
        ..Config::default()
    };
    let url = serve(router(App::new(config).unwrap())).await;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    Service {
        url,
        github,
        telegram,
        http,
        _dirs: dirs,
    }
}

fn site(site: &Path, guide: &Path, images: &Path) {
    let page = |text: &str| {
        format!(
            "<!doctype html><html><body><p>{text}</p>{}</body></html>",
            " ".repeat(200)
        )
    };
    std::fs::create_dir_all(site.join("ru")).unwrap();
    std::fs::create_dir_all(site.join("ui")).unwrap();
    std::fs::write(site.join("index.html"), page("english")).unwrap();
    std::fs::write(site.join("ru/index.html"), page("русский")).unwrap();
    std::fs::write(site.join("404.html"), page("not here")).unwrap();
    std::fs::write(site.join("ui/panel.js"), "export const panel = 1;").unwrap();
    std::fs::write(site.join("LICENSE-MIT.txt"), "MIT License").unwrap();
    std::fs::write(guide.join("index.html"), page("the guide")).unwrap();
    std::fs::write(images.join("panel.png"), b"\x89PNG not really").unwrap();
}

fn app_report(kind: ReportKind, text: &str) -> Report {
    Report {
        kind,
        source: ReportSource::App,
        text: text.to_owned(),
        contact: Some("tg @player".to_owned()),
        app: Some(AppContext {
            version: "0.1.0".to_owned(),
            interface_language: "ru".to_owned(),
            client_language: Some("en".to_owned()),
            windows: Some("Windows 11 Pro 24H2 (26100.4061)".to_owned()),
            league: Some("Forbidden Rites".to_owned()),
            ui_scale: Some(1.0),
        }),
        item: None,
        crash: None,
        diagnostics: None,
        website: String::new(),
    }
}

fn site_report(text: &str) -> Report {
    Report {
        source: ReportSource::Site,
        app: None,
        contact: None,
        ..app_report(ReportKind::Idea, text)
    }
}

impl Service {
    async fn post(&self, report: &Report, client: &str) -> reqwest::Response {
        self.http
            .post(format!("{}/api/v1/reports", self.url))
            .header("x-forwarded-for", format!("10.9.9.9, {client}"))
            .json(report)
            .send()
            .await
            .unwrap()
    }

    async fn get(&self, path: &str, headers: &[(&str, &str)]) -> reqwest::Response {
        let mut request = self.http.get(format!("{}{path}", self.url));
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request.send().await.unwrap()
    }
}

/// What `stand_in` has seen once it has seen `count` requests: a report's files still go to
/// Telegram after the sender has its answer.
async fn seen_at_least(stand_in: &StandIn, count: usize) -> Vec<Seen> {
    for _ in 0..500 {
        let seen = stand_in.seen();
        if seen.len() >= count {
            return seen;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    stand_in.seen()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The files `telegram` was sent, once there are `count`: they go after the sender's answer.
async fn documents(telegram: &StandIn, count: usize) -> Vec<Seen> {
    let path = format!("/bot{BOT}/sendDocument");
    for _ in 0..500 {
        let sent = telegram.requests_to(Method::POST, &path);
        if sent.len() >= count {
            return sent;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    telegram.requests_to(Method::POST, &path)
}

/// A connection to the service that has sent the head of a report -- `length` bytes of JSON from
/// `client` -- and waits to be told to send the body (`Expect: 100-continue`): the service says
/// `100 Continue` once it reads the body, or refuses it unread. What it said comes along.
async fn upload_head(service: &Service, length: usize, client: &str) -> (TcpStream, String) {
    let address = service.url.trim_start_matches("http://");
    let mut stream = TcpStream::connect(address).await.unwrap();
    let head = format!(
        "POST /api/v1/reports HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\n\
         Content-Length: {length}\r\nExpect: 100-continue\r\nX-Forwarded-For: {client}\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    let said = read_answer(&mut stream).await;
    (stream, said)
}

/// What the service wrote to `stream` next.
async fn read_answer(stream: &mut TcpStream) -> String {
    let mut answer = vec![0; 4096];
    let read = stream.read(&mut answer).await.unwrap();
    String::from_utf8_lossy(&answer[..read]).into_owned()
}

#[tokio::test]
async fn a_report_reaches_github_and_telegram_with_its_files() {
    let service = start(false, false, false).await;
    let mut report = app_report(ReportKind::Item, "Цена странная\nвторая строка");
    report.item = Some(ReportItem {
        name: "Жуть шлема".to_owned(),
        text: "Item Class: Helmets\r\nRarity: Rare".to_owned(),
    });
    report.diagnostics = Some(b"PK\x03\x04 a diagnostics zip".to_vec());
    let response = service.post(&report, "203.0.113.1").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap(), json!({ "id": 42 }));

    // The missing labels were created once, then the issue opened with both.
    let created: Vec<Value> = service
        .github
        .requests_to(Method::POST, "/repos/mttzzz/poe2-oracle/labels")
        .iter()
        .map(|seen| serde_json::from_slice(&seen.body).unwrap())
        .collect();
    assert_eq!(created.len(), 2);
    assert_eq!(created[0]["name"], "item");
    assert_eq!(created[1]["name"], "from-app");
    let issues = service
        .github
        .requests_to(Method::POST, "/repos/mttzzz/poe2-oracle/issues");
    assert_eq!(issues.len(), 1);
    let issue: Value = serde_json::from_slice(&issues[0].body).unwrap();
    assert_eq!(issue["title"], "[Предмет] Жуть шлема");
    assert_eq!(issue["labels"], json!(["item", "from-app"]));
    let body = issue["body"].as_str().unwrap();
    assert!(
        body.contains("```text\nЦена странная\nвторая строка\n```"),
        "{body}"
    );
    assert!(
        body.contains("```text\nItem Class: Helmets\nRarity: Rare\n```"),
        "{body}"
    );
    assert!(
        body.contains("Диагностика (zip, 22 Б) — в Telegram"),
        "{body}"
    );

    // Telegram: the message linking the issue, then the files as replies to it.
    let telegram = seen_at_least(&service.telegram, 3).await;
    let paths: Vec<&str> = telegram.iter().map(|seen| seen.path.as_str()).collect();
    let bot = format!("/bot{BOT}");
    assert_eq!(
        paths,
        [
            format!("{bot}/sendMessage"),
            format!("{bot}/sendDocument"),
            format!("{bot}/sendDocument")
        ],
        "nothing but sending"
    );
    let message: Value = serde_json::from_slice(&telegram[0].body).unwrap();
    assert_eq!(message["chat_id"], "1001");
    assert_eq!(message["parse_mode"], "HTML");
    let text = message["text"].as_str().unwrap();
    assert!(
        text.starts_with(
            "💎 <b>Предмет</b> · из программы\n#42 · v0.1.0 · интерфейс ru, клиент en\n"
        ),
        "{text}"
    );
    assert!(
        text.ends_with(
            "<a href=\"https://github.com/mttzzz/poe2-oracle/issues/42\">Issue #42 на GitHub</a>"
        ),
        "{text}"
    );
    let zip = &telegram[1].body;
    assert!(contains(zip, b"filename=\"report-42.zip\""));
    assert!(contains(zip, b"PK\x03\x04 a diagnostics zip"));
    assert!(contains(zip, b"\"message_id\":7"), "a reply to the message");
    assert!(contains(zip, b"name=\"chat_id\"\r\n\r\n1001\r\n"));
    let item = &telegram[2].body;
    assert!(contains(item, b"filename=\"item-42.txt\""));
    assert!(contains(item, b"Item Class: Helmets\r\nRarity: Rare"));
}

#[tokio::test]
async fn github_down_still_takes_the_report_with_id_0() {
    let service = start(true, false, false).await;
    let response = service
        .post(
            &app_report(ReportKind::Bug, "Панель не открывается"),
            "203.0.113.2",
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap(), json!({ "id": 0 }));
    let telegram = seen_at_least(&service.telegram, 2).await;
    let message: Value = serde_json::from_slice(&telegram[0].body).unwrap();
    assert!(
        message["text"]
            .as_str()
            .unwrap()
            .ends_with("GitHub не принял отчёт: он целиком в файле ниже")
    );
    // The issue's body goes along as a file, so nothing of the report is lost.
    assert_eq!(telegram.len(), 2);
    assert!(contains(&telegram[1].body, b".md\""));
    assert!(contains(
        &telegram[1].body,
        "- Лига: `Forbidden Rites`".as_bytes()
    ));
}

#[tokio::test]
async fn both_down_is_503() {
    let service = start(true, true, false).await;
    let response = service
        .post(&app_report(ReportKind::Idea, "Идея"), "203.0.113.3")
        .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"],
        "unavailable"
    );
}

#[tokio::test]
async fn the_files_go_even_when_the_message_does_not() {
    let mut report = app_report(ReportKind::Bug, "Панель не открывается");
    report.diagnostics = Some(b"PK\x03\x04 a diagnostics zip".to_vec());

    // Telegram refuses the first message: after the sender has its answer, another try gets it
    // through, and the files reply to that one.
    let refused = Arc::new(AtomicBool::new(false));
    let service = start_with(false, false, false, move |seen: &Seen| {
        if seen.path.ends_with("/sendMessage") && !refused.swap(true, Ordering::SeqCst) {
            return flooded(0);
        }
        telegram(seen)
    })
    .await;
    let response = service.post(&report, "203.0.113.6").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap(), json!({ "id": 42 }));
    let zip = &documents(&service.telegram, 1).await[0];
    assert!(contains(&zip.body, b"filename=\"report-42.zip\""));
    assert!(
        contains(&zip.body, b"\"message_id\":7"),
        "a reply to the message"
    );

    // Telegram refuses every message: the files still go, on their own.
    let service = start_with(false, false, false, |seen: &Seen| {
        if seen.path.ends_with("/sendMessage") {
            return flooded(0);
        }
        telegram(seen)
    })
    .await;
    let response = service.post(&report, "203.0.113.7").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap(), json!({ "id": 42 }));
    let zip = &documents(&service.telegram, 1).await[0];
    assert!(contains(&zip.body, b"filename=\"report-42.zip\""));
    assert!(contains(&zip.body, b"PK\x03\x04 a diagnostics zip"));
    assert!(
        !contains(&zip.body, b"reply_parameters"),
        "nothing to reply to"
    );
    let messages = service
        .telegram
        .requests_to(Method::POST, &format!("/bot{BOT}/sendMessage"));
    assert!(messages.len() > 1, "the message was tried again");
}

#[tokio::test]
async fn reports_past_the_ones_being_read_wait_their_turn() {
    let service = start(false, false, true).await;
    let body = serde_json::to_vec(&site_report("идея")).unwrap();
    // Uploads that have sent their heads only: each holds its place once the service reads it.
    let mut uploads = Vec::new();
    for client in 0..REPORTS_AT_ONCE {
        let (upload, said) =
            upload_head(&service, body.len(), &format!("198.51.100.{client}")).await;
        assert!(said.starts_with("HTTP/1.1 100 Continue"), "{said}");
        uploads.push(upload);
    }
    // The next is refused before its body is read, and told when to come back.
    let (_, said) = upload_head(&service, body.len(), "203.0.113.8").await;
    assert!(said.starts_with("HTTP/1.1 503"), "{said}");
    assert!(
        said.to_ascii_lowercase().contains("\r\nretry-after: "),
        "{said}"
    );
    assert!(said.contains("\"error\":\"unavailable\""), "{said}");

    // One upload ends: its report is taken, and the next report gets its place.
    let mut done = uploads.pop().unwrap();
    done.write_all(&body).await.unwrap();
    let answer = read_answer(&mut done).await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let next = service.post(&site_report("идея"), "203.0.113.8").await;
    assert_eq!(next.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_report_with_a_zip_keeps_its_place_until_the_zip_has_gone() {
    // Telegram takes messages, but floods on files and asks for two seconds each time: every zip
    // stays with the service through the tries at sending it.
    let service = start_with(false, false, false, |seen: &Seen| {
        if seen.path.ends_with("/sendDocument") {
            return flooded(2);
        }
        telegram(seen)
    })
    .await;
    let mut report = app_report(ReportKind::Bug, "Панель не открывается");
    report.diagnostics = Some(b"PK\x03\x04 a diagnostics zip".to_vec());
    for client in 0..REPORTS_AT_ONCE {
        let response = service.post(&report, &format!("198.51.100.{client}")).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Their zips are still on their way: any next report is refused, even one without a zip.
    let refused = service.post(&site_report("идея"), "203.0.113.10").await;
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(refused.headers().contains_key(header::RETRY_AFTER));
    assert_eq!(
        refused.json::<Value>().await.unwrap()["error"],
        "unavailable"
    );

    // Once Telegram's tries at a zip are over, its report's place is free again.
    let mut tries = 0;
    loop {
        let response = service.post(&site_report("идея"), "203.0.113.10").await;
        if response.status() == StatusCode::OK {
            break;
        }
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        tries += 1;
        assert!(tries < 200, "the places never came back");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn a_sender_over_every_limit_is_refused_before_its_body() {
    let service = start(false, false, true).await;
    for _ in 0..3 {
        let response = service.post(&site_report("идея"), "203.0.113.9").await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    // The site's limit alone leaves the app's open: the body is read.
    let (_, said) = upload_head(&service, 100, "203.0.113.9").await;
    assert!(said.starts_with("HTTP/1.1 100 Continue"), "{said}");
    for _ in 0..5 {
        let response = service
            .post(&app_report(ReportKind::Bug, "ошибка"), "203.0.113.9")
            .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    // Both full: no report from this address could pass, and its body isn't read.
    let (_, said) = upload_head(&service, 100, "203.0.113.9").await;
    assert!(said.starts_with("HTTP/1.1 429"), "{said}");
    assert!(said.contains("\"error\":\"rate_limited\""), "{said}");
}

#[tokio::test]
async fn reports_are_checked_and_rate_limited() {
    let service = start(false, false, true).await;
    let reports = format!("{}/api/v1/reports", service.url);

    let form = service
        .http
        .post(&reports)
        .header("content-type", "text/plain")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(form.status(), StatusCode::BAD_REQUEST);
    assert_eq!(form.json::<Value>().await.unwrap()["error"], "invalid");
    let garbage = service
        .http
        .post(&reports)
        .header("content-type", "application/json")
        .body("{\"kind\":")
        .send()
        .await
        .unwrap();
    assert_eq!(garbage.status(), StatusCode::BAD_REQUEST);
    let mut with_item = site_report("идея");
    with_item.item = Some(ReportItem {
        name: "x".to_owned(),
        text: "y".to_owned(),
    });
    assert_eq!(
        service.post(&with_item, "203.0.113.4").await.status(),
        StatusCode::BAD_REQUEST
    );

    // Declared too large: refused before a byte of the body is read.
    let address = service.url.trim_start_matches("http://");
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let head = format!(
        "POST /api/v1/reports HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        MAX_BODY_BYTES + 1
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    let mut answer = vec![0; 4096];
    let read = stream.read(&mut answer).await.unwrap();
    let answer = String::from_utf8_lossy(&answer[..read]);
    assert!(answer.starts_with("HTTP/1.1 413"), "{answer}");
    assert!(answer.contains("\"error\":\"too_large\""), "{answer}");

    // Three from the site in ten minutes, dry run: taken with no issue number.
    for _ in 0..3 {
        let response = service.post(&site_report("идея"), "203.0.113.4").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.json::<Value>().await.unwrap(), json!({ "id": 0 }));
    }
    let limited = service.post(&site_report("идея"), "203.0.113.4").await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    let wait: u64 = limited.headers()[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=600).contains(&wait), "{wait}");
    assert_eq!(
        limited.json::<Value>().await.unwrap()["error"],
        "rate_limited"
    );
    assert_eq!(
        service
            .post(&site_report("идея"), "203.0.113.5")
            .await
            .status(),
        StatusCode::OK
    );
    // A dry run sends nothing anywhere.
    assert!(service.github.seen().is_empty() && service.telegram.seen().is_empty());
}

#[tokio::test]
async fn the_latest_release_is_mapped_cached_and_revalidated() {
    let service = start(false, false, false).await;
    let first = service.get("/api/v1/releases/latest", &[]).await;
    assert_eq!(first.status(), StatusCode::OK);
    let etag = first.headers()[header::ETAG].to_str().unwrap().to_owned();
    let release: Release = first.json().await.unwrap();
    assert_eq!(release.tag_name, "v0.1.0");
    assert_eq!(release.html_url, "https://oracle.example/");
    assert_eq!(
        release
            .assets
            .iter()
            .map(|asset| (asset.browser_download_url.as_str(), asset.size))
            .collect::<Vec<_>>(),
        [
            (
                "https://oracle.example/download/v0.1.0/PoE2-Oracle-Setup-0.1.0.exe",
                INSTALLER.len() as u64
            ),
            (
                "https://oracle.example/download/v0.1.0/SHA256SUMS",
                SUMS.len() as u64
            ),
        ]
    );

    let again = service.get("/api/v1/releases/latest", &[]).await;
    assert_eq!(again.headers()[header::ETAG], etag.as_str());
    let unchanged = service
        .get("/api/v1/releases/latest", &[("if-none-match", &etag)])
        .await;
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
    assert!(unchanged.bytes().await.unwrap().is_empty());
    let stale = service
        .get(
            "/api/v1/releases/latest",
            &[("if-none-match", "\"something else\"")],
        )
        .await;
    assert_eq!(stale.status(), StatusCode::OK);
    let asked = service
        .github
        .requests_to(Method::GET, "/repos/mttzzz/poe2-oracle/releases/latest");
    assert_eq!(asked.len(), 1, "cached for five minutes");
}

#[tokio::test]
async fn without_a_token_there_is_no_release() {
    let service = start(false, false, true).await;
    assert_eq!(
        service.get("/api/v1/releases/latest", &[]).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        service.get("/download/latest", &[]).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn downloads_stream_the_release_files() {
    let service = start(false, false, false).await;
    let latest = service.get("/download/latest", &[]).await;
    assert_eq!(latest.status(), StatusCode::FOUND);
    let location = latest.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(location, "/download/v0.1.0/PoE2-Oracle-Setup-0.1.0.exe");

    for _ in 0..2 {
        let file = service
            .get(&location, &[("accept-encoding", "gzip, br")])
            .await;
        assert_eq!(file.status(), StatusCode::OK);
        let headers = file.headers().clone();
        assert_eq!(headers[header::CONTENT_TYPE], "application/octet-stream");
        assert_eq!(
            headers[header::CONTENT_LENGTH],
            INSTALLER.len().to_string().as_str()
        );
        assert_eq!(
            headers[header::CONTENT_DISPOSITION],
            "attachment; filename=\"PoE2-Oracle-Setup-0.1.0.exe\""
        );
        assert!(!headers.contains_key(header::CONTENT_ENCODING));
        assert_eq!(file.bytes().await.unwrap(), INSTALLER);
    }
    let fetched = service
        .github
        .requests_to(Method::GET, "/repos/mttzzz/poe2-oracle/releases/assets/1");
    assert_eq!(fetched.len(), 1, "kept in memory after the first download");

    let sums = service.get("/download/v0.1.0/SHA256SUMS", &[]).await;
    assert_eq!(sums.bytes().await.unwrap(), SUMS);
    let head = service
        .http
        .head(format!("{}/download/v0.1.0/SHA256SUMS", service.url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        head.headers()[header::CONTENT_LENGTH],
        SUMS.len().to_string().as_str()
    );
    for missing in [
        "/download/v0.0.9/PoE2-Oracle-Setup-0.1.0.exe",
        "/download/v0.1.0/other.exe",
    ] {
        assert_eq!(
            service.get(missing, &[]).await.status(),
            StatusCode::NOT_FOUND,
            "{missing}"
        );
    }
}

#[tokio::test]
async fn the_site_is_served_from_its_directories() {
    let service = start(false, false, true).await;
    let home = service.get("/", &[]).await;
    assert_eq!(home.status(), StatusCode::OK);
    let headers = home.headers().clone();
    assert_eq!(headers[header::CONTENT_TYPE], "text/html; charset=utf-8");
    assert_eq!(headers[header::CACHE_CONTROL], "no-cache");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        "frame-ancestors 'none'"
    );
    assert_eq!(
        headers[header::REFERRER_POLICY],
        "strict-origin-when-cross-origin"
    );
    assert!(home.text().await.unwrap().contains("english"));
    let etag = headers[header::ETAG].to_str().unwrap();
    let unchanged = service.get("/", &[("if-none-match", etag)]).await;
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);

    assert!(
        service
            .get("/ru/", &[])
            .await
            .text()
            .await
            .unwrap()
            .contains("русский")
    );
    assert!(
        service
            .get("/guide/", &[])
            .await
            .text()
            .await
            .unwrap()
            .contains("the guide")
    );
    for (path, target) in [
        ("/ru", "/ru/"),
        ("/guide", "/guide/"),
        ("/ru?lang=1", "/ru/?lang=1"),
    ] {
        let moved = service.get(path, &[]).await;
        assert_eq!(moved.status(), StatusCode::MOVED_PERMANENTLY, "{path}");
        assert_eq!(moved.headers()[header::LOCATION], target);
    }

    let picture = service.get("/images/panel.png", &[]).await;
    assert_eq!(
        picture.headers()[header::CACHE_CONTROL],
        "public, max-age=600"
    );
    assert_eq!(
        picture.bytes().await.unwrap().as_ref(),
        b"\x89PNG not really"
    );
    let license = service.get("/LICENSE-MIT.txt", &[]).await;
    assert_eq!(
        license.headers()[header::CONTENT_TYPE],
        "text/plain; charset=utf-8"
    );

    // Compressed for a browser that takes it, with a weak validator for the compressed body.
    let gzipped = service.get("/", &[("accept-encoding", "gzip")]).await;
    assert_eq!(gzipped.headers()[header::CONTENT_ENCODING], "gzip");
    assert!(
        gzipped.headers()[header::ETAG]
            .to_str()
            .unwrap()
            .starts_with("W/\"")
    );

    for missing in [
        "/ui/",
        "/ui",
        "/nothing.html",
        "/guide/..%2Findex.html",
        "/images/",
    ] {
        let response = service.get(missing, &[]).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{missing}");
        assert!(
            response.text().await.unwrap().contains("not here"),
            "{missing}"
        );
    }
    assert_eq!(
        service.get("/healthz", &[]).await.text().await.unwrap(),
        "ok"
    );
}
