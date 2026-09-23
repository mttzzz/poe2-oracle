//! The onboarding tour's course, apart from how it is drawn (`ui::tour`): its stops in order,
//! what moves the player from one to the next -- Next, Back, Skip, and the real actions some stops
//! wait for -- and where a stop's card sits beside the spotlight's hole. Pure and platform-free,
//! so it is tested on every target.
//!
//! The tour has four steps, as its progress diamonds count them: the league in the settings
//! window, the first price check, the price panel that check opens, and the XP overlay. The
//! panel's step has four stops, one per part: the filters, the Search button, the listings and the
//! league in the title bar. A stop whose part the panel doesn't show -- the filters of an item the
//! exchange prices -- is passed over, both ways, and so is the stop the tour stands at when the
//! panel comes back without its part: another item checked, or the settings window closed.

use crate::settings::LeagueChoice;

/// The steps the progress diamonds count.
pub const STEPS: usize = 4;

/// One card of the tour, in the order they come.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Stop {
    /// The settings window's league select.
    League,
    /// Hover an item and press the hotkey: waits for a check to open the panel.
    PriceCheck,
    /// The panel's filter rows.
    Filters,
    /// The panel's Search button.
    Search,
    /// The panel's listings.
    Listings,
    /// The league select in the panel's title bar.
    PanelLeague,
    /// The XP overlay's line above the experience bar.
    XpLine,
}

/// The stops in order; a stop's place here is its discriminant.
const ORDER: [Stop; 7] = [
    Stop::League,
    Stop::PriceCheck,
    Stop::Filters,
    Stop::Search,
    Stop::Listings,
    Stop::PanelLeague,
    Stop::XpLine,
];

/// Where a stop is drawn: its card, and the spotlight around what it points at.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Host {
    /// Over the settings window.
    Settings,
    /// Over the price panel.
    Panel,
    /// In the tour's own window over the game.
    Screen,
}

impl Stop {
    /// The step it belongs to, counted from 1.
    pub fn step(self) -> usize {
        match self {
            Stop::League => 1,
            Stop::PriceCheck => 2,
            Stop::Filters | Stop::Search | Stop::Listings | Stop::PanelLeague => 3,
            Stop::XpLine => 4,
        }
    }

    pub fn host(self) -> Host {
        match self {
            Stop::League => Host::Settings,
            Stop::Filters | Stop::Search | Stop::Listings | Stop::PanelLeague => Host::Panel,
            Stop::PriceCheck | Stop::XpLine => Host::Screen,
        }
    }

    /// It moves on only when the player does what it asks: it offers no Next.
    pub fn waits(self) -> bool {
        self == Stop::PriceCheck
    }

    /// Nothing comes before it: it offers no Back.
    pub fn is_first(self) -> bool {
        self as usize == 0
    }

    /// Its Next finishes the tour.
    pub fn is_last(self) -> bool {
        self as usize == ORDER.len() - 1
    }
}

/// Something the player did in the app, which moves the tour on from a stop that asks for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Picked a league in the settings window.
    LeaguePicked,
    /// Closed the settings window: off to the game.
    SettingsClosed,
    /// A price check opened the panel.
    CheckShown,
    /// Started a search from the panel.
    SearchStarted,
    /// Closed the panel: done with it.
    PanelClosed,
}

/// What the tour watches of the app, to tell what the player just did.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Watched {
    pub settings_open: bool,
    pub panel_shown: bool,
    /// Checks shown so far.
    pub checks: u64,
    /// The league the settings name.
    pub league: LeagueChoice,
    /// A search is under way.
    pub searching: bool,
}

impl Watched {
    /// What the player did between this look at the app and `now`, in the order a player does
    /// it: picks the league, closes the settings, checks an item, searches, closes the panel. A
    /// check starts a search of its own, which is no search the player started; and the panel
    /// steps aside for the settings window (its ⚙), which is no closing of it.
    pub fn actions_to(&self, now: &Watched) -> Vec<Action> {
        let checked = now.checks > self.checks && now.panel_shown;
        let settings_opened = now.settings_open && !self.settings_open;
        [
            (now.league != self.league, Action::LeaguePicked),
            (
                self.settings_open && !now.settings_open,
                Action::SettingsClosed,
            ),
            (checked, Action::CheckShown),
            (
                !checked && now.searching && !self.searching,
                Action::SearchStarted,
            ),
            (
                self.panel_shown && !now.panel_shown && !settings_opened,
                Action::PanelClosed,
            ),
        ]
        .into_iter()
        .filter_map(|(happened, action)| happened.then_some(action))
        .collect()
    }
}

