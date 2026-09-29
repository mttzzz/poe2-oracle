//! The HTTP clients the app builds at launch (`app::run`): its own, which nearly every request goes
//! through, and the updater's, which has no read timeout.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use http_client::HttpClient;
use reqwest_client::ReqwestClient;

/// The app's HTTP clients. Both come from reqwest_client's constructor that takes a read timeout,
/// which also verifies certificates through Windows (rustls-platform-verifier, as Zed does) and
/// offers no ALPN, so they speak HTTP/1.1. The trade sites, poe2scout, GGG's CDN and GitHub all
/// answer it (checked 2026-09-23), and so does oracle.pushka.biz.
pub struct Clients {
    /// Nearly every request -- the trade sites', poe2scout's, GGG's CDN's -- with the app's read
    /// timeout (`app::READ_TIMEOUT`). Its answers are read only through `SessionHttpClient`
    /// (`session`), which reads them the way the read timeout needs. Reports go through a client of
    /// their own (`report::send`).
    pub app: Arc<dyn HttpClient>,
    /// The updater's (`updates`): its event stream, the release answers and the downloads. No read
    /// timeout: something on the way from the service -- an antivirus checking the installer, say
    /// -- can hold a download back longer than the app's read timeout before its first byte, and
    /// the updater has limits of its own: each release answer's and download's whole-request one
    /// (`auto_update`), and the event stream's watchdog. Nor does it need `SessionHttpClient`'s way
    /// of reading: without a read timeout reqwest starts no timer as an answer is read, and a
    /// request's own time limit is set as it's sent, in reqwest's runtime, so GPUI's executors read
    /// its answers as they are.
    pub updater: Arc<dyn HttpClient>,
}

impl Clients {
    /// The app's client with `read_timeout`, the updater's without one.
    pub fn new(read_timeout: Duration) -> Result<Clients> {
        let app = ReqwestClient::proxy_user_agent_and_read_timeout(
            None,
            crate::brand::USER_AGENT,
            Some(read_timeout),
        )?;
        let updater = ReqwestClient::proxy_and_user_agent(None, crate::brand::USER_AGENT)?;
        Ok(Clients {
            app: Arc::new(app),
            updater: Arc::new(updater),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use anyhow::Context as _;
    use auto_update::{Version, test_support};
    use futures::executor::block_on;
    use futures::future::BoxFuture;
    use http_client::http::HeaderValue;
    use http_client::{AsyncBody, Request, Response, Url};
    use oracle_protocol::{API_BASE, DOWNLOAD_PATH, installer_asset};

    use super::*;
    use crate::session::{SessionHttpClient, TradeSession};

    const INSTALLER: &[u8] = b"MZ\x90\x00 stand-in for an NSIS installer";

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A fresh directory under the system temp dir, removed on drop: this crate has no `tempfile`
    /// dependency, the same hand-rolled scheme as `update_rules`' tests.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> TempDir {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            TempDir(
                std::env::temp_dir()
                    .join(format!("poe2-oracle-http-test-{}-{n}", std::process::id())),
            )
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A server on this machine for `files`, each a path and its body, one request per connection.
    /// It sends every answer's headers at once, but the body of the one at `held` only `hold`
    /// later. Its address.
    fn serve(files: Vec<(String, Vec<u8>)>, held: String, hold: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let files: Arc<HashMap<String, Vec<u8>>> = Arc::new(files.into_iter().collect());
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let files = files.clone();
                let held = held.clone();
                std::thread::spawn(move || {
                    let mut head = Vec::new();
                    let mut byte = [0];
                    while !head.ends_with(b"\r\n\r\n") {
                        if stream.read(&mut byte).unwrap_or(0) == 0 {
                            return;
                        }
                        head.push(byte[0]);
                    }
                    let head = String::from_utf8_lossy(&head);
                    let path = head.split(' ').nth(1).unwrap_or_default();
                    let (status, body) = match files.get(path) {
                        Some(body) => ("200 OK", body.as_slice()),
                        None => ("404 Not Found", &[][..]),
                    };
                    let headers = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(headers.as_bytes());
                    if path == held {
                        std::thread::sleep(hold);
                    }
                    let _ = stream.write_all(body);
                });
            }
        });
        base
    }

    /// `client`, sending what's meant for the service ([`API_BASE`]) to `base` instead.
    struct Local {
        base: String,
        client: Arc<dyn HttpClient>,
    }

    impl HttpClient for Local {
        fn user_agent(&self) -> Option<&HeaderValue> {
            self.client.user_agent()
        }

        fn proxy(&self) -> Option<&Url> {
            self.client.proxy()
        }

        fn send(
            &self,
            mut request: Request<AsyncBody>,
        ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
            let url = request.uri().to_string();
            let path = url
                .strip_prefix(API_BASE)
                .expect("the updater asks its service only");
            *request.uri_mut() = format!("{}{path}", self.base)
                .parse()
                .expect("an address on this machine");
            self.client.send(request)
        }
    }

    /// Release 0.2.0, fetched through `client` as the updater fetches a new version -- the
    /// service's answer, then the installer, verified -- from a server that holds the installer's
    /// body back for `hold` after its headers. The installer's bytes.
    fn fetch_held_installer(client: Arc<dyn HttpClient>, hold: Duration) -> Result<Vec<u8>> {
        let version = Version::new(0, 2, 0);
        let held = format!(
            "{DOWNLOAD_PATH}/v{version}/{}",
            installer_asset(&version.to_string())
        );
        let base = serve(test_support::release_files(&version, INSTALLER), held, hold);
        let client: Arc<dyn HttpClient> = Arc::new(Local { base, client });
        let dir = TempDir::new();
        // Off tokio, as on GPUI's executors.
        block_on(async {
            let update = auto_update::check_for_update(&client, &Version::new(0, 1, 0), &dir.0)
                .await?
                .context("the service offers no newer version")?;
            let installer = test_support::download_update(&client, &update, &dir.0).await?;
            anyhow::Ok(fs::read(installer)?)
        })
    }

    #[test]
    fn an_installer_held_back_past_the_read_timeout_still_downloads_verified() {
        let clients = Clients::new(Duration::from_millis(250)).unwrap();
        let hold = Duration::from_millis(1500);

        // The app's client, read the way the app reads it, gives up on the installer...
        let app: Arc<dyn HttpClient> =
            Arc::new(SessionHttpClient::new(clients.app, TradeSession::default()));
        let cut = fetch_held_installer(app, hold).unwrap_err();
        assert!(
            format!("{cut:#}").contains(&installer_asset("0.2.0")),
            "{cut:#}"
        );
        // ...and the updater's waits for it.
        assert_eq!(
            fetch_held_installer(clients.updater, hold).unwrap(),
            INSTALLER
        );
    }
}
