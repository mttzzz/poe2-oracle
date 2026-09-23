//! Trade requests from the game log: the whispers the trade site writes for a buyer ("Hi, I would
//! like to buy your ...", "Здравствуйте, хочу купить у вас ..."), and other players arriving and
//! leaving -- the buyer joining the party, coming to the hideout. Plain text, built and tested on
//! every target; `ui::trade_overlay` shows the requests and answers them.
//!
//! Line shapes, as the clients write them to `Client.txt` (the prefixes are EE2's
//! `CHAT_WHISPER_FROM` for each client language; the texts are the trade site's own, verified in
//! `listing.whisper` of live fetch results 2026-09-23):
//!
//! ```text
//! 2026/09/09 16:02:28 33995171 3ef23348 [INFO Client 31244] @От кого Fansiel: спасибо)
//! ... @From <TAG> Buyer: Hi, I would like to buy your Gale Coil Emerald Ring listed for 1 aug in
//!     Forbidden Rites (stash tab "3"; position: left 1, top 1)
//! ... @От кого Buyer: Здравствуйте, хочу купить у вас Разящее Золотое кольцо льва за 1 transmute
//!     в лиге Forbidden Rites (секция "5"; позиция: 10 столбец, 12 ряд)
//! ... : Geladinhas has joined the area.
//! ... : Buyer присоединяется к вашей группе.
//! ```
//!
//! The request's text is in the language of the buyer's trade site, not this client's: an English
//! buyer's request reaches a Russian client in English, and the reply goes back in it.

/// How each client language starts a whisper received.
const WHISPER_FROM: [&str; 2] = ["@From ", "@От кого "];

/// What the clients print after another player's name when the player arrives (`true`) or leaves.
/// The party ones are the clients' own `ClientStrings` (`HUDCharacterJoinedParty` and
/// `HUDCharacterLeftParty`, both languages, read from the game's files 2026-09-23); the area ones
/// come from the server, as the test machine's log had them while its client was English -- the
/// Russian wording of those hasn't been seen yet.
const PRESENCE: [(&str, bool); 6] = [
    (" has joined your party.", true),
    (" has left the party.", false),
    (" присоединяется к вашей группе.", true),
    (" покидает группу.", false),
    (" has joined the area.", true),
    (" has left the area.", false),
];

/// What a `Client.txt` line says that the trade overlay cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    /// A whisper from another player.
    Whisper { from: String, body: String },
    /// Another player arrived: joined the party, or entered the area this character is in.
    Joined(String),
    /// Another player left the party or the area.
    Left(String),
}

/// The event in one line of `Client.txt`, `None` for everything else.
pub fn parse_chat_line(line: &str) -> Option<ChatEvent> {
    // After the header `2026/09/09 16:02:28 33995171 3ef23348 [INFO Client 31244] `.
    let (_, message) = line.split_once("] ")?;
    if let Some(rest) = WHISPER_FROM
        .iter()
        .find_map(|prefix| message.strip_prefix(prefix))
    {
        // A guild tag comes first when the sender is in a guild: `<TAG> Name: text`.
        let rest = rest
            .strip_prefix('<')
            .and_then(|tagged| tagged.split_once("> "))
            .map_or(rest, |(_, named)| named);
        let (from, body) = rest.split_once(": ")?;
        return Some(ChatEvent::Whisper {
            from: from.to_owned(),
            body: body.to_owned(),
        });
    }
    let system = message.strip_prefix(": ")?;
    PRESENCE.iter().find_map(|&(suffix, joined)| {
        let name = system.strip_suffix(suffix)?.to_owned();
        Some(if joined {
            ChatEvent::Joined(name)
        } else {
            ChatEvent::Left(name)
        })
    })
}

/// The language a buyer's trade site wrote the request in; replies go back in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestLanguage {
    English,
    Russian,
}

/// Where the item waits in the seller's stash: the tab's name and the item's cell, counted from 1
/// at the tab's top left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashSpot {
    pub tab: String,
    pub left: u32,
    pub top: u32,
}