/// How a tour ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Past the last stop.
    Finished,
    /// The player skipped the rest.
    Skipped,
}

/// A tour under way: the stop it is at, or how it ended. Every move takes `shown`, which says
/// whether a stop can be shown now; the ones that can't are passed over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tour {
    stop: Stop,
    ended: Option<Outcome>,
}

impl Default for Tour {
    fn default() -> Tour {
        Tour::new()
    }
}

impl Tour {
    /// A tour at its first stop.
    pub fn new() -> Tour {
        Tour {
            stop: ORDER[0],
            ended: None,
        }
    }

    pub fn stop(self) -> Stop {
        self.stop
    }

    pub fn ended(self) -> Option<Outcome> {
        self.ended
    }

    /// Next: on to the next stop that can be shown, or the end after the last. A waiting stop
    /// offers no Next.
    pub fn next(&mut self, shown: impl Fn(Stop) -> bool) {
        if self.ended.is_none() && !self.stop.waits() {
            self.advance(shown);
        }
    }

    /// Back: to the stop before that can be shown -- none before the first.
    pub fn back(&mut self, shown: impl Fn(Stop) -> bool) {
        if self.ended.is_some() {
            return;
        }
        if let Some(stop) = ORDER[..self.stop as usize]
            .iter()
            .rev()
            .copied()
            .find(|&stop| shown(stop))
        {
            self.stop = stop;
        }
    }

    /// Skip: the tour ends where it is.
    pub fn skip(&mut self) {
        if self.ended.is_none() {
            self.ended = Some(Outcome::Skipped);
        }
    }

    /// The player did `action`: the stop that asks for it moves on -- the league's with a pick or
    /// with the settings window closed, the check's with a panel opened, Search's with a search --
    /// and a closed panel ends the panel's step. A panel shown again at one of its stops -- another
    /// check, or the settings window it stepped aside for closed -- passes over the stop when it
    /// no longer shows that part, as Next would: another item's panel may lack it. Any other stop
    /// stays.
    pub fn act(&mut self, action: Action, shown: impl Fn(Stop) -> bool) {
        if self.ended.is_some() {
            return;
        }
        match (self.stop, action) {
            (Stop::League, Action::LeaguePicked | Action::SettingsClosed)
            | (Stop::PriceCheck, Action::CheckShown)
            | (Stop::Search, Action::SearchStarted) => self.advance(shown),
            (stop, Action::CheckShown | Action::SettingsClosed)
                if stop.host() == Host::Panel && !shown(stop) =>
            {
                self.advance(shown);
            }
            (stop, Action::PanelClosed) if stop.host() == Host::Panel => {
                self.stop = Stop::XpLine;
            }
            _ => {}
        }
    }

    fn advance(&mut self, shown: impl Fn(Stop) -> bool) {
        match ORDER[self.stop as usize + 1..]
            .iter()
            .copied()
            .find(|&stop| shown(stop))
        {
            Some(stop) => self.stop = stop,
            None => self.ended = Some(Outcome::Finished),
        }
    }
}

