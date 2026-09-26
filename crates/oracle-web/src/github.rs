//! GitHub's REST API as the service uses it: an issue per report (with its labels), the
//! repository's releases, and their files. Every call carries the owner's fine-grained token
//! (`GITHUB_TOKEN`: Issues read/write, Contents read). Without one, issues are only logged and
//! there is no release to offer.

use std::collections::HashSet;
use std::time::Duration;

use reqwest::{Client, Method, RequestBuilder, Response, StatusCode, header};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::upstream::describe;

const API_VERSION: &str = "2022-11-28";
/// An API call: an issue or a label to create, a page of releases to read.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// How many releases a page of the list holds: GitHub's most.
pub const RELEASES_PER_PAGE: usize = 100;

/// The labels reports carry, each with the color and description it's created with when the
/// repository lacks it (`bug` and `enhancement` as GitHub's defaults have them).
const LABELS: [(&str, &str, &str); 6] = [
    ("bug", "d73a4a", "Something isn't working"),
    ("enhancement", "a2eeef", "New feature or request"),
    ("item", "fbca04", "Цена или разбор предмета"),
    ("crash", "b60205", "Программа закрылась с ошибкой"),
    ("from-app", "0e8a16", "Сообщение из программы"),
    ("from-site", "1d76db", "Сообщение с сайта"),
];

pub struct GitHub {
    http: Client,
    api: String,
    repo: String,
    token: Option<String>,
    labels: Mutex<Labels>,
}

/// The repository's labels, as far as this process has learned them.
#[derive(Default)]
struct Labels {
    /// Listed once; labels created since are added.
    existing: Option<HashSet<String>>,
    /// Labels GitHub refused to create: issues go out without them until the service restarts.
    refused: HashSet<&'static str>,
}

/// An issue opened for a report.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub html_url: String,
}

/// What became of a report on GitHub.
pub enum Filing {
    Opened(Issue),
    /// No token: the issue was logged instead.
    DryRun,
    Failed,
}

impl Filing {
    /// Whether GitHub has the report, or would have outside a dry run.
    pub fn took(&self) -> bool {
        !matches!(self, Filing::Failed)
    }
}

/// The part of GitHub's release JSON the service uses.
#[derive(Debug, Clone, Deserialize)]
pub struct GhRelease {
    pub tag_name: String,
    /// Not published yet.
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    pub assets: Vec<GhAsset>,
}

/// A page of the repository's releases, as GitHub gave it.
pub struct Page {
    /// GitHub's validator of the page, which asks it whether the page has changed since.
    pub etag: Option<String>,
    pub releases: Vec<GhRelease>,
}

/// GitHub's answer for a page of releases.
pub enum Listed {
    /// The page is as it was when its validator was given: a `304`, which costs no rate limit.
    Unchanged,
    Changed(Page),
}

/// One file of a release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GhAsset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    /// `uploaded`, or `open` while an upload is still going on.
    #[serde(default = "uploaded")]
    pub state: String,
}

fn uploaded() -> String {
    "uploaded".to_owned()
}

impl GitHub {
    pub fn new(http: Client, api: String, repo: String, token: Option<String>) -> GitHub {
        GitHub {
            http,
            api,
            repo,
            token,
            labels: Mutex::default(),
        }
    }

    pub fn configured(&self) -> bool {
        self.token.is_some()
    }

    /// A call answered in GitHub's JSON.
    fn call(&self, method: Method, path: &str, token: &str) -> RequestBuilder {
        self.request(method, path, token)
            .header(header::ACCEPT, "application/vnd.github+json")
    }

