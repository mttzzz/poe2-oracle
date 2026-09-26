//! Experience tracking for the XP overlay: how fast the character levels, how much play is left
//! until the next level, how long the current map has taken, and whether the player is playing
//! at all.
//!
//! Pure and not Windows-gated, so the native test pass covers all of it. The Windows side only
//! feeds it -- `platform::xp_bar` captures the bar's pixels, `platform::client_log` tails the game
//! log -- and `ui::xp_overlay` only renders [`XpStatus`]:
//!
//! - [`XpBarGeometry`] and [`read_fill`]: where PoE2 draws its experience bar and how the bar's
//!   pixels read as the fraction of the level already earned.
//! - [`parse_log_line`]: the `Client.txt` lines that say the character levelled up, entered an
//!   area instance or an ascendancy trial, or logged out.
//! - [`XpTracker`]: the rate (levels per hour of play, over a window the settings pick), the time
//!   to the next level, whether the player is playing or paused ([`Activity`]), and the current
//!   or last map run ([`MapStatus`]): its time, its experience, and the average time of the maps
//!   before it. With the game out of the front it asks for fewer looks at the bar
//!   ([`XpTracker::unattended_look_due`]) and carries its reading over the samples between
//!   ([`BarLook::Skipped`]).
//! - [`Word`] and [`percent_words`], [`rate_words`], [`level_parts`], [`map_words`]: what the
//!   overlay's plates say, word by word, in the interface language (`crate::i18n`), in full or in
//!   the shorter [`Wording`] a rail too narrow for the full one gets.
//! - [`XpTracker::carry`], [`XpTracker::carried`] and [`XpTracker::catch_up`]: the tracker across
//!   an update's restart, so the plates carry on where they were instead of starting over.

use std::borrow::Cow;
use std::ops::{Range, RangeInclusive};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::overlay_layout::PhysicalRect;
use crate::{i18n, tr};

// --- Where the bar is ---------------------------------------------------------------------------
//
// Measured live 2026-09-22 on the test machine's 3840x2160 borderless game (the bar at ~65 %,
// fixture `tests/fixtures/xp_bar_4k_65pct.rgb`): a bar of 20 segments centred under the game at
// the bottom edge of the HUD, its segments separated by 19 ornamental ticks whose dark stems hang
// below the fill. PoE2 scales its HUD with the game's height -- the assumption
// `overlay_layout::panel_rect` makes for the side panels -- so every length below is in pixels of
// a 2160-row game and gets multiplied by `height / 2160`; horizontally the bar is centred on the
// client area.

const REFERENCE_HEIGHT: f64 = 2160.0;
/// Half the fill track's width: filled pixels start at x = 1154, so the track ends at 2686 if it
/// is symmetric -- its right end hasn't been seen filled; empty, it is the same grey as the frame.
const FILL_HALF_WIDTH: f64 = 766.0;
/// Half the captured width: the fill track and a pixel or two of frame on each side.
const CAPTURE_HALF_WIDTH: f64 = 768.0;
/// Tick to tick, fitted over all 19 stems (residuals under 0.6 px); the middle tick sits on the
/// centre line.
const TICK_SPACING: f64 = 77.27;
const TICK_COUNT: usize = 19;
/// How far a tick's ornament reaches into the fill band on either side: those columns show the
/// ornament, not the fill.
const TICK_HALF_WIDTH: f64 = 5.0;
/// The fill band's core rows (2141-2145 of 2160) as distances from the client area's bottom
/// edge: cream-to-orange where filled, neutral grey where empty. The rows around it blend into
/// the frame.
const FILL_BAND: (f64, f64) = (19.0, 14.0);
/// The rows just below the fill (2147-2150), where each tick's stem is a dark notch in the frame.
const STEM_BAND: (f64, f64) = (13.0, 9.0);
/// The smallest game height read at all: at 720 rows the fill band is already a single row and a
/// tick stem a single column; any smaller and the bar can't be told apart from other pixels.
const MIN_HEIGHT: i32 = 720;

// Pixel tests, all with wide margins on the measured bar: every stem is at most 0.45 of its
// surroundings' brightness; filled columns have (R-B)/R of 0.31-0.34 with R >= 138, empty ones
// -0.03-0.02 with R of 46-62.
const MIN_TICKS_SEEN: usize = 17;
const STEM_MAX_RATIO: f64 = 0.7;
const STEM_MIN_DEPTH: f64 = 6.0;
const MAX_UNKNOWN_SHARE: f64 = 0.1;
const MAX_ERROR_SHARE: f64 = 0.03;

/// Where the experience bar is in a game window of a given size.
#[derive(Debug, Clone, PartialEq)]
pub struct XpBarGeometry {
    /// The screen rect to capture for [`read_fill`], physical pixels: the fill band and the tick
    /// stems below it, the full width of the bar.
    pub capture: PhysicalRect,
    /// Game pixels per reference pixel: 1.0 for a 2160-row game.
    scale: f64,
    /// Capture-local, in continuous coordinates (pixel `i` spans `[i, i + 1)`).
    fill_start: f64,
    fill_end: f64,
    ticks: [f64; TICK_COUNT],
    /// Capture-local rows.
    fill_rows: Range<usize>,
    stem_rows: Range<usize>,
}

impl XpBarGeometry {
    /// The bar in a game whose client area is `client` (physical pixels); `None` for a game too
    /// small to read, or too narrow for the whole bar -- the capture never reaches outside the
    /// game's client area.
    pub fn for_client(client: PhysicalRect) -> Option<Self> {
        if client.height < MIN_HEIGHT {
            return None;
        }
        let scale = f64::from(client.height) / REFERENCE_HEIGHT;
        let centre = f64::from(client.x) + f64::from(client.width) / 2.0;
        let bottom = f64::from(client.y) + f64::from(client.height);
        let left = (centre - CAPTURE_HALF_WIDTH * scale).floor();
        let right = (centre + CAPTURE_HALF_WIDTH * scale).ceil();
        let fill = rows_between(bottom - FILL_BAND.0 * scale, bottom - FILL_BAND.1 * scale);
        let stems = rows_between(bottom - STEM_BAND.0 * scale, bottom - STEM_BAND.1 * scale);
        let top = fill.start;
        let capture = PhysicalRect {
            x: left as i32,
            y: top,
            width: (right - left) as i32,
            height: stems.end - top,
        };
        if capture.x < client.x || capture.x + capture.width > client.x + client.width {
            return None;
        }
        let local = |rows: Range<i32>| (rows.start - top) as usize..(rows.end - top) as usize;
        Some(Self {
            capture,
            scale,
            fill_start: centre - FILL_HALF_WIDTH * scale - left,
            fill_end: centre + FILL_HALF_WIDTH * scale - left,
            ticks: std::array::from_fn(|i| centre + (i as f64 - 9.0) * TICK_SPACING * scale - left),
            fill_rows: local(fill),
            stem_rows: local(stems),
        })
    }
}

/// The rows whose centres lie in `[from, to)`, at least one.
fn rows_between(from: f64, to: f64) -> Range<i32> {
    let start = (from - 0.5).ceil() as i32;
    let end = ((to - 0.5).ceil() as i32).max(start + 1);
    start..end
}

/// The fraction of the level the bar shows, from the pixels of `geometry.capture` -- 32-bit BGRA
/// rows, top to bottom, as a `BI_RGB` DIB section holds them. `None` unless the bar is
/// unmistakably what's on screen: nearly all tick stems in place, nearly every column of the fill
/// track either the fill's warm colour or the empty track's grey, and the filled columns a
/// prefix. A panel, tooltip or loading screen over the bar fails those checks.
pub fn read_fill(geometry: &XpBarGeometry, bgra: &[u8]) -> Option<f64> {
    let width = usize::try_from(geometry.capture.width).ok()?;
    let height = usize::try_from(geometry.capture.height).ok()?;
    if bgra.len() != width * height * 4 {
        return None;
    }
    let scale = geometry.scale;
    let stems: Vec<f64> = band_average(bgra, width, &geometry.stem_rows)
        .into_iter()
        .map(luma)
        .collect();
    let ticks_seen = geometry
        .ticks
        .iter()
        .filter(|&&tick| stem_visible(&stems, tick, scale))
        .count();
    if ticks_seen < MIN_TICKS_SEEN {
        return None;
    }

    // Every track column clear of the ticks, left to right: `true` if filled.
    let mut columns = Vec::with_capacity(width);
    let mut unknown = 0usize;
    for (column, colour) in band_average(bgra, width, &geometry.fill_rows)
        .into_iter()
        .enumerate()
    {
        let centre = column as f64 + 0.5;
        if centre < geometry.fill_start
            || centre >= geometry.fill_end
            || geometry
                .ticks
                .iter()
                .any(|tick| (centre - tick).abs() < TICK_HALF_WIDTH * scale)
        {
            continue;
        }
        match classify(colour) {
            Some(filled) => columns.push((column, filled)),
            None => unknown += 1,
        }
    }
    if columns.is_empty() || unknown as f64 > MAX_UNKNOWN_SHARE * (columns.len() + unknown) as f64 {
        return None;
    }

    // The fill is a prefix of the track: take the split the fewest columns contradict.
    let filled_total = columns.iter().filter(|(_, filled)| *filled).count();
    let (mut split, mut errors) = (0, filled_total);
    let (mut empty_before, mut filled_before) = (0, 0);
    for (index, &(_, filled)) in columns.iter().enumerate() {
        if filled {
            filled_before += 1;
        } else {
            empty_before += 1;
        }
        let contradicted = empty_before + filled_total - filled_before;
        if contradicted < errors {
            (split, errors) = (index + 1, contradicted);
        }
    }
    if errors as f64 > MAX_ERROR_SHARE * columns.len() as f64 {
        return None;
    }
    let boundary = match split {
        0 => geometry.fill_start,
        // A bar filled end to end never shows in play: at 100 % the level wraps. Something over
        // the bar reads that way -- live on 2026-09-24, twice for 5-20 s in the hideout -- and
        // taken at its word it gains the rest of the level, then loses it again.
        n if n == columns.len() => return None,
        // Between the last filled and the first empty column -- mid-gap when a tick hides it.
        n => (columns[n - 1].0 + 1 + columns[n].0) as f64 / 2.0,
    };
    Some(
        ((boundary - geometry.fill_start) / (geometry.fill_end - geometry.fill_start))
            .clamp(0.0, 1.0),
    )
}

/// Per-column mean `[R, G, B]` over `rows` of a BGRA image `width` pixels wide.
fn band_average(bgra: &[u8], width: usize, rows: &Range<usize>) -> Vec<[f64; 3]> {
    let count = rows.len() as f64;
    (0..width)
        .map(|column| {
            let mut sum = [0.0; 3];
            for row in rows.clone() {
                let pixel = &bgra[(row * width + column) * 4..][..4];
                sum[0] += f64::from(pixel[2]);
                sum[1] += f64::from(pixel[1]);
                sum[2] += f64::from(pixel[0]);
            }
            sum.map(|channel| channel / count)
        })
        .collect()
}

fn luma([r, g, b]: [f64; 3]) -> f64 {
    0.299 * r + 0.587 * g + 0.114 * b
}

/// `Some(true)` for the fill's warm cream/orange, `Some(false)` for the empty track's neutral
/// grey, `None` for anything else (whatever covers the bar). Relative to the red channel, so a
/// dimmer UI brightness setting still reads.
fn classify([r, _, b]: [f64; 3]) -> Option<bool> {
    if r >= 60.0 && r - b >= 0.15 * r {
        Some(true)
    } else if (20.0..=110.0).contains(&r) && (r - b).abs() <= 0.08 * r + 4.0 {
        Some(false)
    } else {
        None
    }
}

/// Whether the tick centred at `tick` has its dark stem: the darkest column within 1.5 px of it
/// well below the columns 5-8 px to either side.
fn stem_visible(stems: &[f64], tick: f64, scale: f64) -> bool {
    let columns = |from: f64, to: f64| {
        let first = (from - 0.5).ceil().max(0.0) as usize;
        let last = (to - 0.5).floor().min(stems.len() as f64 - 1.0);
        if last < first as f64 {
            &stems[0..0]
        } else {
            &stems[first..=last as usize]
        }
    };
    let Some(stem) = columns(tick - 1.5 * scale, tick + 1.5 * scale)
        .iter()
        .copied()
        .reduce(f64::min)
    else {
        return false;
    };
    let mut around: Vec<f64> = columns(tick - 8.0 * scale, tick - 5.0 * scale)
        .iter()
        .chain(columns(tick + 5.0 * scale, tick + 8.0 * scale))
        .copied()
        .collect();
    if around.is_empty() {
        return false;
    }
    around.sort_by(f64::total_cmp);
    let base = around[around.len() / 2];
    stem < STEM_MAX_RATIO * base && base - stem >= STEM_MIN_DEPTH
}

// --- What the game log says ---------------------------------------------------------------------

