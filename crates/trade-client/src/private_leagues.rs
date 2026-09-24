//! The player's private leagues, as pathofexile.com's own pages show them. GGG's API lists an
//! account's private leagues only under OAuth (`account:leagues`), and the trade site's league list
//! (`data/leagues`) holds only public ones, signed in or not. The site's "Private Leagues" page,
//! though, lists the signed-in account's own leagues -- the ones it made or joined; the caller's
//! client adds the session, as for searches -- and each league's page names the league the way
//! the trade site does, "<name> (PL<number>)", and says which public league it's made from.
//! Verified 2026-09-24 on the owner's account: the list named "HC FRites League by Cardiff", a
//! "PoE 2 - HC Forbidden Rites" league, and its page "HC FRites League by Cardiff (PL86503)". Only
//! these pages are read, and nothing that changes the account.

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use futures::AsyncReadExt as _;
use http_client::{AsyncBody, HttpClient};

use crate::account::unescape_html;

/// The site the pages are read from: the international one, which names a league's realm in
/// English whatever the player's language.
const SITE: &str = "https://www.pathofexile.com";
/// Where a league's page, or a card on the list, says what kind of league it is: a PoE 2 private
/// league's "League Type" is this, then the public league it's made from.
const POE2_TYPE: &str = "PoE 2 - ";

/// One PoE 2 private league.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateLeague {
    /// What the trade site knows it by: "HC FRites League by Cardiff (PL86503)".
    pub id: String,
    /// The public league it's made from: "HC Forbidden Rites".
    pub parent: String,
}

/// The signed-in account's PoE 2 private leagues, in the list's order, each named from its own
/// page: two or three pages for most players. Empty when it has none.
pub async fn mine(client: &Arc<dyn HttpClient>) -> Result<Vec<PrivateLeague>> {
    let Page::Found(list) = get(client, "/private-leagues").await? else {
        bail!("pathofexile.com has no private leagues page");
    };
    let mut leagues = Vec::new();
    for slug in poe2_slugs(&list) {
        if let Page::Found(page) = get(client, &format!("/private-leagues/league/{slug}")).await? {
            leagues.extend(read_league(&page));
        }
    }
    Ok(leagues)
}

/// Whether `name` is a private league's as the trade site writes it: "<name> (PL<number>)", the
/// way EE2 tells them apart (`Leagues.ts`, `isPrivateLeague`).
pub fn is_private(name: &str) -> bool {
    number_start(name).is_some()
}

/// Where `name`'s trailing "(PL<number>)" starts, if it has one.
fn number_start(name: &str) -> Option<usize> {
    let rest = name.strip_suffix(')')?;
    let start = rest.rfind("(PL")?;
    let number = &rest[start + 3..];
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then_some(start)
}

enum Page {
    Found(String),
    /// A 404: no such page -- for a league's, no such league.
    Missing,
}

/// A page of the site, redirects not followed: the session goes nowhere else, and a signed-out
/// one sent to the login page is an error, as any answer but the page or a 404 is.
async fn get(client: &Arc<dyn HttpClient>, path: &str) -> Result<Page> {
    let url = format!("{SITE}{path}");
    let mut response = client
        .get(&url, AsyncBody::default(), false)
        .await
        .with_context(|| format!("GET {url}"))?;
    match response.status().as_u16() {
        200 => {
            let mut page = String::new();
            response
                .body_mut()
                .read_to_string(&mut page)
                .await
                .with_context(|| format!("reading {url}"))?;
            Ok(Page::Found(page))
        }
        404 => Ok(Page::Missing),
        status => bail!("GET {url}: HTTP {status}"),
    }
}

/// The list page's PoE 2 leagues, as the paths of their pages name them
/// ("HC+FRites+League+by+Cardiff"): each card (`class="custom-league"`) links its league's page
/// and says its type.
fn poe2_slugs(list: &str) -> Vec<String> {
    const CARD: &str = r#"class="custom-league""#;
    const LINK: &str = r#"href="/private-leagues/league/"#;
    let mut slugs: Vec<String> = Vec::new();
    for card in list.split(CARD).skip(1) {
        let Some(at) = card.find(LINK) else {
            continue;
        };
        let rest = &card[at + LINK.len()..];
        let Some(end) = rest.find('"') else {
            continue;
        };
        let slug = &rest[..end];
        if !slug.is_empty()
            && card.contains(&format!(">{POE2_TYPE}"))
            && !slugs.iter().any(|kept| kept == slug)
        {
            slugs.push(slug.to_owned());
        }
    }
    slugs
}

