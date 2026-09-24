//! The price panel's league chip (`ui::panel::title_bar`): what it says, and the choices its menu
//! offers -- the ones the settings window's league chips offer -- each league named the way the
//! trade site in the interface language names it; and the public league whose market prices a
//! private one. Pure and not Windows-gated, so the native CI test pass covers it.

use trade_client::League;

use crate::settings::LeagueChoice;
use crate::tr;

/// League `id`'s name in `names` -- one trade site's league list, in that site's language -- or the
/// id itself for a league the list lacks: a typed private league, or a list that didn't load.
pub fn league_name<'a>(id: &'a str, names: &'a [League]) -> &'a str {
    names
        .iter()
        .find(|league| league.id == id)
        .map_or(id, |league| league.text.as_str())
}

/// What the chip says: `league` -- the one searches go to, as `LeagueChoice::resolve` gave it --
/// by its name in `names`, after «Авто · » (`Auto · `) while `choice` leaves the league to the
/// trade site.
pub fn chip_label(choice: &LeagueChoice, league: &str, names: &[League]) -> String {
    let name = league_name(league, names);
    match choice {
        LeagueChoice::Auto => tr!("Auto · {league}", league = name),
        LeagueChoice::Named(_) | LeagueChoice::Custom(_) => name.to_owned(),
    }
}

/// The menu's choices, each with its label, as the settings window offers them: «Авто · » with the
/// current league, every league `listed` (the trade site's ids, current first), then `current`
/// itself when it is neither -- a picked league the site no longer lists, or a typed one. Leagues
/// are named in `names`, as on the chip.
pub fn menu(
    current: &LeagueChoice,
    listed: &[String],
    names: &[League],
) -> Vec<(LeagueChoice, String)> {
    let auto = match listed.first() {
        Some(id) => tr!("Auto · {league}", league = league_name(id, names)),
        None => tr!("Auto").to_owned(),
    };
    let unlisted = match current {
        LeagueChoice::Named(id) if !listed.contains(id) => Some((current.clone(), id.clone())),
        LeagueChoice::Custom(name) => Some((
            current.clone(),
            tr!("Private league · {league}", league = name),
        )),
        LeagueChoice::Auto | LeagueChoice::Named(_) => None,
    };
    std::iter::once((LeagueChoice::Auto, auto))
        .chain(listed.iter().map(|id| {
            (
                LeagueChoice::Named(id.clone()),
                league_name(id, names).to_owned(),
            )
        }))
        .chain(unlisted)
        .collect()
}