/// A `Client.txt` line that matters to the tracker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    /// A character reached `level`.
    LevelUp { character: String, level: u32 },
    /// The character entered an area instance: the area's id (`MapEpitaph`, `HideoutCanal`) and
    /// the instance's seed. Going back into an instance logs its seed again -- live, `MapEpitaph`
    /// 2266921739 at 21:50:20 and again at 21:50:36 after a trip to the hideout -- so the seed
    /// tells a return from a new map.
    AreaEntered { area: String, seed: u64 },
    /// The area just entered is an ascendancy trial's (`TRIAL_SCENES`): its scene line follows
    /// the area line, within a second, and comes again whenever the world map is closed there.
    TrialEntered,
    /// The scene went `(unknown)`: the client is back at character select -- if a freshly
    /// generated area follows, the next login -- or the scene blanked for a moment and comes back
    /// named, with no new area. In the test machine's 15-month log 526 of the 653 such lines led
    /// to a generated area, 86 straight back to a named scene (5 s later, the same hideout, on
    /// 2026-09-23 18:20), and 41 ended the log (the game closed).
    SceneLost,
    /// The scene is named again: whatever [`LogEvent::SceneLost`] said, the character is in the
    /// world. Any name but `(null)`, the blank between an area line and its scene.
    SceneNamed,
}

/// Parses one `Client.txt` line, line ending already removed. The formats, from the test
/// machine's Russian client log (verified 2026-09-22) and, for the English level-up, EE2's
/// `LOG_LEVEL_UP` and its real-log fixture `specs/web/FullCampaign.txt`:
///
/// ```text
/// 2026/09/22 18:48:06 4189156 3ef23348 [INFO Client 19772] : mttzzz_merc_next (Легионер каменитов) достигает 38 уровня
/// 2025/12/12 13:03:36 1004610062 3ef232c2 [INFO Client 1157464] : HolyMolyThisIsCharName (Mercenary) is now level 2
/// 2026/09/22 18:50:44 4347046 2caa229f [DEBUG Client 19772] Generating level 44 area "G3_town" with seed 1
/// 2026/09/18 13:48:29 24486437 7fbd1225 [INFO Client 14580] [SCENE] Set Source [Испытание Хаоса]
/// 2026/09/22 21:35:24 14225828 7fbd1225 [INFO Client 31244] [SCENE] Set Source [(unknown)]
/// ```
///
/// The area line is the same in every client language; a scene line names the area in the
/// client's language, and `(unknown)` is the scene at client start and at character select --
/// and, for a moment, now and then in the world.
pub fn parse_log_line(line: &str) -> Option<LogEvent> {
    let (_, rest) = line.split_once(" [")?;
    let (header, message) = rest.split_once("] ")?;
    if !header.contains(" Client ") {
        return None;
    }
    // System messages start with ": "; chat lines start with the speaker's name or a channel
    // sigil, so a player can't fake a level-up.
    if let Some(system) = message.strip_prefix(": ") {
        return parse_level_up(system);
    }
    if let Some(generating) = message.strip_prefix("Generating level ") {
        let (_, area) = generating.split_once(" area \"")?;
        let (area, seed) = area.split_once("\" with seed ")?;
        return Some(LogEvent::AreaEntered {
            area: area.to_owned(),
            seed: seed.parse().ok()?,
        });
    }
    let scene = message
        .strip_prefix("[SCENE] Set Source [")?
        .strip_suffix(']')?;
    match scene {
        "(unknown)" => Some(LogEvent::SceneLost),
        "(null)" => None,
        scene if TRIAL_SCENES.contains(&scene) => Some(LogEvent::TrialEntered),
        _ => Some(LogEvent::SceneNamed),
    }
}

/// [`parse_log_line`], with when the line was written: its third field, the client's
/// millisecond tick -- `GetTickCount`, milliseconds since Windows started, which wraps every
/// 49.7 days. Verified 2026-09-24 on the test machine: the line at 09:39:52 said 144087359, and
/// `GetTickCount` read 145499812 at 10:03:24.71, 23:32.45 later by both. `None` for a line whose
/// third field isn't a tick.
pub fn parse_timed_log_line(line: &str) -> Option<(Option<u32>, LogEvent)> {
    let event = parse_log_line(line)?;
    let tick = line.split(' ').nth(2).and_then(|field| field.parse().ok());
    Some((tick, event))
}

/// When a line with client tick `tick` ([`parse_timed_log_line`]) was written, on a clock of time
/// since Windows started that reads `now` -- `GetTickCount64`, the overlay's clock. The tick has
/// 32 bits, so its age is taken modulo 2^32 ms; a line from before a reboot comes out anywhere
/// before `now`, but a login always follows it, and the tracker forgets the past at a login. A
/// line without a tick counts as written `now`.
pub fn log_time(tick: Option<u32>, now: Duration) -> Duration {
    let Some(tick) = tick else {
        return now;
    };
    let now_ms = u64::try_from(now.as_millis()).unwrap_or(u64::MAX);
    // Truncating to the tick's 32 bits is the point: both count the same milliseconds.
    let age = u64::from((now_ms as u32).wrapping_sub(tick));
    Duration::from_millis(now_ms.saturating_sub(age))
}

/// The scene names of the ascendancy trials' areas in the Russian and the English client: the
/// Trial of the Sekhemas (its altar room `G2_13` and its floors `Sanctum_*`) and the Trial of
/// Chaos (`G3_10`). The Russian ones name every entry into those areas in the test machine's
/// 15-month log. The English ones are the areas' names in RePoE's PoE2 `world_areas`, which is
/// how an English scene line names its area: 94 of the areas in EE2's English
/// `FullCampaign.txt`, all but the two RePoE doesn't list. The Act 4 campaign area `G4_4_3`,
/// «Испытание предков» or Trial of the Ancestors, is not a trial.
const TRIAL_SCENES: [&str; 4] = [
    "Испытание Сехем",
    "Испытание Хаоса",
    "Trial of the Sekhemas",
    "The Trial of Chaos",
];

/// `<name> (<class>) достигает <n> уровня` / `<name> (<class>) is now level <n>`.
fn parse_level_up(message: &str) -> Option<LogEvent> {
    let (who, level) = message
        .strip_suffix(" уровня")
        .and_then(|text| text.rsplit_once(" достигает "))
        .or_else(|| message.rsplit_once(" is now level "))?;
    let (character, class) = who.split_once(" (")?;
    if !class.ends_with(')') {
        return None;
    }
    Some(LogEvent::LevelUp {
        character: character.to_owned(),
        level: level.parse().ok()?,
    })
}

/// Towns (`G1_town`, `C_G2_town`, `P1_Town`, `G_Endgame_Town`) and hideouts (`HideoutCanal`):
/// every monster-free area id in the test machine's 15-month log. `MapHideout*_Claimable` is the
/// map a hideout is found in, and `Delirium_Act1Town` a league encounter -- both have monsters.
fn is_town(area: &str) -> bool {
    area.starts_with("Hideout") || area.ends_with("_town") || area.ends_with("_Town")
}

/// Map instances: every endgame map's area id starts with `Map` (`MapEpitaph`), the map a hideout
/// is found in (`MapHideout*_Claimable`) included.
fn is_map(area: &str) -> bool {
    area.starts_with("Map")
}

// --- Rate and time to level ---------------------------------------------------------------------

/// Readable samples this close together count as continuous play; the sampler runs every 2 s.
const MAX_SAMPLE_GAP: Duration = Duration::from_secs(6);
/// Longer than this without a readable bar (character select, alt-tab, a long look at the
/// passive tree) and the next reading starts a fresh baseline instead of crediting whatever
/// changed meanwhile -- it may not even be the same character.
const REBASE_GAP: Duration = Duration::from_secs(60);
/// Play time stops counting this long after the last gain, so an idle player in a map doesn't
/// dilute the rate; and once the player has gone this long without a gain or a change of area,
/// the overlay shows a pause. Generous: at level 95+ a pixel of the 4K bar takes tens of seconds
/// of mapping.
const IDLE_AFTER: Duration = Duration::from_secs(5 * 60);
/// The rate window unless the settings pick another: play ten minutes ago weighs half as much as
/// play now. A few maps, so one lucky pack doesn't swing the rate, and a change of farming
/// strategy shows within a quarter of an hour.
const DEFAULT_RATE_WINDOW_MINUTES: u16 = 10;
/// The rate windows the settings may pick, clamped rather than trusted: a minute is the last few
/// packs, two hours about a whole session. A zero half-life would turn every weight into NaN, and
/// one of days would never let the rate move.
const RATE_WINDOW_MINUTES: RangeInclusive<u16> = 1..=120;
/// No rate until this much play has been counted. Unweighted: with a one-minute window the
/// weighted play time never gets past ~87 s (the half-life over ln 2).
const MIN_RATE_TIME: Duration = Duration::from_secs(2 * 60);
/// A reading this far below the best since the last rebase is a real loss (the death penalty),
/// not reading noise: ~6 px of the 4K bar.
const DROP_THRESHOLD: f64 = 0.004;
/// A drop by more than half a level can only be a level-up: the death penalty costs a fraction
/// of that (10 % of a level in PoE2 -- not verified live).
const WRAP_DROP: f64 = 0.5;
/// A single step up this big (5 % of a level between two readings, a fraction of a second apart)
/// is worth a log line: real play earns that only from a big kill, and a misread bar that comes
/// back looks the same.
const BIG_GAIN: f64 = 0.05;
/// How far apart a logged level-up and the bar's wrap may be and still be the same level-up.
const LEVEL_UP_MATCH: Duration = Duration::from_secs(30);
/// How often the bar is looked at while the game is out of the front, where no experience comes
/// in without play: its last reading stands for it at the samples between ([`BarLook::Skipped`]).
/// A change still shows within two looks -- the median filter's due, the second of them at the
/// very next sample -- so a level-up's wrap, gained by minions left fighting, meets its log line
/// well within `LEVEL_UP_MATCH`; and a reading is never near `REBASE_GAP` old.
pub const UNATTENDED_LOOK_EVERY: Duration = Duration::from_secs(10);

/// A sample's look at the bar ([`XpTracker::on_sample`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BarLook {
    /// It read as this fraction of the level ([`read_fill`]).
    Read(f64),
    /// It couldn't be read: nowhere on screen, covered, a screen without the HUD.
    Unreadable,
    /// It wasn't looked at, the game being out of the front ([`XpTracker::unattended_look_due`]):
    /// it's taken to read as it did at the last look.
    Skipped,
}

/// What the overlay shows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct XpStatus {
    /// The fraction of the current level already earned.
    pub fraction: Option<f64>,
    /// Levels earned per hour of play (0.124 = 12.4 % of a level per hour). A pause doesn't
    /// change it: it stays the rate of the play before.
    pub rate_per_hour: Option<f64>,
    /// The character's current level, once the log has named it since the last login.
    pub level: Option<u32>,
    /// Whether the player is playing, or since when they haven't been.
    pub activity: Activity,
    /// The current map run, once the character has entered a map since the last login, as far
    /// back as the log's tail read at start goes -- the last one, once the character is done with
    /// it.
    pub map: Option<MapStatus>,
}

impl XpStatus {
    /// Play time left until the next level at the current rate.
    pub fn time_to_level(&self) -> Option<Duration> {
        let rate = self.rate_per_hour.filter(|rate| *rate > 0.0)?;
        Duration::try_from_secs_f64((1.0 - self.fraction?) / rate * 3600.0).ok()
    }
}

/// Whether the player is playing, which decides what the overlay shows: in a pause, a rate and a
/// time to level measured over the play before would pass for current ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    /// In a map, a campaign zone or a trial, and gaining experience or changing areas.
    #[default]
    Playing,
    /// In a town or hideout, as the log says, or idle in play for longer than `IDLE_AFTER`.
    /// `since` is when the pause began, on the clock of [`XpTracker::on_sample`] -- entering the
    /// town, or the last gain or change of area before idling -- and `elapsed` how long it has
    /// lasted as of the latest update.
    Paused { since: Duration, elapsed: Duration },
}

/// The current or last map run, for the overlay's map part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapStatus {
    /// Time spent in the map instance and the side areas entered from it (wall time, stopped
    /// while the character is anywhere else).
    pub time: Duration,
    /// Levels earned in it (same unit as [`XpStatus::rate_per_hour`]'s numerator: 0.012 = 1.2 %
    /// of a level).
    pub gained: f64,
    /// Whether the character is in it, left it a moment ago, or is done with it.
    pub state: RunState,
    /// Maps finished since the last login, as far back as the log's tail read at start goes:
    /// every map left for another one, completed or not -- the log doesn't say.
    pub finished: u32,
    /// The finished maps' average time; `None` before the first finishes.
    pub average: Option<Duration>,
}

/// Where a map run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// The character is in the map or one of its side areas: the run's clock runs.
    Running,
    /// Left no longer than `LAST_MAP_AFTER` ago -- for the hideout between portals, say: the
    /// clock waits for the character to come back.
    Waiting,
    /// Left longer ago: the last map, until the character goes back into its instance (the run
    /// resumes) or into another map (a new run starts).
    Last,
}

/// The rate window: seconds of play after which a gain weighs half as much in the rate.
#[derive(Debug, Clone, Copy)]
struct HalfLife(f64);

impl HalfLife {
    fn minutes(minutes: u16) -> Self {
        let minutes = minutes.clamp(*RATE_WINDOW_MINUTES.start(), *RATE_WINDOW_MINUTES.end());
        Self(f64::from(minutes) * 60.0)
    }
}

