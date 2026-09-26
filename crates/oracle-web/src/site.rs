//! The site, laid out as GitHub Pages published it: the landing pages from `SITE_DIR` at the root
//! (English, Russian under `/ru/`), the guide from `GUIDE_DIR` under `/guide/`, its pictures from
//! `IMAGES_DIR` under `/images/`.
//!
//! Paths resolve the way Pages resolved them, so the pages' relative links keep working: a
//! directory is its `index.html`, a directory named without the trailing slash redirects to it
//! (relative links in the page need it), and a directory without an `index.html` is a 404, as is
//! any file or directory whose name starts with a dot. A missing page is answered with
//! `SITE_DIR/404.html` when there is one. tower-http's `ServeFile` sends the files, with
//! `ETag`/`Last-Modified`, conditional requests and ranges.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use percent_encoding::percent_decode_str;
use tower_http::services::ServeFile;

use crate::App;

/// Pages must show an edit at once: browsers ask again every time (a 304 when nothing changed).
const PAGE_CACHE: &str = "no-cache";
/// Styles, scripts, fonts and pictures may wait a little, as they did on Pages.
const ASSET_CACHE: &str = "public, max-age=600";

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

impl Site {
    pub fn new(site: PathBuf, guide: PathBuf, images: PathBuf) -> Site {
        Site {
            site,
            guide,
            images,
        }
    }

    /// The directory a request path's files come from, and the rest of the path under it.
    fn mount<'a>(&'a self, path: &'a str) -> (&'a Path, &'a str) {
        for (prefix, root) in [("/guide", &self.guide), ("/images", &self.images)] {
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
    let path = request.uri().path().to_owned();
    let (root, rest) = app.site.mount(&path);
    match resolve(root, rest).await {
        Target::File(file) => send(file, request).await,
        Target::Directory => {
            let location = match request.uri().query() {
                Some(query) => format!("{path}/?{query}"),
                None => format!("{path}/"),
            };
            (
                StatusCode::MOVED_PERMANENTLY,
                [(header::LOCATION, location)],
            )
                .into_response()
        }
        Target::Missing => not_found(&app.site, request.method()).await,
    }
}

/// `file` as `ServeFile` sends it, with the site's caching and text charset.
async fn send(file: PathBuf, request: Request) -> Response {
    let page = file
        .extension()
        .is_some_and(|extension| extension == "html");
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

/// A 404, with the site's own 404 page when it has one.
async fn not_found(site: &Site, method: &Method) -> Response {
    let page = site.site.join("404.html");
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
        _ => return StatusCode::NOT_FOUND.into_response(),
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
    response
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
    use super::*;

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
}
