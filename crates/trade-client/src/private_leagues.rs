//! The player's private leagues, as pathofexile.com's own pages show them. GGG's API lists an
//! account's private leagues only under OAuth (`account:leagues`), and the trade site's league list
//! (`data/leagues`) holds only public ones, signed in or not. The site's "Private Leagues" page,
//! though, lists the signed-in account's own leagues -- the ones it made or joined; the caller's
//! client adds the session, as for searches -- and each league's page names the league the way
//! the trade site does, "<name> (PL<number>)", and says which public league it's made from.
//!
//! The list can hold PoE 1's leagues too. On 2026-09-24 a PoE 2 league's "League Type" said
//! "PoE 2 - HC Forbidden Rites"; by 2026-09-26 the site had dropped the "PoE 2 - ", on the card
//! and on the league's page alike ("HC Forbidden Rites"), and only the league's flag on the card
//! tells the games apart: a PoE 2 league's comes from PoE 2's folder on GGG's CDN
//! (`flagart/poe2/ForbiddenRitesHardcore.png`), a PoE 1 Allflame league's doesn't
//! (`flagart/BannerDeepwater.png`). Both shapes are read. Verified on the owner's account both
//! days: the list named "HC FRites League by Cardiff", made from "HC Forbidden Rites", and its page
//! "HC FRites League by Cardiff (PL86503)". Only these pages are read, and nothing that changes the
//! account.

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use futures::AsyncReadExt as _;
use http_client::{AsyncBody, HttpClient};

use crate::account::unescape_html;

/// The site the pages are read from: the international one, which labels a league's details in
/// English whatever the player's language.
const SITE: &str = "https://www.pathofexile.com";
/// Where each league's card on the list starts.
const CARD: &str = r#"class="custom-league""#;
/// Where a PoE 2 league's flag is on GGG's CDN: a card shows it
/// (`…/custom-leagues/flagart/poe2/ForbiddenRitesHardcore.png`), and a PoE 1 league's is elsewhere.
const POE2_FLAG: &str = "/custom-leagues/flagart/poe2/";
/// How a PoE 2 league's "League Type" began before the site dropped it: "PoE 2 - HC Forbidden
/// Rites" for a league made from HC Forbidden Rites.
const POE2_TYPE: &str = "PoE 2 - ";
/// The label of a league's type, the public league it's made from, on its page.
const TYPE_LABEL: &str = "League Type:</span>";

/// One PoE 2 private league.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateLeague {
    /// What the trade site knows it by: "HC FRites League by Cardiff (PL86503)".
    pub id: String,
    /// The public league it's made from: "HC Forbidden Rites".
    pub parent: String,
}