impl Default for HalfLife {
    fn default() -> Self {
        Self::minutes(DEFAULT_RATE_WINDOW_MINUTES)
    }
}

/// Turns timestamped bar readings and log events into an [`XpStatus`].
///
/// Gains are measured against the best reading since the last rebase, so reading jitter below it
/// never counts twice, and every reading first goes through a median of the last three, so one
/// misread sample never moves anything. A drop is either a level-up (the bar wraps: the rest of
/// the old level plus the new level's start count as gained) or a death (the penalty is lost, not
/// negative progress: the baseline moves down and re-earning it counts). The rate is play-time
/// based: time only counts between readable samples, outside towns and hideouts (when the log
/// says so), and until `IDLE_AFTER` without a gain; it carries over level-ups, deaths and breaks,
/// and weighs recent play most ([`Self::set_rate_window`]).
///
/// The player is paused ([`Activity`]) in towns and hideouts, since entering the first of them,
/// and once `IDLE_AFTER` has passed without a gain or a change of area, since the last one. That
/// only changes what the overlay shows; the rate keeps its own notion of play.
///
/// Map runs follow the log's area lines. A map instance is known by its seed: back into it
/// through its portal, the run resumes -- also once it has become the last map, `LAST_MAP_AFTER`
/// after the character left it -- and a map with another seed finishes the run and starts the
/// next. Side areas entered from the map (the Abyss depths) are part of its run; anything else
/// entered from a town (a campaign zone) is not, and neither is an ascendancy trial's area,
/// however it was entered: its scene line takes it out of the run. A run's time is wall time
/// between calls while the character is in it; its experience also takes what the bar shows in
/// the town right after it, since nothing in town gives any.
///
/// A logout resets everything but the rate window, since the next character may be a different
/// one. The log says so in two lines: the scene goes `(unknown)`, and an area line follows -- the
/// login's -- before the scene is named again; a scene named first was only a moment's blank.
///
/// An update's restart carries the tracker over as JSON, into the app's next version
/// ([`Self::carry`]): a field whose meaning changes takes a new name, so that version starts
/// afresh rather than misread this one's.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XpTracker {
    /// The two readings before the latest, oldest first, for the median filter.
    recent: [Option<f64>; 2],
    last_readable_at: Option<Duration>,
    /// The last filtered reading and when it was taken.
    last: Option<(Duration, f64)>,
    /// When the bar was last looked at, and whether that look read it: what a skipped look
    /// carries over ([`BarLook::Skipped`]). Not carried across an update's restart, whose new
    /// copy looks afresh.
    #[serde(skip)]
    last_look: Option<(Duration, bool)>,
    /// The best filtered reading since the last rebase.
    best: f64,
    character: Option<String>,
    level: Option<u32>,
    /// The latest time the tracker was told, by any call.
    clock: Option<Duration>,
    /// When the character went into the town or hideout it is in -- or into the first of the
    /// towns it has been in since it last left one -- per the log; `None` anywhere else.
    town_since: Option<Duration>,
    /// The last gain or change of area, or the first call before any: when idling began.
    active_at: Option<Duration>,
    /// Counted play since the last gain.
    since_gain: Duration,
    /// Logged level-ups minus wraps seen on the bar, and when it last changed: positive means a
    /// wrap is expected, negative that the bar wrapped before the log line arrived.
    level_up_balance: i32,
    balance_at: Duration,
    /// Exponentially weighted gain (levels) and play time (seconds), and the half-life they decay
    /// with.
    weighted_gain: f64,
    weighted_secs: f64,
    #[serde(skip)]
    half_life: HalfLife,
    /// All play counted, unweighted.
    counted: Duration,
    maps: MapRuns,
    /// The scene went `(unknown)` and hasn't been named since: a logout, if an area line comes
    /// next ([`LogEvent::SceneLost`]).
    scene_lost: bool,
}

impl XpTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the rate window: play `minutes` ago weighs half as much in the rate as play now,
    /// clamped to 1..=120 minutes. The play already weighed stays as it is and only decays at the
    /// new pace from now on, so the rate on screen neither jumps nor vanishes.
    pub fn set_rate_window(&mut self, minutes: u16) {
        self.half_life = HalfLife::minutes(minutes);
    }

    /// Applies what the log said before the tracker started (the tail of `Client.txt`, read at
    /// `at`), each event at the time its line was written, on the tracker's clock
    /// ([`log_time`]): the current area -- a town or hideout the character is in counts from when
    /// it was entered -- and, if the character levelled up since the last login, its name and
    /// level. Unlike [`Self::on_log_event`], old level-ups don't make the tracker expect a wrap,
    /// and the idle allowance starts at `at`. Map runs are timed from when they were entered, but
    /// for the first map the tail shows before any login: it may have started before the tail
    /// begins, so it's neither shown nor averaged, even after a trip to the hideout and back.
    pub fn restore(
        &mut self,
        history: impl IntoIterator<Item = (Duration, LogEvent)>,
        at: Duration,
    ) {
        self.advance(at);
        // Whether a map run the tail shows starting started there: after a login, or once an
        // earlier map was seen -- a map opened after another is a new instance.
        let mut covered = false;
        let mut cursor: Option<Duration> = None;
        for (when, event) in history {
            let when = when.min(at);
            if let Some(previous) = cursor {
                self.maps.tick(when.saturating_sub(previous));
            }
            cursor = Some(when);
            match event {
                LogEvent::LevelUp { character, level } => {
                    self.note_level(character, level);
                }
                LogEvent::AreaEntered { area, seed } => {
                    // The login after a logout, maybe as another character.
                    if std::mem::take(&mut self.scene_lost) {
                        self.character = None;
                        self.level = None;
                        self.town_since = None;
                        self.maps = MapRuns::default();
                        covered = true;
                    }
                    let timed = covered || self.maps.current.is_some();
                    self.enter(area, seed, timed, when);
                }
                LogEvent::TrialEntered => {
                    self.scene_lost = false;
                    self.maps.leave_for_trial(when);
                }
                LogEvent::SceneLost => self.scene_lost = true,
                LogEvent::SceneNamed => self.scene_lost = false,
            }
        }
        if let Some(previous) = cursor {
            self.maps.tick(at.saturating_sub(previous));
        }
    }

    /// A log line that just appeared; `at` is on the same clock as [`Self::on_sample`]'s.
    pub fn on_log_event(&mut self, event: LogEvent, at: Duration) {
        // The login after a logout, maybe as another character: everything starts over.
        if self.scene_lost && matches!(event, LogEvent::AreaEntered { .. }) {
            *self = Self {
                half_life: self.half_life,
                ..Self::default()
            };
        }
        self.advance(at);
        match event {
            LogEvent::LevelUp { character, level } => {
                if self.note_level(character, level) {
                    self.expire_level_up_balance(at);
                    self.level_up_balance += 1;
                    self.balance_at = at;
                }
            }
            LogEvent::AreaEntered { area, seed } => {
                self.enter(area, seed, true, at);
                // Changing areas is play: the idle allowance starts over.
                self.since_gain = Duration::ZERO;
                self.active_at = Some(at);
            }
            LogEvent::TrialEntered => {
                self.scene_lost = false;
                self.maps.leave_for_trial(at);
            }
            LogEvent::SceneLost => self.scene_lost = true,
            LogEvent::SceneNamed => self.scene_lost = false,
        }
    }

    /// Moves the clock to `at`, counting the time since the previous call toward the map run
    /// while the character is in it. Log lines arrive with the time they were read, so the time
    /// before one goes to the area it left.
    fn advance(&mut self, at: Duration) {
        let elapsed = self
            .clock
            .map_or(Duration::ZERO, |clock| at.saturating_sub(clock));
        self.clock = Some(at);
        self.active_at.get_or_insert(at);
        self.maps.tick(elapsed);
    }

    /// Follows the character into an area at `at`; `timed` is false for a map replayed from the
    /// log's tail that may have started before it ([`Self::restore`]).
    fn enter(&mut self, area: String, seed: u64, timed: bool, at: Duration) {
        let town = is_town(&area);
        self.town_since = if town {
            self.town_since.or(Some(at))
        } else {
            None
        };
        self.maps.enter(area, seed, town, timed, at);
    }

    /// Records a level-up of `character`, unless another character already levelled since the
    /// login (a party member's level-up shows up in the log too). Returns whether it was ours.
    fn note_level(&mut self, character: String, level: u32) -> bool {
        if self
            .character
            .as_ref()
            .is_some_and(|ours| *ours != character)
        {
            return false;
        }
        self.character = Some(character);
        self.level = Some(level);
        true
    }

    fn expire_level_up_balance(&mut self, at: Duration) {
        if at.saturating_sub(self.balance_at) > LEVEL_UP_MATCH {
            self.level_up_balance = 0;
        }
    }

    /// One sample's look at the bar ([`BarLook`]). `at` is monotonic time since any fixed origin.
    pub fn on_sample(&mut self, look: BarLook, at: Duration) {
        self.advance(at);
        let reading = match look {
            BarLook::Read(reading) => reading,
            BarLook::Unreadable => {
                self.last_look = Some((at, false));
                return;
            }
            BarLook::Skipped => {
                self.carry_reading(at);
                return;
            }
        };
        self.last_look = Some((at, true));
        if self
            .last_readable_at
            .is_some_and(|last| at.saturating_sub(last) > REBASE_GAP)
        {
            self.recent = [None; 2];
            self.last = None;
        }
        self.last_readable_at = Some(at);

        let median = match self.recent {
            [Some(a), Some(b)] => Some(reading.clamp(a.min(b), a.max(b))),
            _ => None,
        };
        self.recent = [self.recent[1], Some(reading)];
        let Some(value) = median else {
            return;
        };
        let Some((last_at, _)) = self.last else {
            self.best = value;
            self.last = Some((at, value));
            return;
        };
        self.expire_level_up_balance(at);
        let gain = self.gain_to(value, at);
        if gain > 0.0 {
            self.active_at = Some(at);
        }
        self.count(at.saturating_sub(last_at), gain);
        self.maps.credit(gain);
        self.last = Some((at, value));
    }

    /// A sample that didn't look at the bar: as a look reading what the last one did would have
    /// gone -- nothing gained, the time since counted as play -- without a reading for the median
    /// filter to weigh; after a look that couldn't read the bar, as unreadable still.
    fn carry_reading(&mut self, at: Duration) {
        if let (Some((_, true)), Some((last_at, value))) = (self.last_look, self.last) {
            self.count(at.saturating_sub(last_at), 0.0);
            self.last = Some((at, value));
        }
    }

    /// Whether a sample at `at`, the game out of the front, should look at the bar: once
    /// [`UNATTENDED_LOOK_EVERY`] has passed since the last look, and at every sample while there's
    /// no reading to carry over -- no look yet, a look that couldn't read the bar, readings the
    /// median filter has yet to make one of -- or the latest reading, not the one held, awaits the
    /// next to confirm it.
    pub fn unattended_look_due(&self, at: Duration) -> bool {
        match (self.last_look, self.last, self.recent[1]) {
            (Some((looked, true)), Some((_, held)), Some(latest)) if latest == held => {
                at.saturating_sub(looked) >= UNATTENDED_LOOK_EVERY
            }
            _ => true,
        }
    }

    /// Levels earned between the best reading so far and `value`, which becomes the new best --
    /// also after a loss, so re-earned experience counts again.
    fn gain_to(&mut self, value: f64, at: Duration) -> f64 {
        let drop = self.best - value;
        let gain = if drop > WRAP_DROP || (self.level_up_balance > 0 && drop > DROP_THRESHOLD) {
            log::info!(
                "xp: bar {:.4} -> {value:.4}, taken as a level-up (balance {})",
                self.best,
                self.level_up_balance
            );
            self.level_up_balance -= 1;
            self.balance_at = at;
            1.0 - self.best + value
        } else if drop < 0.0 {
            if -drop > BIG_GAIN {
                log::info!("xp: bar {:.4} -> {value:.4} in one step", self.best);
            }
            -drop
        } else if drop > DROP_THRESHOLD {
            log::info!("xp: bar {:.4} -> {value:.4}, taken as a loss", self.best);
            0.0
        } else {
            // Jitter below the best reading.
            return 0.0;
        };
        self.best = value;
        gain
    }

    /// Credits `elapsed` as play -- unless in town, across a gap nothing was gained over, or past
    /// `IDLE_AFTER` without a gain -- and folds it and `gain` into the weighted rate.
    fn count(&mut self, elapsed: Duration, gain: f64) {
        let eligible = if self.town_since.is_some() {
            Duration::ZERO
        } else if elapsed <= MAX_SAMPLE_GAP {
            elapsed
        } else if gain > 0.0 {
            // The bar was hidden (a panel over it, say) while the player kept earning.
            elapsed.min(IDLE_AFTER)
        } else {
            Duration::ZERO
        };
        let played = if gain > 0.0 {
            self.since_gain = Duration::ZERO;
            eligible
        } else {
            let allowance = IDLE_AFTER.saturating_sub(self.since_gain);
            self.since_gain += eligible;
            eligible.min(allowance)
        };
        let secs = played.as_secs_f64();
        self.counted += played;
        let decay = (-secs / self.half_life.0).exp2();
        self.weighted_gain = self.weighted_gain * decay + gain;
        self.weighted_secs = self.weighted_secs * decay + secs;
    }

    pub fn status(&self) -> XpStatus {
        XpStatus {
            fraction: self.last.map(|(_, value)| value),
            rate_per_hour: (self.counted >= MIN_RATE_TIME)
                .then(|| self.weighted_gain / self.weighted_secs * 3600.0),
            level: self.level,
            activity: self.activity(),
            map: self.clock.and_then(|now| self.maps.status(now)),
        }
    }

    /// Paused in a town or hideout since entering it, or since the last gain or change of area
    /// once that is longer ago than `IDLE_AFTER`.
    fn activity(&self) -> Activity {
        let Some(now) = self.clock else {
            return Activity::Playing;
        };
        let idle_since = self
            .active_at
            .filter(|active| now.saturating_sub(*active) > IDLE_AFTER);
        match self.town_since.or(idle_since) {
            Some(since) => Activity::Paused {
                since,
                elapsed: now.saturating_sub(since),
            },
            None => Activity::Playing,
        }
    }
}