/// Whether `league` is a private league: GGG names one "<name> (PL<number>)", which is how EE2
/// tells them apart (`Leagues.ts`, `isPrivateLeague`).
pub fn is_private(league: &str) -> bool {
    league
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once("(PL"))
        .is_some_and(|(_, number)| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The league whose exchange market and poe2scout prices stand for `league`'s: itself, unless
/// it's private. A private league's own exchange is too thin to price (GGG's record, 2026-09-24:
/// the busiest private league, "HC FRites League by Cardiff (PL86503)", had 74 exchange markets
/// in 12 hours against Forbidden Rites' 21 290, and never Divine Orbs for Exalted Orbs), and
/// poe2scout lists none, so it takes the prices of the public league it's most likely made from:
/// the current one, or its hardcore twin ("HC <league>") for a league named hardcore ("HC" or
/// "Hardcore" as a word). `listed`: the trade site's league ids, current first; without them a
/// private league stands for itself too.
pub fn market_league<'a>(league: &'a str, listed: &'a [String]) -> &'a str {
    let Some(current) = listed.first().filter(|_| is_private(league)) else {
        return league;
    };
    let hardcore = league
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case("hc") || word.eq_ignore_ascii_case("hardcore"));
    if !hardcore {
        return current;
    }
    listed
        .iter()
        .find(|id| id.strip_prefix("HC ") == Some(current.as_str()))
        .map_or(current, String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, with_lang};

    /// A trade site's league list: `(id, text)` pairs.
    fn site(leagues: &[(&str, &str)]) -> Vec<League> {
        leagues
            .iter()
            .map(|&(id, text)| League {
                id: id.to_owned(),
                text: text.to_owned(),
            })
            .collect()
    }

    /// Both sites' lists as they answered live on 2026-09-23 (Hardcore and its "Одна жизнь" left
    /// out): `ru` translates the challenge leagues and Standard, not the HC ones.
    fn international() -> Vec<League> {
        site(&[
            ("Forbidden Rites", "Forbidden Rites"),
            ("HC Forbidden Rites", "HC Forbidden Rites"),
            ("Runes of Aldur", "Runes of Aldur"),
            ("Standard", "Standard"),
        ])
    }

    fn russian() -> Vec<League> {
        site(&[
            ("Forbidden Rites", "Запретные ритуалы"),
            ("HC Forbidden Rites", "HC Forbidden Rites"),
            ("Runes of Aldur", "Руны Альдура"),
            ("Standard", "Стандарт"),
        ])
    }

    fn listed() -> Vec<String> {
        international()
            .into_iter()
            .map(|league| league.id)
            .collect()
    }

    fn named(id: &str) -> LeagueChoice {
        LeagueChoice::Named(id.to_owned())
    }

    #[test]
    fn the_chip_names_the_league_searched_as_the_interface_languages_site_does() {
        let aldur = named("Runes of Aldur");
        assert_eq!(
            chip_label(&aldur, "Runes of Aldur", &russian()),
            "Руны Альдура"
        );
        assert_eq!(
            chip_label(&aldur, "Runes of Aldur", &international()),
            "Runes of Aldur"
        );
        // Auto says so, beside the league the trade site made current.
        assert_eq!(
            chip_label(&LeagueChoice::Auto, "Forbidden Rites", &russian()),
            "Авто · Запретные ритуалы"
        );
        assert_eq!(
            chip_label(&LeagueChoice::Auto, "Forbidden Rites", &international()),
            "Авто · Forbidden Rites"
        );
        // A league the site leaves untranslated reads as the site writes it; a typed one, which
        // no list names, as typed.
        assert_eq!(
            chip_label(
                &named("HC Forbidden Rites"),
                "HC Forbidden Rites",
                &russian()
            ),
            "HC Forbidden Rites"
        );
        let typed = LeagueChoice::Custom("My League (PL12345)".to_owned());
        assert_eq!(
            chip_label(&typed, "My League (PL12345)", &russian()),
            "My League (PL12345)"
        );
    }

    #[test]
    fn the_menu_offers_auto_then_every_listed_league_as_the_chip_names_them() {
        let labels = |choices: Vec<(LeagueChoice, String)>| -> Vec<String> {
            choices.into_iter().map(|(_, label)| label).collect()
        };
        let choices = menu(&named("Standard"), &listed(), &russian());
        assert_eq!(
            choices.iter().map(|(choice, _)| choice).collect::<Vec<_>>(),
            [
                &LeagueChoice::Auto,
                &named("Forbidden Rites"),
                &named("HC Forbidden Rites"),
                &named("Runes of Aldur"),
                &named("Standard"),
            ]
        );
        assert_eq!(
            labels(choices),
            [
                "Авто · Запретные ритуалы",
                "Запретные ритуалы",
                "HC Forbidden Rites",
                "Руны Альдура",
                "Стандарт",
            ]
        );
        assert_eq!(
            labels(menu(&LeagueChoice::Auto, &listed(), &international())),
            [
                "Авто · Forbidden Rites",
                "Forbidden Rites",
                "HC Forbidden Rites",
                "Runes of Aldur",
                "Standard",
            ]
        );
    }

    #[test]
    fn the_menu_keeps_a_choice_the_site_does_not_list() {
        // A typed private league is offered while it is the choice -- the only place its name is
        // kept.
        let typed = LeagueChoice::Custom("My League (PL12345)".to_owned());
        assert_eq!(
            menu(&typed, &listed(), &russian()).last(),
            Some(&(typed.clone(), "Своя лига · My League (PL12345)".to_owned()))
        );
        assert!(
            !menu(&LeagueChoice::Auto, &listed(), &russian())
                .iter()
                .any(|(choice, _)| matches!(choice, LeagueChoice::Custom(_))),
            "no typed league to offer"
        );
        // A picked league that has ended stays on offer, by its id.
        let ended = named("Dawn of the Hunt");
        assert_eq!(
            menu(&ended, &listed(), &russian()).last(),
            Some(&(ended.clone(), "Dawn of the Hunt".to_owned()))
        );
        assert_eq!(
            menu(&ended, &listed(), &russian()).len(),
            listed().len() + 2
        );
    }

    #[test]
    fn an_english_interface_says_auto_and_private_league_in_english() {
        with_lang(Lang::English, || {
            assert_eq!(
                chip_label(&LeagueChoice::Auto, "Forbidden Rites", &international()),
                "Auto · Forbidden Rites"
            );
            let typed = LeagueChoice::Custom("My League (PL12345)".to_owned());
            let labels: Vec<String> = menu(&typed, &listed(), &international())
                .into_iter()
                .map(|(_, label)| label)
                .collect();
            assert_eq!(labels.first().unwrap(), "Auto · Forbidden Rites");
            assert_eq!(
                labels.last().unwrap(),
                "Private league · My League (PL12345)"
            );
            assert_eq!(menu(&typed, &[], &[])[0].1, "Auto");
        });
    }

    #[test]
    fn a_private_league_is_priced_by_the_public_league_it_is_made_from() {
        let listed = listed();
        // The owner's league, live 2026-09-24, and GGG's record's other private leagues then.
        assert_eq!(
            market_league("HC FRites League by Cardiff (PL86503)", &listed),
            "HC Forbidden Rites"
        );
        assert_eq!(
            market_league("The Forbidden Rites of Conflux (PL86619)", &listed),
            "Forbidden Rites"
        );
        assert_eq!(
            market_league("hardcore-only friends (PL1)", &listed),
            "HC Forbidden Rites"
        );
        // "hc" inside a word says nothing.
        assert_eq!(market_league("Chcoolers (PL2)", &listed), "Forbidden Rites");
        // A hardcore one whose current league has no twin listed takes the current league.
        let without_twin = vec!["Forbidden Rites".to_owned(), "Standard".to_owned()];
        assert_eq!(
            market_league("HC mates (PL3)", &without_twin),
            "Forbidden Rites"
        );
        // Public leagues, and typed names that aren't private, stand for themselves; so does a
        // private one before the list loads.
        for league in [
            "HC Forbidden Rites",
            "Runes of Aldur",
            "Standard",
            "My League",
            "My League (PL)",
            "My League (PL12a)",
            "My League (PL12345) x",
        ] {
            assert_eq!(market_league(league, &listed), league);
        }
        assert_eq!(
            market_league("My League (PL12345)", &[]),
            "My League (PL12345)"
        );
    }
}