    /// A call to `path` under the repository, with the token. Headers add up rather than replace
    /// each other, so each call names its own `Accept`.
    fn request(&self, method: Method, path: &str, token: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}/repos/{}/{path}", self.api, self.repo))
            .bearer_auth(token)
            .header("X-GitHub-Api-Version", API_VERSION)
    }

    /// Opens an issue with as many of `labels` as the repository has or lets this token create.
    pub async fn open_issue(&self, title: &str, body: &str, labels: [&'static str; 2]) -> Filing {
        let Some(token) = &self.token else {
            info!(
                title,
                ?labels,
                body_chars = body.chars().count(),
                "dry run: would open a GitHub issue"
            );
            debug!(body, "dry run: the issue's body");
            return Filing::DryRun;
        };
        let labels = self.usable_labels(token, labels).await;
        let sent = self
            .call(Method::POST, "issues", token)
            .timeout(CALL_TIMEOUT)
            .json(&json!({ "title": title, "body": body, "labels": labels }))
            .send()
            .await;
        match answer::<Issue>(sent).await {
            Ok(issue) => Filing::Opened(issue),
            Err(problem) => {
                warn!(%problem, "GitHub didn't open the issue");
                Filing::Failed
            }
        }
    }

    /// Those of `wanted` the repository has, creating the ones it lacks. Listed once per process;
    /// a label GitHub refuses to create is left off from then on.
    async fn usable_labels(&self, token: &str, wanted: [&'static str; 2]) -> Vec<&'static str> {
        let mut labels = self.labels.lock().await;
        if labels.existing.is_none() {
            let sent = self
                .call(Method::GET, "labels?per_page=100", token)
                .timeout(CALL_TIMEOUT)
                .send()
                .await;
            match answer::<Vec<Named>>(sent).await {
                Ok(listed) => {
                    labels.existing = Some(listed.into_iter().map(|label| label.name).collect())
                }
                Err(problem) => {
                    warn!(%problem, "couldn't list the labels; this issue goes without");
                    return Vec::new();
                }
            }
        }
        let mut usable = Vec::new();
        for name in wanted {
            if labels
                .existing
                .as_ref()
                .is_some_and(|existing| existing.contains(name))
            {
                usable.push(name);
            } else if !labels.refused.contains(name) {
                match self.create_label(token, name).await {
                    Created::Yes => {
                        labels
                            .existing
                            .get_or_insert_default()
                            .insert(name.to_owned());
                        usable.push(name);
                    }
                    Created::Refused(problem) => {
                        warn!(label = name, %problem, "GitHub refused the label; issues go without it");
                        labels.refused.insert(name);
                    }
                    Created::Failed(problem) => {
                        warn!(label = name, %problem, "couldn't create the label")
                    }
                }
            }
        }
        usable
    }

    async fn create_label(&self, token: &str, name: &'static str) -> Created {
        let (_, color, description) = LABELS
            .into_iter()
            .find(|(label, ..)| *label == name)
            .unwrap_or((name, "ededed", ""));
        let sent = self
            .call(Method::POST, "labels", token)
            .timeout(CALL_TIMEOUT)
            .json(&json!({ "name": name, "color": color, "description": description }))
            .send()
            .await;
        let response = match sent {
            Ok(response) => response,
            Err(error) => return Created::Failed(describe(error)),
        };
        let status = response.status();
        if status.is_success() {
            return Created::Yes;
        }
        let refusal: Refusal = response.json().await.unwrap_or_default();
        // Created meanwhile, by another replica or by hand.
        if refusal
            .errors
            .iter()
            .any(|error| error.code == "already_exists")
        {
            Created::Yes
        } else if status.is_client_error() {
            Created::Refused(format!("{status}: {}", refusal.message))
        } else {
            Created::Failed(format!("{status}: {}", refusal.message))
        }
    }

    /// Page `page` (from 1) of the repository's releases, newest first as GitHub lists them,
    /// [`RELEASES_PER_PAGE`] to a page. With `etag`, the validator of the copy the caller holds, a
    /// page unchanged since is [`Listed::Unchanged`].
    pub async fn releases(&self, page: usize, etag: Option<&str>) -> Result<Listed, String> {
        let token = self.token.as_deref().ok_or("no GitHub token")?;
        let path = format!("releases?per_page={RELEASES_PER_PAGE}&page={page}");
        let mut request = self.call(Method::GET, &path, token).timeout(CALL_TIMEOUT);
        if let Some(etag) = etag {
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        let response = request.send().await.map_err(describe)?;
        let status = response.status();
        if status == StatusCode::NOT_MODIFIED {
            return Ok(Listed::Unchanged);
        }
        if !status.is_success() {
            return Err(refusal(response).await);
        }
        let etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|etag| etag.to_str().ok())
            .map(str::to_owned);
        let releases = response.json().await.map_err(describe)?;
        Ok(Listed::Changed(Page { etag, releases }))
    }

    /// A release file, its bytes the answer's body. GitHub answers with a redirect to its storage,
    /// which the client follows without the token: reqwest drops `Authorization` when a redirect
    /// leaves the host. No overall timeout: a long file can take long to a slow player; the
    /// client's read timeout ends one that stalls.
    pub async fn download(&self, asset_id: u64) -> Result<Response, String> {
        let token = self.token.as_deref().ok_or("no GitHub token")?;
        let response = self
            .request(Method::GET, &format!("releases/assets/{asset_id}"), token)
            .header(header::ACCEPT, "application/octet-stream")
            .send()
            .await
            .map_err(describe)?;
        if response.status() == StatusCode::OK {
            Ok(response)
        } else {
            Err(refusal(response).await)
        }
    }
}

enum Created {
    Yes,
    /// GitHub said no: the token may not create labels.
    Refused(String),
    /// No answer, or GitHub failing: worth another try.
    Failed(String),
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

/// GitHub's error JSON.
#[derive(Default, Deserialize)]
struct Refusal {
    #[serde(default)]
    message: String,
    #[serde(default)]
    errors: Vec<RefusalError>,
}

#[derive(Deserialize)]
struct RefusalError {
    #[serde(default)]
    code: String,
}

/// A successful answer's JSON, or what went wrong, for the log.
async fn answer<T: DeserializeOwned>(sent: reqwest::Result<Response>) -> Result<T, String> {
    let response = sent.map_err(describe)?;
    if response.status().is_success() {
        response.json().await.map_err(describe)
    } else {
        Err(refusal(response).await)
    }
}

/// A refusal as the log shows it: the status and GitHub's message.
async fn refusal(response: Response) -> String {
    let status = response.status();
    match response.json::<Refusal>().await {
        Ok(refusal) if !refusal.message.is_empty() => format!("{status}: {}", refusal.message),
        _ => status.to_string(),
    }
}