/// A buyer's request, as the trade site worded it.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeRequest {
    pub buyer: String,
    /// The item as the buyer's site names it: in the seller's own language only if the buyer's
    /// site is in it too.
    pub item: String,
    /// The listed amount and the trade site's id of its currency (`exalted`, `divine`, `aug`).
    pub price: Option<(f64, String)>,
    pub league: String,
    pub stash: Option<StashSpot>,
    /// What the buyer added after the site's text: an offer, a question.
    pub note: Option<String>,
    pub language: RequestLanguage,
}

/// How the trade site words a request in one of its languages.
struct Wording {
    language: RequestLanguage,
    greeting: &'static str,
    /// Between the item and its price.
    priced: &'static str,
    /// Before the league.
    league: &'static str,
    /// The stash part, piece by piece: before the tab's name, between it and the column, between
    /// the column and the row, and after the row.
    tab: &'static str,
    column: &'static str,
    row: &'static str,
    end: &'static str,
}

const WORDINGS: [Wording; 2] = [
    Wording {
        language: RequestLanguage::English,
        greeting: "Hi, I would like to buy your ",
        priced: " listed for ",
        league: " in ",
        tab: " (stash tab \"",
        column: "\"; position: left ",
        row: ", top ",
        end: ")",
    },
    Wording {
        language: RequestLanguage::Russian,
        greeting: "Здравствуйте, хочу купить у вас ",
        priced: " за ",
        league: " в лиге ",
        tab: " (секция \"",
        column: "\"; позиция: ",
        row: " столбец, ",
        end: " ряд)",
    },
];

/// The request in a whisper `body` from `from`, if the trade site wrote it.
pub fn trade_request(from: &str, body: &str) -> Option<TradeRequest> {
    WORDINGS.iter().find_map(|wording| {
        let rest = body.strip_prefix(wording.greeting)?;
        // The stash part closes the site's text; the buyer may add their own after it.
        let (head, stash, note) = match rest.rfind(wording.tab) {
            Some(at) => {
                let (spot, note) = stash_spot(&rest[at + wording.tab.len()..], wording)?;
                (&rest[..at], Some(spot), note)
            }
            None => (rest, None, None),
        };
        let (listing, league) = head.rsplit_once(wording.league)?;
        let (item, price) = match listing.rsplit_once(wording.priced) {
            Some((item, price)) => (item, parse_price(price)),
            None => (listing, None),
        };
        Some(TradeRequest {
            buyer: from.to_owned(),
            item: item.to_owned(),
            price,
            league: league.to_owned(),
            stash,
            note,
            language: wording.language,
        })
    })
}

/// `3"; position: left 1, top 1) offer 2 ex` -> the spot and the buyer's note.
fn stash_spot(text: &str, wording: &Wording) -> Option<(StashSpot, Option<String>)> {
    let (tab, position) = text.split_once(wording.column)?;
    let (left, rest) = position.split_once(wording.row)?;
    let (top, note) = rest.split_once(wording.end)?;
    let spot = StashSpot {
        tab: tab.to_owned(),
        left: left.trim().parse().ok()?,
        top: top.trim().parse().ok()?,
    };
    let note = Some(note.trim())
        .filter(|note| !note.is_empty())
        .map(str::to_owned);
    Some((spot, note))
}