/// A league's page read: its name as the trade site writes it -- the title of the page's details
/// panel -- and, for a PoE 2 league, the public league its "League Type" names. `None` for a page
/// that isn't a PoE 2 private league's.
fn read_league(page: &str) -> Option<PrivateLeague> {
    let details = &page[page.find(r#"class="prop title""#)?..];
    let heading = &details[details.find("<h2>")? + "<h2>".len()..];
    let id = unescape_html(heading[..heading.find("</h2>")?].trim());
    let kind =
        &page[page.find(&format!("<span>{POE2_TYPE}"))? + "<span>".len() + POE2_TYPE.len()..];
    let parent = unescape_html(kind[..kind.find("</span>")?].trim());
    (is_private(&id) && !parent.is_empty()).then_some(PrivateLeague { id, parent })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use futures::future::BoxFuture;
    use http_client::http::HeaderValue;
    use http_client::{Request, Response, Url};

    use super::*;

    /// The account's list, as the site served it 2026-09-24 -- the owner's league's card as it
    /// was -- and a second card of a league that isn't PoE 2's.
    const LIST: &str = r#"<div class="custom-leagues-pagination FontinSmallCaps">
        <div class="total-count">
        Total: 2    </div>
</div>
<div class="custom-leagues custom-league-view small centered">
    <div class="top-bg"></div>
<div class="custom-league-list">
<div class="custom-league">
    <div class="header">
        <h3>
            <a href="/private-leagues/league/HC+FRites+League+by+Cardiff"
                >HC FRites League by Cardiff</a>
                                </h3>
    </div>
    <div class="endtime">
        <div class="prop">
            <span>League Type:</span>
            <span>PoE 2 - HC Forbidden Rites</span>
        </div>
    </div>
    <div class="controls">
        <a class="button-text" href="/private-leagues/league/HC+FRites+League+by+Cardiff">View</a>
    </div>
</div>
<div class="custom-league">
    <div class="header">
        <h3><a href="/private-leagues/league/Old+Friends">Old Friends</a></h3>
    </div>
    <div class="endtime">
        <div class="prop"><span>League Type:</span><span>Mercenaries</span></div>
    </div>
</div>
</div>"#;

    /// A league's page, as the site served it 2026-09-24: its details panel.
    const LEAGUE: &str = r#"<div class="topBar first last shopTopBar1">
<h1>HC FRites League by Cardiff (PL86503)</h1></div>
<div class="league-details custom-league-panel"> <div class="props left-pane">
<div class="prop title"><h2>HC FRites League by Cardiff (PL86503)</h2></div>
<div class="prop"><span>Start Time:</span>
<span>Sep 4, 2026, 11:00:00 PM (Europe/Moscow)</span></div>
<div class="prop"><span>League Type:</span><span>PoE 2 - HC Forbidden Rites</span></div>
<div class="prop"><span>Players:</span><span>348 / 350 </span></div>"#;

    /// pathofexile.com's pages by URL, a 404 for any other; it records what was asked.
    struct Site {
        pages: HashMap<String, (u16, String)>,
        asked: Mutex<Vec<String>>,
    }

    impl Site {
        fn new(pages: &[(&str, u16, &str)]) -> Arc<Site> {
            Arc::new(Site {
                pages: pages
                    .iter()
                    .map(|&(path, status, body)| {
                        (format!("{SITE}{path}"), (status, body.to_owned()))
                    })
                    .collect(),
                asked: Mutex::new(Vec::new()),
            })
        }
    }

    impl HttpClient for Site {
        fn user_agent(&self) -> Option<&HeaderValue> {
            None
        }

        fn proxy(&self) -> Option<&Url> {
            None
        }

        fn send(
            &self,
            request: Request<AsyncBody>,
        ) -> BoxFuture<'static, Result<Response<AsyncBody>>> {
            let url = request.uri().to_string();
            self.asked.lock().unwrap().push(url.clone());
            let (status, body) = self
                .pages
                .get(&url)
                .cloned()
                .unwrap_or((404, String::new()));
            let response = Response::builder()
                .status(status)
                .body(AsyncBody::from(body))
                .map_err(anyhow::Error::from);
            Box::pin(async move { response })
        }
    }

    #[test]
    fn the_accounts_poe2_leagues_come_named_from_their_own_pages() {
        let site = Site::new(&[
            ("/private-leagues", 200, LIST),
            (
                "/private-leagues/league/HC+FRites+League+by+Cardiff",
                200,
                LEAGUE,
            ),
        ]);
        let client: Arc<dyn HttpClient> = site.clone();
        let leagues = futures::executor::block_on(mine(&client)).unwrap();
        assert_eq!(
            leagues,
            [PrivateLeague {
                id: "HC FRites League by Cardiff (PL86503)".to_owned(),
                parent: "HC Forbidden Rites".to_owned(),
            }]
        );
        // The list and the one PoE 2 league's page, which the card links twice: nothing else.
        assert_eq!(site.asked.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_signed_out_list_is_an_error_not_an_empty_one() {
        let client: Arc<dyn HttpClient> = Site::new(&[("/private-leagues", 401, "")]);
        assert!(futures::executor::block_on(mine(&client)).is_err());
    }

    #[test]
    fn a_page_that_is_not_a_poe2_private_leagues_names_none() {
        assert_eq!(
            read_league("<html><title>Path of Exile</title></html>"),
            None
        );
        let poe1 = LEAGUE.replace("PoE 2 - HC Forbidden Rites", "Mercenaries");
        assert_eq!(read_league(&poe1), None);
    }

    #[test]
    fn a_private_leagues_number_is_its_last_bracket() {
        assert!(is_private("HC FRites League by Cardiff (PL86503)"));
        assert!(!is_private("My League (PL12a)"));
        assert!(!is_private("My League (PL12345) x"));
        assert!(!is_private("My League (PL)"));
        assert!(!is_private("Forbidden Rites"));
    }
}