/// A rectangle in a window, in its logical pixels.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Area {
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// `self` grown by `by` on every side.
    pub fn grown(self, by: f32) -> Area {
        Area {
            x: self.x - by,
            y: self.y - by,
            width: self.width + 2. * by,
            height: self.height + 2. * by,
        }
    }

    /// The part of `self` inside `view`; `None` when none of it is.
    pub fn within(self, view: Area) -> Option<Area> {
        let (x, y) = (self.x.max(view.x), self.y.max(view.y));
        let (right, bottom) = (
            self.right().min(view.right()),
            self.bottom().min(view.bottom()),
        );
        (right > x && bottom > y).then_some(Area {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

/// The four dimmed strips of `view` around `hole` (which lies inside it): the full width above
/// and below the hole, and the hole's height left and right of it. A strip with no room is empty.
pub fn dim_around(view: Area, hole: Area) -> [Area; 4] {
    [
        Area {
            height: hole.y - view.y,
            ..view
        },
        Area {
            y: hole.bottom(),
            height: view.bottom() - hole.bottom(),
            ..view
        },
        Area {
            y: hole.y,
            width: hole.x - view.x,
            height: hole.height,
            ..view
        },
        Area {
            x: hole.right(),
            y: hole.y,
            width: view.right() - hole.right(),
            height: hole.height,
        },
    ]
}

/// Where a card `card` (width, height) in size goes in `view`, `gap` from the hole and at least
/// `gap` from the view's edges -- VibeTools' order: below the hole when it fits there, else above
/// it, else right of it, else left of it, else across the hole on its roomier side, since the card
/// holds the tour's only buttons and must stay whole in the view. Without a hole, the view's
/// middle.
pub fn card_origin(view: Area, hole: Option<Area>, card: (f32, f32), gap: f32) -> (f32, f32) {
    let (width, height) = card;
    let clamp_x = |x: f32| x.min(view.right() - gap - width).max(view.x + gap);
    let clamp_y = |y: f32| y.min(view.bottom() - gap - height).max(view.y + gap);
    let Some(hole) = hole else {
        return (
            clamp_x(view.x + (view.width - width) / 2.),
            clamp_y(view.y + (view.height - height) / 2.),
        );
    };
    let centred_x = clamp_x(hole.x + (hole.width - width) / 2.);
    let centred_y = clamp_y(hole.y + (hole.height - height) / 2.);
    let below = view.bottom() - hole.bottom();
    let above = hole.y - view.y;
    let right = view.right() - hole.right();
    let left = hole.x - view.x;
    if below >= height + 2. * gap {
        (centred_x, hole.bottom() + gap)
    } else if above >= height + 2. * gap {
        (centred_x, hole.y - gap - height)
    } else if right >= width + 2. * gap {
        (hole.right() + gap, centred_y)
    } else if left >= width + 2. * gap {
        (hole.x - gap - width, centred_y)
    } else if below >= above {
        (centred_x, clamp_y(hole.bottom() + gap))
    } else {
        (centred_x, clamp_y(hole.y - gap - height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(_: Stop) -> bool {
        true
    }

    /// What the panel of an item the exchange prices shows: no filters, no Search.
    fn exchange_item(stop: Stop) -> bool {
        !matches!(stop, Stop::Filters | Stop::Search)
    }

    fn at(stop: Stop) -> Tour {
        Tour { stop, ended: None }
    }

    #[test]
    fn next_walks_the_stops_in_order_and_finishes_after_the_last() {
        let mut tour = Tour::new();
        assert_eq!(tour.stop(), Stop::League);
        tour.next(all);
        assert_eq!(tour.stop(), Stop::PriceCheck);
        tour.act(Action::CheckShown, all);
        let mut seen = vec![tour.stop()];
        while tour.ended().is_none() {
            tour.next(all);
            seen.push(tour.stop());
        }
        assert_eq!(
            seen,
            [
                Stop::Filters,
                Stop::Search,
                Stop::Listings,
                Stop::PanelLeague,
                Stop::XpLine,
                Stop::XpLine
            ]
        );
        assert_eq!(tour.ended(), Some(Outcome::Finished));
    }

    #[test]
    fn the_check_stop_takes_no_next_and_moves_on_when_a_check_opens_the_panel() {
        let mut tour = at(Stop::PriceCheck);
        tour.next(all);
        assert_eq!(tour, at(Stop::PriceCheck), "Next can't skip the check");
        tour.act(Action::SearchStarted, all);
        tour.act(Action::PanelClosed, all);
        assert_eq!(tour, at(Stop::PriceCheck), "only the check counts");
        tour.act(Action::CheckShown, all);
        assert_eq!(tour.stop(), Stop::Filters);
        tour.act(Action::CheckShown, all);
        assert_eq!(tour.stop(), Stop::Filters, "a second check keeps the stop");
    }

    #[test]
    fn back_returns_to_the_stop_before_and_stops_at_the_first() {
        let mut tour = at(Stop::Search);
        tour.back(all);
        assert_eq!(tour.stop(), Stop::Filters);
        tour.back(all);
        assert_eq!(tour.stop(), Stop::PriceCheck, "back to waiting for a check");
        tour.back(all);
        tour.back(all);
        assert_eq!(tour, at(Stop::League));
        let mut last = at(Stop::XpLine);
        last.back(all);
        assert_eq!(last.stop(), Stop::PanelLeague);
    }

    #[test]
    fn skip_ends_the_tour_from_any_stop_and_nothing_moves_it_after() {
        for stop in ORDER {
            let mut tour = at(stop);
            tour.skip();
            assert_eq!(tour.ended(), Some(Outcome::Skipped), "{stop:?}");
            tour.next(all);
            tour.back(all);
            tour.act(Action::CheckShown, all);
            tour.skip();
            assert_eq!(
                tour,
                Tour {
                    stop,
                    ended: Some(Outcome::Skipped)
                }
            );
        }
    }

    #[test]
    fn parts_the_panel_does_not_show_are_passed_over_both_ways() {
        let mut tour = at(Stop::PriceCheck);
        tour.act(Action::CheckShown, exchange_item);
        assert_eq!(tour.stop(), Stop::Listings);
        tour.back(exchange_item);
        assert_eq!(tour.stop(), Stop::PriceCheck);
        let mut nothing_on_the_panel = at(Stop::PriceCheck);
        nothing_on_the_panel.act(Action::CheckShown, |stop| stop.host() != Host::Panel);
        assert_eq!(nothing_on_the_panel.stop(), Stop::XpLine);
    }

    #[test]
    fn a_panel_back_without_the_stops_part_passes_it_over_and_one_with_it_keeps_it() {
        // The panel of an item it couldn't read: its title bar alone.
        let unread_item = |stop: Stop| stop.host() != Host::Panel || stop == Stop::PanelLeague;
        for action in [Action::CheckShown, Action::SettingsClosed] {
            for stop in [Stop::Filters, Stop::Search] {
                let mut tour = at(stop);
                tour.act(action, exchange_item);
                assert_eq!(tour, at(Stop::Listings), "{stop:?}, {action:?}");
            }
            let mut listings = at(Stop::Listings);
            listings.act(action, unread_item);
            assert_eq!(listings, at(Stop::PanelLeague), "{action:?}");
            for stop in [Stop::Search, Stop::Listings, Stop::PanelLeague] {
                let mut tour = at(stop);
                tour.act(action, all);
                assert_eq!(tour, at(stop), "resumed where it was: {stop:?}, {action:?}");
            }
        }
    }

    #[test]
    fn the_league_stop_moves_on_with_a_pick_or_the_settings_closed() {
        for action in [Action::LeaguePicked, Action::SettingsClosed] {
            let mut tour = Tour::new();
            tour.act(action, all);
            assert_eq!(tour.stop(), Stop::PriceCheck, "{action:?}");
        }
        let mut tour = Tour::new();
        tour.act(Action::CheckShown, all);
        tour.act(Action::PanelClosed, all);
        assert_eq!(tour, Tour::new());
    }

    #[test]
    fn a_search_moves_on_from_search_only_and_a_closed_panel_ends_the_panel_step() {
        let mut filters = at(Stop::Filters);
        filters.act(Action::SearchStarted, all);
        assert_eq!(filters, at(Stop::Filters));
        let mut search = at(Stop::Search);
        search.act(Action::SearchStarted, all);
        assert_eq!(search.stop(), Stop::Listings);
        for stop in [
            Stop::Filters,
            Stop::Search,
            Stop::Listings,
            Stop::PanelLeague,
        ] {
            let mut tour = at(stop);
            tour.act(Action::PanelClosed, all);
            assert_eq!(tour, at(Stop::XpLine), "{stop:?}");
        }
        let mut last = at(Stop::XpLine);
        last.act(Action::PanelClosed, all);
        last.act(Action::LeaguePicked, all);
        assert_eq!(last, at(Stop::XpLine));
    }

    #[test]
    fn every_stop_counts_toward_one_of_the_steps() {
        let steps: Vec<usize> = ORDER.iter().map(|stop| stop.step()).collect();
        assert_eq!(steps, [1, 2, 3, 3, 3, 3, STEPS]);
        assert!(Stop::League.is_first() && !Stop::PriceCheck.is_first());
        assert!(Stop::XpLine.is_last() && !Stop::PanelLeague.is_last());
    }

    const VIEW: Area = Area {
        x: 0.,
        y: 0.,
        width: 500.,
        height: 1000.,
    };
    const CARD: (f32, f32) = (300., 200.);

    fn hole(y: f32, height: f32) -> Area {
        Area {
            x: 20.,
            y,
            width: 460.,
            height,
        }
    }

    #[test]
    fn the_card_goes_below_the_hole_then_above_then_beside() {
        assert_eq!(
            card_origin(VIEW, Some(hole(100., 40.)), CARD, 8.),
            (100., 148.),
            "below, centred under the hole"
        );
        assert_eq!(
            card_origin(VIEW, Some(hole(760., 200.)), CARD, 8.),
            (100., 552.),
            "above when below is too short"
        );
        let wide = Area {
            x: 0.,
            y: 0.,
            width: 1100.,
            height: 400.,
        };
        let tall = Area {
            x: 100.,
            y: 20.,
            width: 300.,
            height: 360.,
        };
        assert_eq!(
            card_origin(wide, Some(tall), CARD, 8.),
            (408., 100.),
            "right"
        );
        let right_edge = Area { x: 700., ..tall };
        assert_eq!(
            card_origin(wide, Some(right_edge), CARD, 8.),
            (392., 100.),
            "left"
        );
    }

    #[test]
    fn a_card_with_no_room_around_the_hole_stays_whole_in_the_view() {
        let (x, y) = card_origin(VIEW, Some(hole(150., 700.)), CARD, 8.);
        assert_eq!((x, y), (100., 792.), "over the hole's bottom, in the view");
        let (x, y) = card_origin(VIEW, Some(hole(-50., 1100.)), CARD, 8.);
        assert!((8. ..=192.).contains(&x) && (8. ..=792.).contains(&y));
        assert_eq!(
            card_origin(VIEW, None, CARD, 8.),
            (100., 400.),
            "no hole: the middle"
        );
        let narrow = Area {
            width: 250.,
            ..VIEW
        };
        assert_eq!(
            card_origin(narrow, None, CARD, 8.).0,
            8.,
            "wider than the view: its left edge shows"
        );
    }

    #[test]
    fn the_dimmed_strips_and_the_hole_tile_the_view() {
        let hole = Area {
            x: 50.,
            y: 120.,
            width: 200.,
            height: 60.,
        };
        let strips = dim_around(VIEW, hole);
        let area = |a: Area| a.width.max(0.) * a.height.max(0.);
        let covered: f32 = strips.iter().map(|&strip| area(strip)).sum::<f32>() + area(hole);
        assert_eq!(covered, area(VIEW));
        for strip in strips {
            assert_eq!(strip.within(hole), None, "{strip:?} overlaps the hole");
        }
        let at_the_top = dim_around(VIEW, Area { y: 0., ..hole });
        assert_eq!(at_the_top[0].height, 0., "nothing above a hole at the top");
    }

    #[test]
    fn an_area_grows_and_clips() {
        let area = Area {
            x: 10.,
            y: 10.,
            width: 20.,
            height: 20.,
        };
        assert_eq!(
            area.grown(4.),
            Area {
                x: 6.,
                y: 6.,
                width: 28.,
                height: 28.
            }
        );
        assert_eq!(
            area.within(Area {
                x: 20.,
                y: 0.,
                width: 100.,
                height: 15.
            }),
            Some(Area {
                x: 20.,
                y: 10.,
                width: 10.,
                height: 5.
            })
        );
        assert_eq!(area.within(Area { x: 40., ..area }), None);
    }

    #[test]
    fn what_the_player_did_is_read_from_two_looks_at_the_app() {
        let panel = Watched {
            panel_shown: true,
            checks: 1,
            ..Watched::default()
        };
        assert_eq!(panel.actions_to(&panel), [], "nothing changed");
        let checked = Watched {
            checks: 2,
            searching: true,
            ..panel.clone()
        };
        assert_eq!(
            panel.actions_to(&checked),
            [Action::CheckShown],
            "a check's own search is no search the player started"
        );
        let searched = Watched {
            searching: true,
            ..panel.clone()
        };
        assert_eq!(panel.actions_to(&searched), [Action::SearchStarted]);
        assert_eq!(
            searched.actions_to(&panel),
            [],
            "a search ending is nothing"
        );
        let hidden_check = Watched {
            checks: 2,
            panel_shown: false,
            ..panel.clone()
        };
        assert_eq!(
            panel.actions_to(&hidden_check),
            [Action::PanelClosed],
            "a check counts only with the panel shown"
        );
        let stepped_aside = Watched {
            settings_open: true,
            panel_shown: false,
            ..panel.clone()
        };
        assert_eq!(
            panel.actions_to(&stepped_aside),
            [],
            "the panel steps aside for the settings window: not closed"
        );
        let settings = Watched {
            settings_open: true,
            ..Watched::default()
        };
        let picked = Watched {
            league: LeagueChoice::Named("Standard".to_owned()),
            ..settings.clone()
        };
        assert_eq!(settings.actions_to(&picked), [Action::LeaguePicked]);
        let picked_and_closed = Watched {
            settings_open: false,
            ..picked
        };
        assert_eq!(
            settings.actions_to(&picked_and_closed),
            [Action::LeaguePicked, Action::SettingsClosed],
            "in the order a player does them"
        );
    }
}