// --- Across an update's restart -----------------------------------------------------------------

/// How long after its last call a tracker an update's restart left ([`XpTracker::carry`]) is
/// carried on with: the app's new copy takes it up within seconds -- the installer's or the
/// relaunch's wait for the old one to quit, then the first sample -- and one left longer ago is
/// from some other run.
const CARRY_FOR: Duration = Duration::from_secs(2 * 60);

impl XpTracker {
    /// The tracker as JSON, for the app's new copy an update's restart starts to carry on with
    /// ([`Self::carried`]): the rate, the map runs, the bar's last readings -- all but the rate
    /// window, which is the settings'.
    pub fn carry(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(self)
    }

    /// The tracker [`Self::carry`] left before an update's restart, taken up at `at` to carry on
    /// with instead of starting over from the log's tail ([`Self::restore`]); [`Self::catch_up`]
    /// takes what the tail says since. `None` for JSON that isn't a tracker as this version keeps
    /// one, and for one whose last call was over [`CARRY_FOR`] before `at` -- or after it, on a
    /// clock that has started over since, with Windows.
    pub fn carried(json: &[u8], at: Duration) -> Option<XpTracker> {
        let tracker: XpTracker = serde_json::from_slice(json)
            .inspect_err(|err| log::info!("xp: the tracker carried over isn't readable: {err}"))
            .ok()?;
        let age = at.checked_sub(tracker.clock?)?;
        (age <= CARRY_FOR).then_some(tracker)
    }

    /// Takes up the lines of the log's tail (`history`, each at the time it was written, as for
    /// [`Self::restore`]) that came after the tracker's last call -- while the app restarted --
    /// as they would have come live, then moves the clock to `at`. The earlier ones it has seen.
    pub fn catch_up(
        &mut self,
        history: impl IntoIterator<Item = (Duration, LogEvent)>,
        at: Duration,
    ) {
        let seen = self.clock;
        for (when, event) in history {
            if seen.is_none_or(|seen| when > seen) {
                self.on_log_event(event, when.min(at));
            }
        }
        self.advance(at);
    }
}

// --- Map runs -----------------------------------------------------------------------------------

/// A map the character left stays the run they may portal back into this long, and is the last
/// map after that: 89 % of the 496 returns to a map instance in the test machine's 15-month log
/// came within five minutes, half of them within 100 s.
const LAST_MAP_AFTER: Duration = Duration::from_secs(5 * 60);

/// Where the character is, as far as the current map run goes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Whereabouts {
    /// Somewhere no run counts: before the first map, in an area entered from a town that isn't
    /// one of the run's (a campaign zone through a waypoint), or in an ascendancy trial.
    #[default]
    Elsewhere,
    /// In the run's map or one of its side areas: the run's clock runs.
    InRun,
    /// In towns and hideouts since leaving the run: its clock stops, but experience read here is
    /// still the run's. Nothing in town gives experience, and the bar shows a map's last seconds
    /// a reading or two late -- often after the portal's loading screen.
    TownAfterRun,
}

/// One map instance and the side areas entered from it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MapRun {
    seed: u64,
    /// The side areas that joined the run, by area id and seed.
    side_areas: Vec<(String, u64)>,
    /// `None` for a run whose start the log's tail read at start doesn't show
    /// ([`XpTracker::restore`]): how long it had lasted is unknown, so the run is neither shown
    /// nor averaged.
    time: Option<Duration>,
    /// Levels earned in it.
    gained: f64,
}

impl MapRun {
    /// Whether a non-map area the character enters is part of the run: any area entered straight
    /// from the run joins it (the Abyss depths, the Vaal ruins of an incursion), and one that
    /// joined is part of it again when a portal opened in it brings the character back from the
    /// hideout.
    fn admits(&mut self, area: &str, seed: u64, from_run: bool) -> bool {
        if self
            .side_areas
            .iter()
            .any(|(id, known)| *known == seed && id == area)
        {
            return true;
        }
        if from_run {
            self.side_areas.push((area.to_owned(), seed));
        }
        from_run
    }
}

/// The map runs since the last login, as far back as the log's tail read at start goes.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MapRuns {
    current: Option<MapRun>,
    whereabouts: Whereabouts,
    /// The area the character is in, by id and seed, as the log last said.
    here: Option<(String, u64)>,
    /// When the character last left the run's areas; `None` while in them.
    left_at: Option<Duration>,
    /// Runs left for another map, and their total time.
    finished: u32,
    finished_time: Duration,
}

impl MapRuns {
    /// Counts `elapsed` toward the run while the character is in it.
    fn tick(&mut self, elapsed: Duration) {
        if self.whereabouts == Whereabouts::InRun
            && let Some(time) = self.current.as_mut().and_then(|run| run.time.as_mut())
        {
            *time += elapsed;
        }
    }

    /// Follows the character into `area`, instance `seed`, at `at`; `timed` as for
    /// [`XpTracker::enter`].
    fn enter(&mut self, area: String, seed: u64, town: bool, timed: bool, at: Duration) {
        let from_run = self.whereabouts == Whereabouts::InRun;
        self.whereabouts = if town {
            match self.whereabouts {
                Whereabouts::Elsewhere => Whereabouts::Elsewhere,
                Whereabouts::InRun | Whereabouts::TownAfterRun => Whereabouts::TownAfterRun,
            }
        } else if is_map(&area) {
            if self.current.as_ref().is_none_or(|run| run.seed != seed) {
                let run = MapRun {
                    seed,
                    side_areas: Vec::new(),
                    time: timed.then_some(Duration::ZERO),
                    gained: 0.0,
                };
                // Another map: the last one is done, whether or not it was completed.
                if let Some(MapRun {
                    time: Some(time), ..
                }) = self.current.replace(run)
                {
                    self.finished += 1;
                    self.finished_time += time;
                }
            }
            Whereabouts::InRun
        } else if self
            .current
            .as_mut()
            .is_some_and(|run| run.admits(&area, seed, from_run))
        {
            Whereabouts::InRun
        } else {
            Whereabouts::Elsewhere
        };
        self.here = Some((area, seed));
        if self.whereabouts == Whereabouts::InRun {
            self.left_at = None;
        } else if from_run {
            self.left_at = Some(at);
        }
    }

    /// The area just entered is an ascendancy trial's, as its scene line said at `at`: not part
    /// of the run, even when entered straight from it.
    fn leave_for_trial(&mut self, at: Duration) {
        if self.whereabouts != Whereabouts::InRun {
            return;
        }
        if let (Some(run), Some((area, seed))) = (self.current.as_mut(), self.here.as_ref()) {
            run.side_areas
                .retain(|(id, known)| !(known == seed && id == area));
        }
        self.whereabouts = Whereabouts::Elsewhere;
        self.left_at = Some(at);
    }

    /// Adds `gain` (levels) to the run if it was earned there.
    fn credit(&mut self, gain: f64) {
        if self.whereabouts != Whereabouts::Elsewhere
            && let Some(run) = self.current.as_mut()
        {
            run.gained += gain;
        }
    }

    fn status(&self, now: Duration) -> Option<MapStatus> {
        let run = self.current.as_ref()?;
        let state = if self.whereabouts == Whereabouts::InRun {
            RunState::Running
        } else if self
            .left_at
            .is_some_and(|left| now.saturating_sub(left) > LAST_MAP_AFTER)
        {
            RunState::Last
        } else {
            RunState::Waiting
        };
        Some(MapStatus {
            time: run.time?,
            gained: run.gained,
            state,
            finished: self.finished,
            average: self.finished_time.checked_div(self.finished),
        })
    }
}

// --- Wording ------------------------------------------------------------------------------------