/// The signed-in account's PoE 2 private leagues, in the list's order, each named from its own
/// page: two or three pages for most players. Empty when it has none. A league the list shows but
/// this can't read -- a card that doesn't look like PoE 2's, a page without its name -- is logged,
/// so a change of the site's pages shows in the log rather than as no leagues.
pub async fn mine(client: &Arc<dyn HttpClient>) -> Result<Vec<PrivateLeague>> {
    let Page::Found(list) = get(client, "/private-leagues").await? else {
        bail!("pathofexile.com has no private leagues page");
    };
    let slugs = poe2_slugs(&list);
    let cards = list.matches(CARD).count();
    if cards > slugs.len() {
        log::info!(
            "pathofexile.com lists {cards} private leagues, {} of them PoE 2's",
            slugs.len()
        );
    }
    let mut leagues = Vec::new();
    for slug in slugs {
        let league = match get(client, &format!("/private-leagues/league/{slug}")).await? {
            Page::Found(page) => read_league(&page),
            Page::Missing => None,
        };
        match league {
            Some(league) => leagues.push(league),
            None => log::warn!("the page of private league {slug} names no private league"),
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
/// ("HC+FRites+League+by+Cardiff"): each card ([`CARD`]) links its league's page, and a PoE 2
/// league's card shows its flag from PoE 2's folder ([`POE2_FLAG`]) -- or, as the cards did
/// before, begins its type with [`POE2_TYPE`].
fn poe2_slugs(list: &str) -> Vec<String> {
    const LINK: &str = r#"href="/private-leagues/league/"#;
    let old_type = format!(">{POE2_TYPE}");
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
        let poe2 = card.contains(POE2_FLAG) || card.contains(&old_type);
        if !slug.is_empty() && poe2 && !slugs.iter().any(|kept| kept == slug) {
            slugs.push(slug.to_owned());
        }
    }
    slugs
}

/// A league's page read: its name as the trade site writes it -- the title of the page's details
/// panel -- and the public league its "League Type" names, without the "PoE 2 - " it once began
/// with. `None` for a page that names no private league.
fn read_league(page: &str) -> Option<PrivateLeague> {
    let details = &page[page.find(r#"class="prop title""#)?..];
    let heading = &details[details.find("<h2>")? + "<h2>".len()..];
    let id = unescape_html(heading[..heading.find("</h2>")?].trim());
    let kind = &details[details.find(TYPE_LABEL)? + TYPE_LABEL.len()..];
    let kind = &kind[kind.find("<span>")? + "<span>".len()..];
    let kind = unescape_html(kind[..kind.find("</span>")?].trim());
    let parent = kind.strip_prefix(POE2_TYPE).unwrap_or(&kind);
    (is_private(&id) && !parent.is_empty()).then(|| PrivateLeague {
        id,
        parent: parent.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::executor::block_on;
    use futures::future::BoxFuture;
    use http_client::http::HeaderValue;
    use http_client::{Request, Response, Url};

    use super::*;

    /// The account's list as the site served it 2026-09-26 -- one league, made from HC Forbidden
    /// Rites -- cut to the page's content: its header and scripts name the account.
    const LIST: &str = include_str!("../tests/fixtures/private-leagues-list.html");
    /// That league's page the same day, signed in: its title bar and details panel.
    const LEAGUE: &str = include_str!("../tests/fixtures/private-leagues-league.html");
    const CARDIFF: &str = "/private-leagues/league/HC+FRites+League+by+Cardiff";

    /// A PoE 1 league's card in the list's shape, its flag and type as its page showed them
    /// 2026-09-26, and that page's details.
    const POE1_CARD: &str = r#"<div class="custom-league">
    <div class="header">
        <h3>
            <a href="/private-leagues/league/Fresh+GSF">Fresh GSF</a>
                                </h3>
    </div>
    <div class="flagart">
        <img src="https://web.poecdn.com/protected/image/custom-leagues/flagart/BannerDeepwater.png">
    </div>
    <div class="endtime">
        <div class="prop">
            <span>League Type:</span>
            <span>Allflame</span>
        </div>
    </div>
    <div class="controls">
        <a class="button-text" href="/private-leagues/league/Fresh+GSF">View</a>
    </div>
</div>
"#;
    const POE1_LEAGUE: &str = r#"<div class="prop title"><h2>Fresh GSF (PL86432)</h2></div>
<div class="prop"><span>League Type:</span><span>Allflame</span></div>"#;

    /// The list and the league's page as the site served them 2026-09-24, cut down: no flags yet,
    /// and a PoE 2 league's type began with "PoE 2 - ".
    const LIST_2026_09_24: &str = r#"<div class="custom-league-list">
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
    const LEAGUE_2026_09_24: &str = r#"<div class="prop title"><h2>HC FRites League by Cardiff (PL86503)</h2></div>
<div class="prop"><span>League Type:</span><span>PoE 2 - HC Forbidden Rites</span></div>"#;

    fn cardiff() -> PrivateLeague {
        PrivateLeague {
            id: "HC FRites League by Cardiff (PL86503)".to_owned(),
            parent: "HC Forbidden Rites".to_owned(),
        }
    }

    /// pathofexile.com's pages by URL, a 404 for any other; it counts what was asked.
    struct Site {
        pages: HashMap<String, (u16, String)>,
        asked: AtomicUsize,
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
                asked: AtomicUsize::new(0),
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
            self.asked.fetch_add(1, Ordering::Relaxed);
            let url = request.uri().to_string();
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
        // A PoE 1 league first on the list: only its flag says it isn't PoE 2's.
        const FIRST: &str = r#"<div class="custom-league">"#;
        let list = LIST.replacen(FIRST, &format!("{POE1_CARD}{FIRST}"), 1);
        let site = Site::new(&[
            ("/private-leagues", 200, &list),
            (CARDIFF, 200, LEAGUE),
            ("/private-leagues/league/Fresh+GSF", 200, POE1_LEAGUE),
        ]);
        let client: Arc<dyn HttpClient> = site.clone();
        assert_eq!(block_on(mine(&client)).unwrap(), [cardiff()]);
        // The list and the PoE 2 league's page, which its card links twice: nothing else.
        assert_eq!(site.asked.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn the_pages_as_they_were_on_2026_09_24_still_read() {
        let site = Site::new(&[
            ("/private-leagues", 200, LIST_2026_09_24),
            (CARDIFF, 200, LEAGUE_2026_09_24),
        ]);
        let client: Arc<dyn HttpClient> = site.clone();
        assert_eq!(block_on(mine(&client)).unwrap(), [cardiff()]);
        assert_eq!(site.asked.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_signed_out_list_is_an_error_not_an_empty_one() {
        let client: Arc<dyn HttpClient> = Site::new(&[("/private-leagues", 401, "")]);
        assert!(block_on(mine(&client)).is_err());
    }

    #[test]
    fn a_page_that_names_no_private_league_names_none() {
        assert_eq!(
            read_league("<html><title>Path of Exile</title></html>"),
            None
        );
        // A public league's name, with no number.
        let public = LEAGUE.replace(" (PL86503)</h2>", "</h2>");
        assert_eq!(read_league(&public), None);
    }

    #[test]
    fn a_leagues_name_reads_as_the_trade_site_writes_it() {
        // The site escapes an apostrophe as `&#039;`.
        let page = LEAGUE.replace(
            "<h2>HC FRites League by Cardiff (PL86503)</h2>",
            "<h2>Cardiff&#039;s &amp; Co (PL86503)</h2>",
        );
        assert_eq!(
            read_league(&page).map(|league| league.id).as_deref(),
            Some("Cardiff's & Co (PL86503)")
        );
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