/// `1 aug`, `0.5 divine` -> the amount and the currency id.
fn parse_price(text: &str) -> Option<(f64, String)> {
    let (amount, currency) = text.split_once(' ')?;
    Some((amount.parse().ok()?, currency.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "2026/09/23 03:04:53 33995171 3ef23348 [INFO Client 31244] ";

    fn line(message: &str) -> String {
        format!("{HEADER}{message}")
    }

    #[test]
    fn whispers_and_arrivals_are_read_in_both_clients() {
        assert_eq!(
            parse_chat_line(&line("@От кого Fansiel: спасибо)")),
            Some(ChatEvent::Whisper {
                from: "Fansiel".to_owned(),
                body: "спасибо)".to_owned(),
            })
        );
        // The guild tag isn't part of the name; a colon inside the text stays in it.
        assert_eq!(
            parse_chat_line(&line("@From <GG> Buyer_1: price: 2 ex?")),
            Some(ChatEvent::Whisper {
                from: "Buyer_1".to_owned(),
                body: "price: 2 ex?".to_owned(),
            })
        );
        assert_eq!(
            parse_chat_line(&line(": Geladinhas has joined the area.")),
            Some(ChatEvent::Joined("Geladinhas".to_owned()))
        );
        assert_eq!(
            parse_chat_line(&line(": Brzytwa_ has left the area.")),
            Some(ChatEvent::Left("Brzytwa_".to_owned()))
        );
        assert_eq!(
            parse_chat_line(&line(": Покупатель присоединяется к вашей группе.")),
            Some(ChatEvent::Joined("Покупатель".to_owned()))
        );
        assert_eq!(
            parse_chat_line(&line(": Buyer покидает группу.")),
            Some(ChatEvent::Left("Buyer".to_owned()))
        );
        // Sent whispers, other channels and other system lines are nobody's request.
        for other in [
            "@Кому Fansiel: :)",
            "$Nyksserino: Хочу купить белые тяжелые пояса 6 exa",
            ": Вы присоединились к чату Торговли 820.",
            "[SCENE] Set Source [HideoutCanal]",
        ] {
            assert_eq!(parse_chat_line(&line(other)), None, "{other}");
        }
    }

    #[test]
    fn the_trade_sites_requests_are_read_in_both_languages() {
        let english = trade_request(
            "Cursedonme",
            "Hi, I would like to buy your Gale Coil Emerald Ring listed for 1 aug in Forbidden \
             Rites (stash tab \"3\"; position: left 1, top 1)",
        );
        assert_eq!(
            english,
            Some(TradeRequest {
                buyer: "Cursedonme".to_owned(),
                item: "Gale Coil Emerald Ring".to_owned(),
                price: Some((1.0, "aug".to_owned())),
                league: "Forbidden Rites".to_owned(),
                stash: Some(StashSpot {
                    tab: "3".to_owned(),
                    left: 1,
                    top: 1,
                }),
                note: None,
                language: RequestLanguage::English,
            })
        );
        let russian = trade_request(
            "IelIpablo",
            "Здравствуйте, хочу купить у вас Разящее Золотое кольцо льва за 1 transmute в лиге \
             Forbidden Rites (секция \"5\"; позиция: 10 столбец, 12 ряд)",
        )
        .expect("a request");
        assert_eq!(russian.item, "Разящее Золотое кольцо льва");
        assert_eq!(russian.price, Some((1.0, "transmute".to_owned())));
        assert_eq!(
            russian.stash,
            Some(StashSpot {
                tab: "5".to_owned(),
                left: 10,
                top: 12,
            })
        );
        assert_eq!(russian.language, RequestLanguage::Russian);
    }

    #[test]
    fn a_buyers_own_words_and_odd_names_stay_where_they_belong() {
        // A note after the site's text, a tab named with the site's own words, a fractional price
        // and a league with spaces.
        let request = trade_request(
            "Buyer",
            "Hi, I would like to buy your Tabula Rasa Simple Robe listed for 0.5 divine in HC \
             Forbidden Rites (stash tab \"sale in bulk\"; position: left 12, top 3) would you take \
             100 ex?",
        )
        .expect("a request");
        assert_eq!(request.item, "Tabula Rasa Simple Robe");
        assert_eq!(request.price, Some((0.5, "divine".to_owned())));
        assert_eq!(request.league, "HC Forbidden Rites");
        assert_eq!(
            request.stash.as_ref().map(|spot| spot.tab.as_str()),
            Some("sale in bulk")
        );
        assert_eq!(request.note.as_deref(), Some("would you take 100 ex?"));
        // No price listed: the whole head is the item.
        let unpriced = trade_request(
            "Buyer",
            "Hi, I would like to buy your Sapphire in Standard (stash tab \"~\"; position: left 2, \
             top 2)",
        )
        .expect("a request");
        assert_eq!((unpriced.item.as_str(), unpriced.price), ("Sapphire", None));
        // A plain whisper is no request.
        assert_eq!(trade_request("Fansiel", "могу 1 челюсть на 2 ребра"), None);
    }
}