/// A word of the overlay's plates, by what it says -- which decides how `ui::xp_overlay` draws it.
#[derive(Debug, Clone, PartialEq)]
pub enum Word {
    /// The rate, the level plate's headline: `+12,4 %/ч`.
    Rate(Cow<'static, str>),
    /// A value, or a word read as one: `1 ч 32 мин`, `4:07`, `пауза`.
    Value(Cow<'static, str>),
    /// A word saying what a value is: `до 75 ур.`, `карта`.
    Label(Cow<'static, str>),
    /// The dot between a part's groups: `·`.
    Dot,
}

impl Word {
    /// What the word reads.
    pub fn text(&self) -> &str {
        match self {
            Word::Rate(text) | Word::Value(text) | Word::Label(text) => text,
            Word::Dot => "·",
        }
    }
}

/// How much a part says: all of it, or the shorter wording for a rail it doesn't fit --
/// `ui::xp_overlay` tries the full one first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wording {
    Full,
    Short,
}

/// How much of the level is earned: `64,8 %`.
pub fn percent_words(fraction: f64) -> Vec<Word> {
    vec![Word::Value(i18n::percent(format_percent(fraction)).into())]
}

/// How fast the character levels and how much play is left to the next level: `+12,4 %/ч · до
/// 75 ур. 1 ч 32 мин` -- `до ур.` until the log names the level, `—` for the time when nothing
/// has been gained to go by; `+12,4 %/ч · 1 ч 32 мин` when [`Wording::Short`]. Or, the first two
/// minutes of play, the wait for a rate.
pub fn rate_words(status: &XpStatus, wording: Wording) -> Vec<Word> {
    let Some(rate) = status.rate_per_hour else {
        return vec![Word::Label(tr!("measuring rate…").into())];
    };
    let mut words = vec![Word::Rate(format_rate(rate).into()), Word::Dot];
    if wording == Wording::Full {
        words.push(Word::Label(match status.level {
            Some(level) => tr!("level {level} in", level = level + 1).into(),
            None => tr!("next level in").into(),
        }));
    }
    words.push(match status.time_to_level() {
        Some(eta) => Word::Value(i18n::duration(eta).into()),
        None => Word::Label("—".into()),
    });
    words
}

/// The level plate's parts in `wording`: the percent when `show_percent`, then the rate
/// ([`rate_words`]). In a pause the percent alone, switched on or not -- the rate and the time
/// to the level would still be those of the play before the pause, and how long it has lasted is
/// not worth the room (the owner's call, 2026-09-24) -- or nothing while the bar can't be read.
pub fn level_parts(status: &XpStatus, show_percent: bool, wording: Wording) -> Vec<Vec<Word>> {
    let percent = status.fraction.map(percent_words);
    match status.activity {
        Activity::Paused { .. } => percent.into_iter().collect(),
        Activity::Playing => percent
            .filter(|_| show_percent)
            .into_iter()
            .chain([rate_words(status, wording)])
            .collect(),
    }
}

/// The map run: `карта 4:07 +1,2 % · ср. 6:30`, and `последняя карта 9:00 +3,66 %` once it is
/// the last one; without the average in a pause. [`Wording::Short`] keeps `карта 4:07 +1,2 %`:
/// no average, and no `последняя` -- the plate dims a run that is over anyway.
pub fn map_words(map: &MapStatus, paused: bool, wording: Wording) -> Vec<Word> {
    let full = wording == Wording::Full;
    let label = if full && map.state == RunState::Last {
        tr!("last map")
    } else {
        tr!("map")
    };
    let mut words = vec![
        Word::Label(label.into()),
        Word::Value(format_clock(map.time).into()),
    ];
    if map.gained > 0.0 {
        let gained = i18n::percent(format_percent(map.gained));
        words.push(Word::Value(format!("+{gained}").into()));
    }
    if let Some(average) = map.average.filter(|_| full && !paused) {
        words.extend([
            Word::Dot,
            Word::Label(tr!("avg").into()),
            Word::Label(format_clock(average).into()),
        ]);
    }
    words
}

/// `+12,4 %/ч`: percent of a level per hour of play.
fn format_rate(rate_per_hour: f64) -> String {
    tr!(
        "+{percent}/h",
        percent = i18n::percent(format_percent(rate_per_hour))
    )
}

/// A fraction as a percentage number in the price panel's style (PoE Overlay II's): two
/// decimals under 10, one under 100, none above, trailing zeros dropped, and the interface
/// language's decimal separator -- `3,25`, `64,8`, `120` (`64.8` in English). Unlike
/// [`i18n::number`], no more decimals below 1: a map's `0,07` stays short.
fn format_percent(fraction: f64) -> String {
    let value = fraction * 100.0;
    let places = if value < 10.0 {
        2
    } else if value < 100.0 {
        1
    } else {
        0
    };
    let mut text = i18n::decimal(value, places);
    if places > 0 {
        let kept = text
            .trim_end_matches('0')
            .trim_end_matches(['.', ','])
            .len();
        text.truncate(kept);
    }
    text
}

/// `0:07`, `4:07`, `1:02:03`: a map's time as a stopwatch shows it -- the whole seconds elapsed,
/// no leading zero on the first field.
fn format_clock(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let (hours, minutes, seconds) = (secs / 3600, secs / 60 % 60, secs % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAME_4K: PhysicalRect = PhysicalRect {
        x: 0,
        y: 0,
        width: 3840,
        height: 2160,
    };

    /// A capture of `GAME_4K`'s bar from a live screenshot: raw RGB rows, as extracted.
    fn fixture_bgra(rgb: &[u8]) -> Vec<u8> {
        let (pixels, _) = rgb.as_chunks::<3>();
        pixels
            .iter()
            .flat_map(|&[r, g, b]| [b, g, r, 255])
            .collect()
    }

    #[test]
    fn reads_the_live_4k_bar() {
        let geometry = XpBarGeometry::for_client(GAME_4K).unwrap();
        let bgra = fixture_bgra(include_bytes!("../tests/fixtures/xp_bar_4k_65pct.rgb"));
        // The last filled pixel is x = 2145 of a track spanning 1154..2686.
        let expected = (2146.0 - 1154.0) / 1532.0;
        let fraction = read_fill(&geometry, &bgra).unwrap();
        assert!((fraction - expected).abs() < 0.001, "{fraction}");
    }

    #[test]
    fn a_bar_filled_end_to_end_is_not_a_reading() {
        // The live 65 % bar with its empty part painted in the fill's colours, row by row, the
        // tick stems under it left as they were: what the tracker read as 100 % for 5-20 s at a
        // time on 2026-09-24, crediting a quarter of a level each time.
        let geometry = XpBarGeometry::for_client(GAME_4K).unwrap();
        let mut bgra = fixture_bgra(include_bytes!("../tests/fixtures/xp_bar_4k_65pct.rgb"));
        let width = geometry.capture.width as usize;
        // Capture columns: x = 1500 is well inside the fill, and the fill ends at x = 2145.
        let (filled, empty_from) = (1500 - 1152, 2146 - 1152);
        for row in geometry.fill_rows.clone() {
            let source = (row * width + filled) * 4;
            let colour: [u8; 4] = bgra[source..source + 4].try_into().unwrap();
            for column in empty_from..width {
                let at = (row * width + column) * 4;
                bgra[at..at + 4].copy_from_slice(&colour);
            }
        }
        assert_eq!(read_fill(&geometry, &bgra), None);
    }

    #[test]
    fn refuses_the_bar_under_the_price_panel() {
        // Live: the price-check panel spanned x = 1485..2508, over the bar's middle.
        let geometry = XpBarGeometry::for_client(GAME_4K).unwrap();
        let bgra = fixture_bgra(include_bytes!(
            "../tests/fixtures/xp_bar_4k_price_panel.rgb"
        ));
        assert_eq!(read_fill(&geometry, &bgra), None);
    }

    #[test]
    fn geometry_scales_with_the_game_height_and_follows_its_position() {
        let full = XpBarGeometry::for_client(GAME_4K).unwrap();
        assert_eq!(
            full.capture,
            PhysicalRect {
                x: 1152,
                y: 2141,
                width: 1536,
                height: 10
            }
        );
        // A 1920x1080 window at (100, 50): everything halves and shifts with it.
        let window = XpBarGeometry::for_client(PhysicalRect {
            x: 100,
            y: 50,
            width: 1920,
            height: 1080,
        })
        .unwrap();
        assert_eq!(window.capture.x, 100 + 960 - 384);
        assert_eq!(window.capture.width, 768);
        assert_eq!(window.capture.y, 50 + 1070);
        assert!((window.ticks[9] + f64::from(window.capture.x) - (100.0 + 960.0)).abs() < 1e-9);
        assert!((window.fill_end - window.fill_start - 766.0).abs() < 1e-9);
        assert!(
            XpBarGeometry::for_client(PhysicalRect {
                height: 600,
                ..GAME_4K
            })
            .is_none()
        );
        // Portrait: the bar would stick out of the window, so nothing is read at all.
        assert!(
            XpBarGeometry::for_client(PhysicalRect {
                width: 1400,
                ..GAME_4K
            })
            .is_none()
        );
    }

    /// Live instances from the test machine's log: a map and the Abyss depths opened in it
    /// (2026-09-22), and a Trial of Chaos (2026-09-18).
    const EPITAPH: u64 = 2_266_921_739;
    const DEPTHS: u64 = 3_193_393_764;
    const CHAOS: u64 = 137_717_311;

    #[test]
    fn parses_level_ups_areas_trials_and_scenes() {
        assert_eq!(
            parse_log_line(
                "2026/09/22 18:48:06 4189156 3ef23348 [INFO Client 19772] : mttzzz_merc_next (Легионер каменитов) достигает 38 уровня"
            ),
            Some(LogEvent::LevelUp {
                character: "mttzzz_merc_next".to_owned(),
                level: 38
            })
        );
        assert_eq!(
            parse_log_line(
                "2025/12/12 13:03:36 1004610062 3ef232c2 [INFO Client 1157464] : HolyMolyThisIsCharName (Mercenary) is now level 2"
            ),
            Some(LogEvent::LevelUp {
                character: "HolyMolyThisIsCharName".to_owned(),
                level: 2
            })
        );
        for (area, seed, town) in [
            ("G3_town", 1, true),
            ("P1_Town", 1, true),
            ("HideoutCanal", 1, true),
            ("MapEpitaph", EPITAPH, false),
            ("Abyss_Depths2", DEPTHS, false),
            ("MapHideoutCanal_Claimable", 915_220_458, false),
            ("Delirium_Act1Town", 51_873_904, false),
        ] {
            let line = format!(
                "2026/09/22 21:50:20 15122078 2caa229f [DEBUG Client 31244] Generating level 77 area \"{area}\" with seed {seed}"
            );
            assert_eq!(
                parse_log_line(&line),
                Some(LogEvent::AreaEntered {
                    area: area.to_owned(),
                    seed
                }),
                "{area}"
            );
            assert_eq!(is_town(area), town, "{area}");
        }
        // Live Russian trial scenes; the English client names them as RePoE does.
        for line in [
            "2026/09/18 13:48:29 24486437 7fbd1225 [INFO Client 14580] [SCENE] Set Source [Испытание Хаоса]",
            "2026/09/17 16:23:11 15768390 7fbd1225 [INFO Client 24908] [SCENE] Set Source [Испытание Сехем]",
            "2025/12/12 22:05:56 1037150078 7fbd122f [INFO Client 1196912] [SCENE] Set Source [The Trial of Chaos]",
            "2025/12/12 22:05:56 1037150078 7fbd122f [INFO Client 1196912] [SCENE] Set Source [Trial of the Sekhemas]",
        ] {
            assert_eq!(parse_log_line(line), Some(LogEvent::TrialEntered), "{line}");
        }
        assert_eq!(
            parse_log_line(
                "2026/09/22 21:35:24 14225828 7fbd1225 [INFO Client 31244] [SCENE] Set Source [(unknown)]"
            ),
            Some(LogEvent::SceneLost)
        );
        // Every other named scene: a hideout, the world map, the Act 4 campaign area named a
        // trial (in Russian and in English). `(null)` names nothing.
        for line in [
            "2026/09/22 21:35:30 14232109 7fbd1225 [INFO Client 31244] [SCENE] Set Source [Убежище в каналах]",
            "2025/07/31 01:23:22 61952828 775aec31 [INFO Client 6392] [SCENE] Set Source [Акт 3]",
            "2026/09/15 07:54:37 63070000 7fbd1225 [INFO Client 36512] [SCENE] Set Source [Испытание предков]",
            "2025/12/12 22:05:56 1037150078 7fbd122f [INFO Client 1196912] [SCENE] Set Source [Trial of the Ancestors]",
        ] {
            assert_eq!(parse_log_line(line), Some(LogEvent::SceneNamed), "{line}");
        }
        assert_eq!(
            parse_log_line(
                "2026/09/23 17:41:02 86561203 7fbd1225 [INFO Client 31244] [SCENE] Set Source [(null)]"
            ),
            None
        );
    }

    #[test]
    fn ignores_look_alike_lines() {
        for line in [
            // A death, a system message with "уровня" in it, chat.
            "2026/09/22 18:50:38 4340937 3ef23348 [INFO Client 19772] : mttzzz_merc_next был повержен.",
            "2026/09/22 16:02:11 7311140 3ef23348 [INFO Client 28800] : Не удалось применить предмет: Уровень предмета слишком низкий для этого уровня",
            "2026/09/22 16:02:12 7311141 3ef23348 [INFO Client 28800] #Trader: Fake (Mercenary) is now level 99",
            "2026/09/22 16:02:12 7311141 3ef23348 [INFO Client 28800] Fake: X (Y) достигает 99 уровня",
        ] {
            assert_eq!(parse_log_line(line), None, "{line}");
        }
    }

    /// One reading every 2 s, `readings` of them, the first at `start` seconds; returns the time
    /// after the last one.
    fn play(
        tracker: &mut XpTracker,
        start: f64,
        readings: usize,
        fraction: impl Fn(f64) -> Option<f64>,
    ) -> f64 {
        let mut t = start;
        for _ in 0..readings {
            tracker.on_sample(reading(fraction(t)), Duration::from_secs_f64(t));
            t += 2.0;
        }
        t
    }

    /// A look at the bar that read `fill`, or couldn't read it.
    fn reading(fill: Option<f64>) -> BarLook {
        fill.map_or(BarLook::Unreadable, BarLook::Read)
    }

    /// A sample at `t` as the overlay takes one: a look at the bar, reading `fill`, while the game
    /// is `in_front` or the tracker is due one; a skipped look otherwise. Whether it looked.
    fn sample_at(tracker: &mut XpTracker, t: f64, in_front: bool, fill: Option<f64>) -> bool {
        let at = Duration::from_secs_f64(t);
        let looks = in_front || tracker.unattended_look_due(at);
        tracker.on_sample(
            if looks {
                reading(fill)
            } else {
                BarLook::Skipped
            },
            at,
        );
        looks
    }

    /// The log's line for entering `name`, instance `seed`.
    fn area(name: &str, seed: u64) -> LogEvent {
        LogEvent::AreaEntered {
            area: name.to_owned(),
            seed,
        }
    }

    /// The log saying the character entered `name`, instance `seed`, at `t` seconds.
    fn enter(tracker: &mut XpTracker, name: &str, seed: u64, t: f64) {
        tracker.on_log_event(area(name, seed), Duration::from_secs_f64(t));
    }

    /// `events` as the log's tail at start replays them, every line as old as the start itself.
    fn at_start(events: impl IntoIterator<Item = LogEvent>) -> Vec<(Duration, LogEvent)> {
        events
            .into_iter()
            .map(|event| (Duration::ZERO, event))
            .collect()
    }

    /// The 4K bar's pixel steps plus a deterministic +-1 px wobble held for two readings at a
    /// time -- jitter the median filter lets through, like a fill edge flickering between pixels.
    fn as_read(fraction: f64, t: f64) -> f64 {
        let pixel = 1.0 / 1532.0;
        let wobble = [0.0, 0.0, pixel, pixel, 0.0, 0.0, -pixel, -pixel][(t / 2.0) as usize % 8];
        ((fraction.fract() / pixel).round() * pixel + wobble).clamp(0.0, 1.0)
    }

    fn assert_near(actual: Option<f64>, expected: f64, tolerance: f64) {
        let actual = actual.expect("a value");
        assert!(
            (actual - expected).abs() <= tolerance * expected,
            "{actual} vs {expected}"
        );
    }

    #[test]
    fn steady_farming_reads_its_rate_and_time_to_level_despite_jitter() {
        let mut tracker = XpTracker::new();
        let rate = 0.12 / 3600.0;
        let t = play(&mut tracker, 0.0, 60, |t| Some(as_read(0.30 + rate * t, t)));
        assert_eq!(tracker.status().rate_per_hour, None, "too early for a rate");
        let t = play(&mut tracker, t, 840, |t| Some(as_read(0.30 + rate * t, t)));
        let status = tracker.status();
        assert_near(status.rate_per_hour, 0.12, 0.05);
        let left = (1.0 - (0.30 + rate * t)) / 0.12 * 3600.0;
        assert_near(
            status.time_to_level().map(|eta| eta.as_secs_f64()),
            left,
            0.06,
        );
    }

    #[test]
    fn a_level_up_carries_the_rate_over() {
        let mut tracker = XpTracker::new();
        let rate = 0.20 / 3600.0;
        let fraction = move |t: f64| Some(as_read(0.90 + rate * t, t));
        // Wraps after 30 minutes; the log names the new level as it happens.
        let t = play(&mut tracker, 0.0, 900, fraction);
        tracker.on_log_event(
            LogEvent::LevelUp {
                character: "hero".to_owned(),
                level: 75,
            },
            Duration::from_secs_f64(t),
        );
        // A minute into the new level the rate is still the old one, not a fresh measurement.
        play(&mut tracker, t, 30, fraction);
        let status = tracker.status();
        assert_near(status.rate_per_hour, 0.20, 0.05);
        assert!(status.fraction.unwrap() < 0.1);
        assert_eq!(status.level, Some(75));
    }

    #[test]
    fn a_death_costs_experience_but_not_rate() {
        let mut tracker = XpTracker::new();
        let rate = 0.10 / 3600.0;
        let t = play(&mut tracker, 0.0, 600, |t| {
            Some(as_read(0.50 + rate * t, t))
        });
        let before = tracker.status();
        // The penalty takes a tenth of the level; 30 s on the death screen, then back to it.
        let t = play(&mut tracker, t, 15, |_| None);
        play(&mut tracker, t, 300, |t| Some(as_read(0.40 + rate * t, t)));
        let after = tracker.status();
        assert_near(after.rate_per_hour, 0.10, 0.05);
        assert!(after.fraction.unwrap() < before.fraction.unwrap());
        assert!(after.time_to_level().unwrap() > before.time_to_level().unwrap());
    }

    #[test]
    fn out_of_the_front_a_look_every_ten_seconds_reads_as_a_look_every_two() {
        // One session twice over, looking at the bar at every sample and as the overlay does: ten
        // minutes of mapping, seven with the game behind another window and the bar still where
        // the game left it -- a pause once five minutes pass without a gain -- then ten minutes'
        // more mapping.
        let rate = 0.12 / 3600.0;
        let mut every = XpTracker::new();
        let mut due = XpTracker::new();
        for tracker in [&mut every, &mut due] {
            enter(tracker, "MapEpitaph", EPITAPH, 0.0);
        }
        let mut parked = None;
        let mut unattended_looks = 0;
        for step in 0..810 {
            let t = f64::from(step) * 2.0;
            let in_front = !(600.0..1_020.0).contains(&t);
            let fill = if t < 600.0 {
                as_read(0.2 + rate * t, t)
            } else {
                // The bar reads on as the last look before read it.
                let parked = *parked.get_or_insert_with(|| every.recent[1].unwrap());
                if in_front {
                    as_read(parked + rate * (t - 1_020.0), t)
                } else {
                    parked
                }
            };
            every.on_sample(BarLook::Read(fill), Duration::from_secs_f64(t));
            if sample_at(&mut due, t, in_front, Some(fill)) && !in_front {
                unattended_looks += 1;
            }
            assert_eq!(due.status(), every.status(), "at {t} s");
            if t == 1_018.0 {
                assert!(matches!(due.status().activity, Activity::Paused { .. }));
            }
        }
        // 210 samples out of the front: a look at every fifth, and at the first few till the
        // still bar's reading was confirmed.
        assert!(unattended_looks <= 210 / 5 + 3, "{unattended_looks}");
    }

    #[test]
    fn out_of_the_front_a_change_is_taken_at_the_sample_after_the_look_that_saw_it() {
        let mut tracker = XpTracker::new();
        let rate = 0.12 / 3600.0;
        let mut t = play(&mut tracker, 0.0, 300, |t| Some(as_read(0.5 + rate * t, t)));
        let held = tracker.status().fraction.unwrap();
        // A minute behind another window, the bar still: a look every ten seconds.
        let still_minute = |tracker: &mut XpTracker, t: &mut f64, fill: f64| {
            let mut looks = 0;
            for _ in 0..30 {
                looks += usize::from(sample_at(tracker, *t, false, Some(fill)));
                *t += 2.0;
            }
            looks
        };
        assert!(still_minute(&mut tracker, &mut t, held) <= 8);
        // The character dies meanwhile, a tenth of the level lost: seen at the next look, taken
        // at the sample after it, where the median filter has seen it twice.
        let (died, dead) = (t, held - 0.1);
        while tracker.status().fraction != Some(dead) {
            sample_at(&mut tracker, t, false, Some(dead));
            t += 2.0;
            assert!(t - died <= 14.0, "not taken by {t} s");
        }
        // And the looks go back to one every ten seconds.
        assert!(still_minute(&mut tracker, &mut t, dead) <= 7);
    }

    #[test]
    fn a_skipped_look_carries_over_only_a_reading() {
        let at = Duration::from_secs_f64;
        assert!(
            XpTracker::new().unattended_look_due(at(0.0)),
            "nothing to carry yet"
        );
        let mut every = XpTracker::new();
        let mut due = XpTracker::new();
        for tracker in [&mut every, &mut due] {
            play(tracker, 0.0, 10, |_| Some(0.5));
            // The bar covered at the next look, by the window in front of the game.
            tracker.on_sample(BarLook::Unreadable, at(20.0));
        }
        assert!(due.unattended_look_due(at(22.0)));
        // Skipped looks after it count no play, as looks finding the bar covered still would.
        for t in [22.0, 24.0, 26.0] {
            every.on_sample(BarLook::Unreadable, at(t));
            due.on_sample(BarLook::Skipped, at(t));
        }
        for tracker in [&mut every, &mut due] {
            play(tracker, 28.0, 5, |_| Some(0.5));
        }
        assert_eq!(due.counted, every.counted);
        assert_eq!(due.status(), every.status());
        // After a look that read it, a skipped one counts the time since as play.
        let counted = due.counted;
        due.on_sample(BarLook::Skipped, at(38.0));
        assert_eq!(due.counted, counted + Duration::from_secs(2));
    }

    /// Ten minutes of mapping at 12 % of a level per hour from `from`, starting at `start`
    /// seconds; returns the end time and fraction.
    fn map_for_ten_minutes(tracker: &mut XpTracker, start: f64, from: f64) -> (f64, f64) {
        let rate = 0.12 / 3600.0;
        let end = play(tracker, start, 300, |t| {
            Some(as_read(from + rate * (t - start), t))
        });
        (end, from + rate * (end - start))
    }

    #[test]
    fn town_and_idle_time_do_not_dilute_the_rate() {
        // Ten minutes in the hideout between two maps, the log saying so.
        let mut logged = XpTracker::new();
        let (t, parked) = map_for_ten_minutes(&mut logged, 0.0, 0.2);
        enter(&mut logged, "HideoutCanal", 1, t);
        let t = play(&mut logged, t, 300, |t| Some(as_read(parked, t)));
        enter(&mut logged, "MapBluff", 17, t);
        map_for_ten_minutes(&mut logged, t, parked);
        assert_near(logged.status().rate_per_hour, 0.12, 0.05);

        // Twenty idle minutes in a map and no log: only `IDLE_AFTER` of them count, which the
        // weighting puts at ~0.099 (all twenty would make it ~0.072).
        let mut idle = XpTracker::new();
        let (t, parked) = map_for_ten_minutes(&mut idle, 0.0, 0.2);
        let t = play(&mut idle, t, 600, |t| Some(as_read(parked, t)));
        map_for_ten_minutes(&mut idle, t, parked);
        let diluted = idle.status().rate_per_hour.unwrap();
        assert!(diluted > 0.088 && diluted < 0.11, "{diluted}");
    }

    #[test]
    fn the_hideout_pauses_play_but_not_the_rate() {
        // The owner's evening: a map, then an hour and a half in the hideout, the bar still.
        let mut tracker = XpTracker::new();
        enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
        let (t, parked) = map_for_ten_minutes(&mut tracker, 0.0, 0.2);
        let mapping = tracker.status();
        assert_eq!(mapping.activity, Activity::Playing);
        enter(&mut tracker, "HideoutCanal", 1, t);
        let t = play(&mut tracker, t, 2700, |_| Some(parked));
        let hideout = tracker.status();
        assert_eq!(
            hideout.activity,
            Activity::Paused {
                since: Duration::from_secs(600),
                elapsed: Duration::from_secs(5398),
            }
        );
        // The rate is still that of the mapping: the pause only changes what the overlay shows.
        assert_eq!(hideout.rate_per_hour, mapping.rate_per_hour);
        enter(&mut tracker, "MapBluff", 17, t);
        assert_eq!(tracker.status().activity, Activity::Playing);
    }

    #[test]
    fn idling_over_five_minutes_in_a_map_pauses_play_until_the_next_gain() {
        let mut tracker = XpTracker::new();
        enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
        // Five minutes of mapping, then the bar stands still: the last gain is read at 300 s, the
        // median filter passing the last rise on a reading late.
        let fill = |t: f64| Some(0.2 + t.min(298.0) / 36_000.0);
        // Five minutes after it, still play; more than five, a pause since the last gain.
        let t = play(&mut tracker, 0.0, 301, fill);
        assert_eq!(tracker.status().activity, Activity::Playing);
        let t = play(&mut tracker, t, 1, fill);
        assert_eq!(
            tracker.status().activity,
            Activity::Paused {
                since: Duration::from_secs(300),
                elapsed: Duration::from_secs(302),
            }
        );
        play(&mut tracker, t, 2, |_| Some(0.21));
        assert_eq!(tracker.status().activity, Activity::Playing);
    }

    #[test]
    fn a_logged_level_up_across_a_loading_screen_is_not_a_death() {
        // Campaign pace: a boss kill worth most of a level, landed right before a loading screen.
        let run = |logged: bool| {
            let mut tracker = XpTracker::new();
            let t = play(&mut tracker, 0.0, 120, |t| {
                Some(as_read(0.30 + 0.25 * t / 240.0, t))
            });
            if logged {
                tracker.on_log_event(
                    LogEvent::LevelUp {
                        character: "hero".to_owned(),
                        level: 3,
                    },
                    Duration::from_secs_f64(t),
                );
            }
            let t = play(&mut tracker, t, 10, |_| None);
            play(&mut tracker, t, 3, |t| Some(as_read(0.30, t)));
            tracker.status().rate_per_hour.unwrap()
        };
        // Wrapped from 0.55 to 0.30: 0.75 of a level on top of the steady 0.25.
        assert!(run(true) > 3.0 * run(false));
    }

    #[test]
    fn single_misreads_move_nothing() {
        let mut tracker = XpTracker::new();
        let spiky = |t: f64| {
            let n = (t / 2.0) as usize;
            Some(match n % 50 {
                10 => 0.95,
                30 => 0.05,
                _ => 0.5,
            })
        };
        play(&mut tracker, 0.0, 200, spiky);
        let status = tracker.status();
        assert_eq!(status.fraction, Some(0.5));
        assert_eq!(status.rate_per_hour, Some(0.0));
        assert_eq!(status.time_to_level(), None);
    }

    /// Half an hour at 10 % of a level per hour, then five minutes at 30 %; returns the rate
    /// right after.
    fn rate_after_a_change_of_pace(tracker: &mut XpTracker) -> f64 {
        let (slow, fast) = (0.10 / 3600.0, 0.30 / 3600.0);
        let change = play(tracker, 0.0, 900, |t| Some(as_read(0.2 + slow * t, t)));
        let from = 0.2 + slow * change;
        play(tracker, change, 150, |t| {
            Some(as_read(from + fast * (t - change), t))
        });
        tracker.status().rate_per_hour.unwrap()
    }

    #[test]
    fn a_shorter_rate_window_follows_a_new_pace_sooner() {
        let mut quick = XpTracker::new();
        quick.set_rate_window(5);
        let mut steady = XpTracker::new();
        steady.set_rate_window(30);
        // Five minutes are one half-life of the short window: about halfway from 10 % to 30 %.
        let quick_rate = rate_after_a_change_of_pace(&mut quick);
        assert!(quick_rate > 0.18, "{quick_rate}");
        let steady_rate = rate_after_a_change_of_pace(&mut steady);
        assert!(steady_rate < 0.16, "{steady_rate}");
        // A new window only changes how play decays from now on: the rate on screen stays put.
        let before = steady.status();
        steady.set_rate_window(5);
        assert_eq!(steady.status(), before);
    }

    #[test]
    fn the_rate_window_is_clamped_and_outlives_a_logout() {
        let rate = |minutes: u16, logged_out: bool| {
            let mut tracker = XpTracker::new();
            tracker.set_rate_window(minutes);
            if logged_out {
                tracker.on_log_event(LogEvent::SceneLost, Duration::ZERO);
                enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
            }
            rate_after_a_change_of_pace(&mut tracker)
        };
        assert_eq!(rate(0, false), rate(1, false));
        assert_eq!(rate(u16::MAX, false), rate(120, false));
        assert_eq!(rate(5, true), rate(5, false));
        assert_ne!(rate(5, true), rate(10, false));
    }

    #[test]
    fn logging_out_forgets_the_character() {
        let mut tracker = XpTracker::new();
        tracker.restore(
            at_start([
                LogEvent::LevelUp {
                    character: "old".to_owned(),
                    level: 90,
                },
                LogEvent::SceneLost,
                LogEvent::AreaEntered {
                    area: "HideoutCanal".to_owned(),
                    seed: 1,
                },
            ]),
            Duration::ZERO,
        );
        assert_eq!(tracker.status().level, None);
        tracker.restore(
            at_start([LogEvent::LevelUp {
                character: "new".to_owned(),
                level: 12,
            }]),
            Duration::ZERO,
        );
        assert_eq!(tracker.status().level, Some(12));

        let t = play(&mut tracker, 0.0, 200, |t| Some(0.4 + t / 36_000.0));
        // Out to character select: nothing forgotten yet.
        tracker.on_log_event(LogEvent::SceneLost, Duration::from_secs_f64(t));
        assert_eq!(tracker.status().level, Some(12));
        // In again: the login's area, maybe another character's.
        enter(&mut tracker, "G1_1", 7, t + 20.0);
        let status = tracker.status();
        assert_eq!(
            (
                status.fraction,
                status.rate_per_hour,
                status.level,
                status.map
            ),
            (None, None, None, None)
        );
    }

    #[test]
    fn a_moment_of_unknown_scene_in_the_hideout_is_not_a_logout() {
        // The owner's log, 2026-09-23: in the hideout since 17:41:02, the scene blanked at
        // 18:20:51 and came back named 5 s later, with no area line between. Taken for a logout,
        // it forgot the character's level and that it was in the hideout, and the XP line showed
        // «+0 %/ч · до ур. —» there instead of the pause.
        let mut tracker = XpTracker::new();
        tracker.restore(
            at_start([
                LogEvent::LevelUp {
                    character: "hero".to_owned(),
                    level: 91,
                },
                LogEvent::AreaEntered {
                    area: "HideoutCanal".to_owned(),
                    seed: 1,
                },
            ]),
            Duration::ZERO,
        );
        tracker.on_log_event(LogEvent::SceneLost, Duration::from_secs(60));
        tracker.on_log_event(LogEvent::SceneNamed, Duration::from_secs(65));
        // Ten idle minutes in the hideout: paused, and not play the rate counts.
        play(&mut tracker, 66.0, 300, |_| Some(0.58));
        let status = tracker.status();
        assert_eq!(status.level, Some(91));
        assert_eq!(status.rate_per_hour, None, "hideout time isn't play");
        assert!(matches!(status.activity, Activity::Paused { .. }));
        // The next map is still this character's: no logout left pending.
        enter(&mut tracker, "MapPit", 2443463257, 700.0);
        assert_eq!(tracker.status().level, Some(91));
    }

    #[test]
    fn a_map_run_pauses_in_the_hideout_and_counts_its_side_areas() {
        // The live sequence from 21:50:20, in seconds: into the map, back from a trip to the
        // hideout at 21:50:36, the Abyss depths opened in the map at 21:53:52, the map again at
        // 22:00:59, then the hideout.
        let mut tracker = XpTracker::new();
        let unread = |_: f64| None;
        enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
        let t = play(&mut tracker, 0.0, 5, unread);
        enter(&mut tracker, "HideoutCanal", 1, t);
        play(&mut tracker, t, 3, unread);
        let paused = tracker.status().map.unwrap();
        assert_eq!(
            (paused.time, paused.state),
            (Duration::from_secs(10), RunState::Waiting)
        );
        enter(&mut tracker, "MapEpitaph", EPITAPH, 16.0);
        play(&mut tracker, 16.0, 98, unread);
        enter(&mut tracker, "Abyss_Depths2", DEPTHS, 212.0);
        play(&mut tracker, 212.0, 213, unread);
        enter(&mut tracker, "MapEpitaph", EPITAPH, 639.0);
        play(&mut tracker, 639.0, 30, unread);
        enter(&mut tracker, "HideoutCanal", 1, 700.0);
        play(&mut tracker, 700.0, 30, unread);
        assert_eq!(
            tracker.status().map,
            Some(MapStatus {
                time: Duration::from_secs(694),
                gained: 0.0,
                state: RunState::Waiting,
                finished: 0,
                average: None,
            })
        );
    }

    #[test]
    fn a_new_map_finishes_the_last_one_and_averages_the_finished() {
        let mut tracker = XpTracker::new();
        let unread = |_: f64| None;
        enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
        play(&mut tracker, 0.0, 150, unread);
        enter(&mut tracker, "HideoutCanal", 1, 300.0);
        // The same map, but another instance: a new run, the first one done in 5:00.
        enter(&mut tracker, "MapEpitaph", 915_220_458, 330.0);
        play(&mut tracker, 330.0, 210, unread);
        let second = tracker.status().map.unwrap();
        assert_eq!(
            (second.time, second.finished, second.average),
            (Duration::from_secs(418), 1, Some(Duration::from_secs(300)))
        );
        // On to a third map, never back to the second: it counts with the 7:00 it had.
        enter(&mut tracker, "HideoutCanal", 1, 750.0);
        enter(&mut tracker, "MapBluff", 17, 760.0);
        play(&mut tracker, 760.0, 30, unread);
        assert_eq!(
            tracker.status().map,
            Some(MapStatus {
                time: Duration::from_secs(58),
                gained: 0.0,
                state: RunState::Running,
                finished: 2,
                average: Some(Duration::from_secs(360)),
            })
        );
    }

    #[test]
    fn a_map_left_for_over_five_minutes_is_the_last_until_the_character_maps_again() {
        let mut tracker = XpTracker::new();
        let unread = |_: f64| None;
        let state = |tracker: &XpTracker| tracker.status().map.unwrap().state;
        enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
        play(&mut tracker, 0.0, 150, unread);
        enter(&mut tracker, "HideoutCanal", 1, 300.0);
        // For five minutes the player may still portal back.
        play(&mut tracker, 300.0, 151, unread);
        assert_eq!(state(&tracker), RunState::Waiting);
        play(&mut tracker, 602.0, 1, unread);
        assert_eq!(state(&tracker), RunState::Last);
        // Back through its portal after all: the same run goes on from its 5:00.
        enter(&mut tracker, "MapEpitaph", EPITAPH, 700.0);
        play(&mut tracker, 700.0, 31, unread);
        assert_eq!(
            tracker.status().map,
            Some(MapStatus {
                time: Duration::from_secs(360),
                gained: 0.0,
                state: RunState::Running,
                finished: 0,
                average: None,
            })
        );
        // Left for good this time: the next map starts a new run and finishes the last one.
        enter(&mut tracker, "HideoutCanal", 1, 760.0);
        enter(&mut tracker, "MapBluff", 17, 1200.0);
        play(&mut tracker, 1200.0, 16, unread);
        assert_eq!(
            tracker.status().map,
            Some(MapStatus {
                time: Duration::from_secs(30),
                gained: 0.0,
                state: RunState::Running,
                finished: 1,
                average: Some(Duration::from_secs(360)),
            })
        );
    }

    #[test]
    fn a_campaign_zone_after_a_town_is_not_part_of_the_run() {
        let mut tracker = XpTracker::new();
        let unread = |_: f64| None;
        let run = |tracker: &XpTracker| {
            let map = tracker.status().map.unwrap();
            (map.time, map.state)
        };
        enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
        enter(&mut tracker, "Abyss_Depths2", DEPTHS, 100.0);
        // A portal from the depths to the hideout, then a waypoint to a campaign zone.
        enter(&mut tracker, "HideoutCanal", 1, 200.0);
        enter(&mut tracker, "G2_3", 42, 220.0);
        play(&mut tracker, 220.0, 40, unread);
        assert_eq!(run(&tracker), (Duration::from_secs(200), RunState::Waiting));
        // Back to the hideout and through that portal into the depths: the run again.
        enter(&mut tracker, "HideoutCanal", 1, 300.0);
        enter(&mut tracker, "Abyss_Depths2", DEPTHS, 320.0);
        play(&mut tracker, 320.0, 41, unread);
        assert_eq!(run(&tracker), (Duration::from_secs(280), RunState::Running));
    }

    #[test]
    fn a_trial_is_play_but_never_part_of_a_map_run() {
        let mut tracker = XpTracker::new();
        enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
        let (t, reached) = map_for_ten_minutes(&mut tracker, 0.0, 0.2);
        let gained = tracker.status().map.unwrap().gained;
        // Into the Trial of Chaos straight from the map, as into one of its side areas; the scene
        // line after the area line names it a trial. Then ten minutes of it at the map's pace.
        enter(&mut tracker, "G3_10", CHAOS, t);
        tracker.on_log_event(LogEvent::TrialEntered, Duration::from_secs_f64(t));
        map_for_ten_minutes(&mut tracker, t, reached);
        let status = tracker.status();
        let map = status.map.unwrap();
        assert_eq!(
            (map.time, map.gained, map.state),
            (Duration::from_secs(600), gained, RunState::Last)
        );
        // The trial's play counts toward the rate just like the map's.
        assert_eq!(status.activity, Activity::Playing);
        assert_near(status.rate_per_hour, 0.12, 0.05);
    }

    #[test]
    fn gains_count_toward_the_run_they_were_earned_in() {
        let gained = |tracker: &XpTracker| tracker.status().map.unwrap().gained;
        let mut tracker = XpTracker::new();
        enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
        let (t, reached) = map_for_ten_minutes(&mut tracker, 0.0, 0.2);
        // The boss's hundredth of a level shows on the bar right before the portal; the median
        // filter passes it on only once the hideout has loaded.
        let boss = reached + 0.01;
        let t = play(&mut tracker, t, 1, |t| Some(as_read(boss, t)));
        enter(&mut tracker, "HideoutCanal", 1, t);
        let t = play(&mut tracker, t, 5, |_| None);
        let t = play(&mut tracker, t, 30, |t| Some(as_read(boss, t)));
        assert_near(Some(gained(&tracker)), 0.03, 0.05);
        // A campaign zone through a waypoint: what's earned there isn't the map's.
        let from_map = gained(&tracker);
        enter(&mut tracker, "G2_3", 42, t);
        let (t, reached) = map_for_ten_minutes(&mut tracker, t, boss);
        assert_eq!(gained(&tracker), from_map);
        // Back through the map's portal, the run earns again.
        enter(&mut tracker, "HideoutCanal", 1, t);
        enter(&mut tracker, "MapEpitaph", EPITAPH, t + 10.0);
        map_for_ten_minutes(&mut tracker, t + 10.0, reached);
        assert_near(Some(gained(&tracker)), 0.05, 0.05);
    }

    #[test]
    fn logging_out_forgets_the_map_runs() {
        let mut tracker = XpTracker::new();
        enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
        enter(&mut tracker, "HideoutCanal", 1, 100.0);
        enter(&mut tracker, "MapBluff", 17, 120.0);
        assert_eq!(tracker.status().map.unwrap().finished, 1);
        tracker.on_log_event(LogEvent::SceneLost, Duration::from_secs(200));
        // In again -- the login's area -- and back into the instance left open: a fresh run,
        // nothing finished before it.
        enter(&mut tracker, "HideoutCanal", 1, 260.0);
        assert_eq!(tracker.status().map, None);
        enter(&mut tracker, "MapBluff", 17, 270.0);
        play(&mut tracker, 270.0, 16, |_| None);
        assert_eq!(
            tracker.status().map,
            Some(MapStatus {
                time: Duration::from_secs(30),
                gained: 0.0,
                state: RunState::Running,
                finished: 0,
                average: None,
            })
        );
    }

    #[test]
    fn the_first_map_the_logs_tail_shows_is_neither_shown_nor_averaged() {
        let s = Duration::from_secs;
        let mut tracker = XpTracker::new();
        // Started in the hideout, with a map in the log's tail that may have begun before it.
        tracker.restore(
            [
                (s(100), area("MapEpitaph", EPITAPH)),
                (s(400), area("HideoutCanal", 1)),
            ],
            s(600),
        );
        enter(&mut tracker, "MapEpitaph", EPITAPH, 600.0);
        play(&mut tracker, 600.0, 60, |_| None);
        assert_eq!(tracker.status().map, None);
        enter(&mut tracker, "HideoutCanal", 1, 720.0);
        enter(&mut tracker, "MapBluff", 17, 730.0);
        play(&mut tracker, 730.0, 31, |_| None);
        assert_eq!(
            tracker.status().map,
            Some(MapStatus {
                time: s(60),
                gained: 0.0,
                state: RunState::Running,
                finished: 0,
                average: None,
            })
        );
    }

    #[test]
    fn the_logs_tail_times_the_runs_and_the_pause_it_shows_from_their_start() {
        // The owner's morning, 2026-09-24, in seconds: a map, then MapPort 1681640215 entered at
        // 08:29:57 and left for the hideout 3:27 later; the app restarted in the hideout about 45
        // minutes on, and the character then went back into the same map (09:24:50). Replayed at
        // the restart, the map was untimed and its plate never came back; the pause read «< 1 мин»
        // after 45 minutes.
        let s = Duration::from_secs;
        let mut tracker = XpTracker::new();
        tracker.restore(
            [
                (s(100), area("MapPort", 3753768427)),
                (s(400), area("HideoutCanal", 1)),
                (s(3700), area("MapPort", 1681640215)),
                (s(3907), area("HideoutCanal", 1)),
            ],
            s(6600),
        );
        let status = tracker.status();
        assert_eq!(
            status.activity,
            Activity::Paused {
                since: s(3907),
                elapsed: s(2693),
            }
        );
        // The first map may have begun before the tail: not averaged.
        assert_eq!(
            status.map,
            Some(MapStatus {
                time: s(207),
                gained: 0.0,
                state: RunState::Last,
                finished: 0,
                average: None,
            })
        );
        enter(&mut tracker, "MapPort", 1681640215, 6700.0);
        play(&mut tracker, 6700.0, 31, |_| None);
        let map = tracker.status().map.unwrap();
        assert_eq!((map.time, map.state), (s(267), RunState::Running));

        // After a login in the tail, the first map is timed too, up to the restart; and the idle
        // allowance starts at the restart, not at the map's entry.
        let mut tracker = XpTracker::new();
        tracker.restore(
            [
                (s(10), LogEvent::SceneLost),
                (s(20), area("HideoutCanal", 1)),
                (s(30), area("MapBluff", 17)),
            ],
            s(900),
        );
        let status = tracker.status();
        assert_eq!(status.activity, Activity::Playing);
        assert_eq!(
            status.map.map(|map| (map.time, map.state)),
            Some((s(870), RunState::Running))
        );
    }

    #[test]
    fn a_lines_tick_puts_it_on_the_overlays_clock() {
        let line = "2026/09/24 09:24:50 143185343 2caa229f [DEBUG Client 31244] Generating level 77 \
                    area \"MapPort\" with seed 1681640215";
        assert_eq!(
            parse_timed_log_line(line),
            Some((Some(143_185_343), area("MapPort", 1681640215)))
        );
        // The test machine, 2026-09-24: 144087359 on the line at 09:39:52, and GetTickCount64
        // 145499812 at 10:03:24.71.
        let now = Duration::from_millis(145_499_812);
        assert_eq!(
            log_time(Some(144_087_359), now),
            Duration::from_millis(144_087_359)
        );
        // 49.7 days on, the line's 32-bit tick has wrapped and the clock hasn't.
        let now = Duration::from_millis((1 << 32) + 5_000);
        assert_eq!(
            log_time(Some(u32::MAX - 999), now),
            Duration::from_millis((1 << 32) - 1_000)
        );
        // A line from before a reboot lands no later than now; a line without a tick, now.
        let early = Duration::from_millis(60_000);
        assert!(log_time(Some(144_087_359), early) <= early);
        assert_eq!(log_time(None, now), now);
    }

    /// `tracker` across an update's restart as the overlay carries it: to JSON and back, taken up
    /// at `at` seconds and caught up on the log's `tail` -- `None` if it isn't taken up.
    fn restarted(tracker: &XpTracker, tail: &[(Duration, LogEvent)], at: f64) -> Option<XpTracker> {
        let at = Duration::from_secs_f64(at);
        let mut carried = XpTracker::carried(&tracker.carry().unwrap(), at)?;
        carried.catch_up(tail.to_vec(), at);
        Some(carried)
    }

    #[test]
    fn an_update_restart_carries_the_rate_and_the_map_runs_on() {
        let s = Duration::from_secs;
        let rate = 0.12 / 3600.0;
        // A map, the hideout, and ten minutes into the next map when the app restarts for an
        // update, 22 s after its last look at the bar; the log's tail shows it all.
        let mut tracker = XpTracker::new();
        enter(&mut tracker, "MapBluff", 17, 0.0);
        let (t, reached) = map_for_ten_minutes(&mut tracker, 0.0, 0.2);
        enter(&mut tracker, "HideoutCanal", 1, t);
        enter(&mut tracker, "MapEpitaph", EPITAPH, t + 20.0);
        let (end, reached) = map_for_ten_minutes(&mut tracker, t + 20.0, reached);
        let tail = [
            (s(0), area("MapBluff", 17)),
            (s(600), area("HideoutCanal", 1)),
            (s(620), area("MapEpitaph", EPITAPH)),
        ];
        let restart = end + 20.0;
        let fraction = |t: f64| Some(as_read(reached + rate * (t - end), t));

        // Carried on: the rate is the one before, the finished map still averaged, and the
        // map's clock and experience ran on through the restart.
        let mut carried = restarted(&tracker, &tail, restart).expect("taken up");
        let last = play(&mut carried, restart, 30, fraction) - 2.0;
        let status = carried.status();
        assert_near(status.rate_per_hour, 0.12, 0.05);
        let map = status.map.unwrap();
        assert_eq!(
            (map.time, map.state, map.finished, map.average),
            (s(678), RunState::Running, 1, Some(s(600)))
        );
        assert_near(Some(map.gained), rate * (last - 620.0), 0.1);

        // Started over from the log's tail instead: no rate for the first two minutes of play,
        // the first map untimed, and nothing of what the map gave before the restart.
        let mut restored = XpTracker::new();
        restored.restore(tail, Duration::from_secs_f64(restart));
        play(&mut restored, restart, 30, fraction);
        let status = restored.status();
        assert_eq!(status.rate_per_hour, None);
        let map = status.map.unwrap();
        assert_eq!((map.finished, map.average), (0, None));
        assert!(map.gained < 0.005, "{}", map.gained);
    }

    #[test]
    fn a_carried_tracker_takes_up_what_the_log_said_during_the_restart() {
        let s = Duration::from_secs;
        let mut tracker = XpTracker::new();
        enter(&mut tracker, "MapEpitaph", EPITAPH, 0.0);
        map_for_ten_minutes(&mut tracker, 0.0, 0.2);
        // The last look at the bar at 598 s; the character went back to the hideout at 605 s,
        // while the app restarted, and the new copy took the tracker up at 615 s.
        let tail = [
            (s(0), area("MapEpitaph", EPITAPH)),
            (s(605), area("HideoutCanal", 1)),
        ];
        let status = restarted(&tracker, &tail, 615.0).unwrap().status();
        assert_eq!(
            status.activity,
            Activity::Paused {
                since: s(605),
                elapsed: s(10),
            }
        );
        let map = status.map.unwrap();
        assert_eq!(
            (map.time, map.state, map.finished),
            (s(605), RunState::Waiting, 0)
        );
        assert_near(status.rate_per_hour, 0.12, 0.05);

        // Logged out and in again meanwhile, maybe as another character: all starts over.
        let tail = [
            (s(0), area("MapEpitaph", EPITAPH)),
            (s(602), LogEvent::SceneLost),
            (s(610), area("HideoutCanal", 1)),
        ];
        let status = restarted(&tracker, &tail, 615.0).unwrap().status();
        assert_eq!((status.rate_per_hour, status.map), (None, None));
    }

    #[test]
    fn only_a_fresh_carry_of_a_tracker_as_this_version_keeps_one_is_taken_up() {
        let at = Duration::from_secs;
        let mut tracker = XpTracker::new();
        // The last look at the bar at 118 s.
        play(&mut tracker, 0.0, 60, |t| Some(as_read(0.3, t)));
        let json = tracker.carry().unwrap();
        assert!(
            XpTracker::carried(&json, at(118 + 120)).is_some(),
            "two minutes on"
        );
        assert!(
            XpTracker::carried(&json, at(118 + 121)).is_none(),
            "from some other run"
        );
        assert!(
            XpTracker::carried(&json, at(60)).is_none(),
            "Windows restarted since: its clock started over"
        );
        // Another version's tracker, with a field this one doesn't keep, isn't misread; nor is a
        // file cut short.
        let mut other: serde_json::Value = serde_json::from_slice(&json).unwrap();
        other["rested_bonus"] = serde_json::json!(0.5);
        let other = serde_json::to_vec(&other).unwrap();
        assert!(XpTracker::carried(&other, at(130)).is_none());
        assert!(XpTracker::carried(&json[..json.len() / 2], at(130)).is_none());
        // A tracker never told anything has nothing to carry on with.
        let untold = XpTracker::new().carry().unwrap();
        assert!(XpTracker::carried(&untold, at(130)).is_none());
    }

    #[test]
    fn wording() {
        assert_eq!(format_rate(0.1244), "+12,4 %/ч");
        assert_eq!(format_rate(0.0325), "+3,25 %/ч");
        assert_eq!(format_rate(1.5), "+150 %/ч");
        assert_eq!(format_percent(0.64752), "64,8");
        // Two decimals below 1 too, not the panel's two significant digits (`0,065`).
        assert_eq!(format_percent(0.000654), "0,07");
        i18n::with_lang(i18n::Lang::English, || {
            assert_eq!(format_rate(0.1244), "+12.4%/h");
            assert_eq!(format_rate(0.0325), "+3.25%/h");
            assert_eq!(format_rate(1.5), "+150%/h");
            assert_eq!(format_percent(0.64752), "64.8");
        });
    }

    /// `words` as a plate shows them, a space apart.
    fn read(words: &[Word]) -> String {
        words.iter().map(Word::text).collect::<Vec<_>>().join(" ")
    }

    /// What the plates read for `status` in `wording`, the percent switched on, the parts a
    /// diamond apart as `ui::xp_overlay` draws them -- `◆` here: the level plate ([`level_parts`])
    /// and, `|` after it, the map plate when there is a run to show.
    fn plates(status: &XpStatus, wording: Wording) -> String {
        let paused = matches!(status.activity, Activity::Paused { .. });
        let level: Vec<String> = level_parts(status, true, wording)
            .iter()
            .map(|words| read(words))
            .collect();
        let level = level.join(" ◆ ");
        match status.map {
            Some(map) => format!("{level} | {}", read(&map_words(&map, paused, wording))),
            None => level,
        }
    }

    #[test]
    fn the_plates_read_in_the_interface_language_in_full_or_short() {
        let map = MapStatus {
            time: Duration::from_secs(4 * 60 + 7),
            gained: 0.012,
            state: RunState::Running,
            finished: 3,
            average: Some(Duration::from_secs(6 * 60 + 30)),
        };
        let playing = XpStatus {
            fraction: Some(0.648),
            rate_per_hour: Some(0.124),
            level: Some(74),
            activity: Activity::Playing,
            map: Some(map),
        };
        // An ascendancy trial five minutes on: play, and the map left for it is the last one.
        let trial = XpStatus {
            map: Some(MapStatus {
                state: RunState::Last,
                ..map
            }),
            ..playing
        };
        let paused = XpStatus {
            activity: Activity::Paused {
                since: Duration::ZERO,
                elapsed: Duration::from_secs(12 * 60),
            },
            map: Some(MapStatus {
                time: Duration::from_secs(9 * 60),
                gained: 0.0366,
                state: RunState::Last,
                ..map
            }),
            ..playing
        };
        let measuring = XpStatus {
            rate_per_hour: None,
            map: None,
            ..playing
        };
        let level_unknown = XpStatus {
            level: None,
            map: None,
            ..playing
        };
        let nothing_gained = XpStatus {
            rate_per_hour: Some(0.0),
            map: None,
            ..playing
        };
        let states = [
            &playing,
            &trial,
            &paused,
            &measuring,
            &level_unknown,
            &nothing_gained,
        ];
        let full = |status: &XpStatus| plates(status, Wording::Full);
        let short = |status: &XpStatus| plates(status, Wording::Short);
        assert_eq!(
            i18n::with_lang(i18n::Lang::Russian, || states.map(full)),
            [
                "64,8 % ◆ +12,4 %/ч · до 75 ур. 2 ч 50 мин | карта 4:07 +1,2 % · ср. 6:30",
                "64,8 % ◆ +12,4 %/ч · до 75 ур. 2 ч 50 мин | последняя карта 4:07 +1,2 % · ср. 6:30",
                "64,8 % | последняя карта 9:00 +3,66 %",
                "64,8 % ◆ замер скорости…",
                "64,8 % ◆ +12,4 %/ч · до ур. 2 ч 50 мин",
                "64,8 % ◆ +0 %/ч · до 75 ур. —",
            ]
        );
        assert_eq!(
            i18n::with_lang(i18n::Lang::English, || states.map(full)),
            [
                "64.8% ◆ +12.4%/h · level 75 in 2h 50m | map 4:07 +1.2% · avg 6:30",
                "64.8% ◆ +12.4%/h · level 75 in 2h 50m | last map 4:07 +1.2% · avg 6:30",
                "64.8% | last map 9:00 +3.66%",
                "64.8% ◆ measuring rate…",
                "64.8% ◆ +12.4%/h · next level in 2h 50m",
                "64.8% ◆ +0%/h · level 75 in —",
            ]
        );
        // Short: no level to reach, no map average, and a run that is over is just a map.
        assert_eq!(
            i18n::with_lang(i18n::Lang::Russian, || {
                [&playing, &trial, &paused, &nothing_gained].map(short)
            }),
            [
                "64,8 % ◆ +12,4 %/ч · 2 ч 50 мин | карта 4:07 +1,2 %",
                "64,8 % ◆ +12,4 %/ч · 2 ч 50 мин | карта 4:07 +1,2 %",
                "64,8 % | карта 9:00 +3,66 %",
                "64,8 % ◆ +0 %/ч · —",
            ]
        );
    }

    #[test]
    fn clock_wording() {
        assert_eq!(format_clock(Duration::from_secs(7)), "0:07");
        assert_eq!(format_clock(Duration::from_secs(4 * 60 + 7)), "4:07");
        // Whole seconds elapsed: the hour doesn't show before it has passed.
        assert_eq!(format_clock(Duration::from_millis(3_599_900)), "59:59");
        assert_eq!(
            format_clock(Duration::from_secs(3600 + 2 * 60 + 3)),
            "1:02:03"
        );
    }
}
