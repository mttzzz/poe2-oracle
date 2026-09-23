//! The player's settings. The settings window (`ui::settings_view`) changes them a control at a
//! time, each change saved and applied at once; the rest of the app reads them at startup and after
//! every change.
//!
//! Stored as JSON in `paths::settings_file` -- `%APPDATA%\poe2-oracle\config\settings.json` on
//! Windows (directories 5.0.1's `win.rs` joins the roaming app-data folder, the project path and
//! `config`). Loading never fails: no file means
//! defaults, and a file that can't be read or parsed is set aside as `settings.json.bak` -- kept
//! for the player, out of the next save's way -- in favour of defaults. Fields the file lacks take
//! their defaults, fields this version doesn't know are ignored, and a choice this version doesn't
//! know (a newer version's, read back after a downgrade) falls back to its default alone instead
//! of costing the whole file.
//!
//! Plain data and file I/O, no Windows API: it builds and is tested on every target.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::Path;

use anyhow::{Context as _, Result, anyhow};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use item_parser::ItemLanguage;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use trade_client::ListingStatus;

use crate::overlay_layout::PanelPositions;
use crate::paths;

/// The file layout [`Settings`] reads and writes. A change to what an existing field means bumps
/// it, and [`load`] converts older files before handing them out. 2: new checks search instant
/// buyouts by default, and a version-1 file's old default moves there.
pub const SETTINGS_VERSION: u32 = 2;

/// The smallest [`Settings::ui_scale`].
pub const MIN_UI_SCALE: f32 = 0.8;
/// The largest [`Settings::ui_scale`].
pub const MAX_UI_SCALE: f32 = 1.5;
/// The averaging windows the XP rate offers, in minutes: the half-life of its weighting.
pub const XP_RATE_WINDOWS: [u16; 4] = [5, 10, 20, 30];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    /// The layout the file was written in; always [`SETTINGS_VERSION`] once loaded.
    pub version: u32,
    /// Which league searches run in.
    #[serde(deserialize_with = "or_default")]
    pub league: LeagueChoice,
    /// The game client's language: the language copied items arrive in.
    #[serde(deserialize_with = "or_default")]
    pub client_language: ClientLanguage,
    /// The language of the app's own words (`crate::i18n`).
    #[serde(deserialize_with = "or_default")]
    pub interface_language: InterfaceLanguage,
    /// Start with Windows. The registry is the truth here -- the installer and Task Manager change
    /// it too -- so read `platform::autostart::autostart_enabled` into this before showing it.
    pub autostart: bool,
    /// The global price-check hotkey; passes [`Hotkey::check`] once loaded.
    #[serde(deserialize_with = "or_default")]
    pub hotkey: Hotkey,
    /// The sellers a new price check searches; the panel's chip still switches them per item.
    #[serde(deserialize_with = "or_default")]
    pub listing_status: ListingStatusChoice,
    /// The seller column of the results table.
    pub show_seller_column: bool,
    /// Overlay size factor, [`MIN_UI_SCALE`] to [`MAX_UI_SCALE`].
    pub ui_scale: f32,
    /// The experience-rate / level-ETA overlay.
    pub xp_overlay: bool,
    /// The overlay also shows how much of the level is earned.
    pub xp_show_percent: bool,
    /// The overlay also shows the current map's time, what it earned and the session's average
    /// map time.
    pub xp_map_timer: bool,
    /// Minutes the experience rate mostly averages over (the half-life of its weighting), one of
    /// [`XP_RATE_WINDOWS`]: shorter reacts to a change of farming sooner, longer reads steadier.
    pub xp_rate_window_minutes: u16,
    /// Look for a new version of the app.
    pub check_updates: bool,
    /// The player's marks on waystone modifiers -- EE2's map check -- by trade stat id, the same
    /// on every client language.
    #[serde(deserialize_with = "or_default")]
    pub waystone_marks: BTreeMap<String, WaystoneMark>,
    /// Hotkeys that type into the game: chat commands and stash searches, at most
    /// [`MAX_QUICK_ACTIONS`].
    #[serde(deserialize_with = "or_default")]
    pub quick_actions: Vec<QuickAction>,
    /// Where the player dragged the price panel, beside the inventory and beside the stash; a
    /// side without one keeps EE2's placement.
    #[serde(deserialize_with = "or_default")]
    pub panel_positions: PanelPositions,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            league: LeagueChoice::default(),
            client_language: ClientLanguage::default(),
            interface_language: InterfaceLanguage::default(),
            autostart: false,
            hotkey: Hotkey::default(),
            listing_status: ListingStatusChoice::default(),
            show_seller_column: true,
            ui_scale: 1.0,
            xp_overlay: true,
            xp_show_percent: false,
            xp_map_timer: true,
            xp_rate_window_minutes: 10,
            check_updates: true,
            waystone_marks: BTreeMap::new(),
            // The command every player types most, ready for a hotkey.
            quick_actions: vec![QuickAction {
                kind: QuickActionKind::ChatCommand,
                text: "/hideout".to_owned(),
                hotkey: None,
            }],
            panel_positions: PanelPositions::default(),
        }
    }
}

impl Settings {
    /// A loaded file under this version's rules: an older layout's values carried over, the
    /// current layout version (what a save writes back), numbers inside the ranges the settings
    /// window offers, and the default hotkey in place of one [`Hotkey::check`] rejects -- a
    /// hand-edited file can hold anything.
    fn normalized(mut self) -> Settings {
        // Version 1 searched in-person sellers too by default. A file still on that default moves
        // to instant buyout once: saved as version 2 from then on, a player's switch back stays.
        if self.version < 2 && self.listing_status == ListingStatusChoice::Available {
            self.listing_status = ListingStatusChoice::Securable;
        }
        self.version = SETTINGS_VERSION;
        // A typed league as typed, minus the spaces around it; nothing typed is no choice.
        self.league = match std::mem::take(&mut self.league) {
            LeagueChoice::Custom(name) if name.trim().is_empty() => LeagueChoice::Auto,
            LeagueChoice::Custom(name) => LeagueChoice::Custom(name.trim().to_owned()),
            choice => choice,
        };
        self.ui_scale = self.ui_scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE);
        // The nearest offered window: a hand-edited 15 reads as 10 or 20, whichever is closer.
        self.xp_rate_window_minutes = XP_RATE_WINDOWS
            .into_iter()
            .min_by_key(|window| window.abs_diff(self.xp_rate_window_minutes))
            .unwrap_or(10);
        if self.hotkey.check().is_err() {
            self.hotkey = Hotkey::default();
        }
        // Actions keep only a usable hotkey that nothing earlier in the list, the price check
        // included, already has.
        let mut taken = vec![self.hotkey];
        self.quick_actions.retain_mut(|action| {
            action.text = action.text.trim().to_owned();
            if let Some(hotkey) = action.hotkey {
                if hotkey.check().is_err() || taken.contains(&hotkey) {
                    action.hotkey = None;
                } else {
                    taken.push(hotkey);
                }
            }
            !action.text.is_empty()
        });
        self.quick_actions.truncate(MAX_QUICK_ACTIONS);
        self
    }
}

/// The most quick actions the settings keep.
pub const MAX_QUICK_ACTIONS: usize = 12;

/// A hotkey that types into the game for the player -- EE2's chat commands and stash searches.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct QuickAction {
    #[serde(default, deserialize_with = "or_default")]
    pub kind: QuickActionKind,
    /// The command (`/hideout`) or the search string (a poe2.re regex).
    #[serde(default)]
    pub text: String,
    /// `None` until the player records one.
    #[serde(default, deserialize_with = "or_default")]
    pub hotkey: Option<Hotkey>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum QuickActionKind {
    /// Sent to the game's chat: Enter, the text, Enter.
    #[default]
    ChatCommand,
    /// Pasted into the open stash's (or a vendor's) search box: Ctrl+F, the text, Enter.
    StashSearch,
}

/// How the player rates a waystone modifier; a checked waystone shows its marked mods in the
/// mark's colour.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WaystoneMark {
    /// Worth skipping the map for.
    Danger,
    /// Worth a second look.
    Warning,
    /// Worth running the map for.
    Wanted,
}

impl WaystoneMark {
    /// The mark a click moves to: none, danger, warning, wanted, and back to none.
    pub fn next(mark: Option<WaystoneMark>) -> Option<WaystoneMark> {
        match mark {
            None => Some(WaystoneMark::Danger),
            Some(WaystoneMark::Danger) => Some(WaystoneMark::Warning),
            Some(WaystoneMark::Warning) => Some(WaystoneMark::Wanted),
            Some(WaystoneMark::Wanted) => None,
        }
    }
}

/// Which league searches run in.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum LeagueChoice {
    /// The trade site's current league: the first one `GET /api/trade2/data/leagues` lists.
    #[default]
    Auto,
    /// A league the player picked by name from the trade site's list.
    Named(String),
    /// A league the player typed in: a private one (`My League (PL12345)`), which the site's list
    /// never shows -- searched as typed, listed or not. The site answers searches in it only for
    /// a signed-in player (`crate::session`).
    Custom(String),
}

impl LeagueChoice {
    /// The league to search, given the trade site's leagues in its order. A picked league the site
    /// no longer lists (it ended) gives way to the current one; without a list (it failed to load)
    /// the pick is used as is. A typed league is always used as typed. `None` only for `Auto`
    /// without a list.
    pub fn resolve<'a>(&'a self, listed: &'a [String]) -> Option<&'a str> {
        match self {
            LeagueChoice::Custom(name) if !name.trim().is_empty() => Some(name.trim()),
            LeagueChoice::Named(name) if listed.is_empty() || listed.contains(name) => {
                Some(name.as_str())
            }
            _ => listed.first().map(String::as_str),
        }
    }
}

/// The game client's language, which copied item text arrives in.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClientLanguage {
    /// Recognized from each copied item's first line (`item_parser::detect_item_language`).
    #[default]
    Auto,
    Russian,
    English,
}

impl ClientLanguage {
    /// The language to parse copied items in; `None` for [`ClientLanguage::Auto`]: detect it.
    pub fn item_language(self) -> Option<ItemLanguage> {
        match self {
            ClientLanguage::Auto => None,
            ClientLanguage::Russian => Some(ItemLanguage::Russian),
            ClientLanguage::English => Some(ItemLanguage::English),
        }
    }
}

/// The language of the app's own words: labels, messages, number formats (`crate::i18n`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceLanguage {
    /// The game client's language, or before the game's first run the Windows display language
    /// (`i18n::resolve`).
    #[default]
    Auto,
    Russian,
    English,
}

/// The sellers a price check searches: the choices the panel's "Продавцы" chip steps through
/// (`PriceCheckApp::cycle_listing_status`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum ListingStatusChoice {
    /// Instant buyout and in person.
    Available,
    /// Instant buyout only: the game's own auction sells without the seller online.
    #[default]
    Securable,
    /// In person, seller online.
    Online,
    /// Offline sellers too.
    Any,
}

impl From<ListingStatusChoice> for ListingStatus {
    fn from(choice: ListingStatusChoice) -> Self {
        match choice {
            ListingStatusChoice::Available => ListingStatus::Available,
            ListingStatusChoice::Securable => ListingStatus::Securable,
            ListingStatusChoice::Online => ListingStatus::Online,
            ListingStatusChoice::Any => ListingStatus::Any,
        }
    }
}

/// The price-check hotkey: Ctrl, Shift and Alt, each held or not, plus one [`KeyName`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hotkey {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
    pub key: KeyName,
}

impl Default for Hotkey {
    /// `Ctrl+E`, the binding the player already uses in EE2.
    fn default() -> Self {
        Hotkey {
            ctrl: true,
            shift: false,
            alt: false,
            key: KeyName(b'E'),
        }
    }
}

/// Why a combination can't be the price-check hotkey. A registered global hotkey takes its
/// combination away from every program while the app runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HotkeyProblem {
    /// A letter or digit without Ctrl or Alt: that key (or, with Shift, its capital) would stop
    /// typing everywhere, game chat included. F-keys may stand alone.
    NeedsModifier,
    /// Ctrl with C: the price check itself makes the game copy the item by sending Ctrl+C or
    /// Ctrl+Alt+C (`platform::synth_input`), and a hotkey on it would swallow that very copy.
    CopyCombo,
    /// Alt with F4: closing the focused window, the game included.
    CloseCombo,
}

impl Hotkey {
    /// Whether this combination can be registered as the price-check hotkey.
    pub fn check(self) -> Result<(), HotkeyProblem> {
        if self.ctrl && self.key == KeyName(b'C') {
            Err(HotkeyProblem::CopyCombo)
        } else if self.alt && self.key == KeyName(VK_F1 + 3) {
            Err(HotkeyProblem::CloseCombo)
        } else if !self.key.is_function_key() && !self.ctrl && !self.alt {
            Err(HotkeyProblem::NeedsModifier)
        } else {
            Ok(())
        }
    }

    /// The hotkey for `GlobalHotKeyManager::register`/`unregister`.
    pub fn to_global(self) -> HotKey {
        let mut modifiers = Modifiers::empty();
        modifiers.set(Modifiers::CONTROL, self.ctrl);
        modifiers.set(Modifiers::SHIFT, self.shift);
        modifiers.set(Modifiers::ALT, self.alt);
        HotKey::new(Some(modifiers), self.key.code())
    }
}

/// `Ctrl+Shift+Alt+E` -- the modifiers held, in [`modifier_names`] order, then the key.
impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for name in modifier_names(self.ctrl, self.shift, self.alt) {
            write!(f, "{name}+")?;
        }
        write!(f, "{}", self.key)
    }
}

/// The names of the held modifiers in the order hotkeys spell them: `Ctrl`, `Shift`, `Alt`.
pub fn modifier_names(ctrl: bool, shift: bool, alt: bool) -> impl Iterator<Item = &'static str> {
    [(ctrl, "Ctrl"), (shift, "Shift"), (alt, "Alt")]
        .into_iter()
        .filter_map(|(held, name)| held.then_some(name))
}

/// Windows' virtual-key code for F1; F2-F12 follow it (`winuser.h`: `VK_F1` 0x70 .. `VK_F12` 0x7B).
const VK_F1: u8 = 0x70;
const VK_F12: u8 = VK_F1 + 11;

/// A hotkey's main key: a letter A-Z, a digit 0-9 or F1-F12 -- keys `RegisterHotKey` takes as they
/// are, and letters stay on the same physical keys in the Russian layout. Held as its Windows
/// virtual-key code (letters and digits are their ASCII codes); stored as its label, `"E"`, `"7"`
/// or `"F5"`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KeyName(u8);

impl KeyName {
    /// The key a label names, in either case: `"E"`/`"e"`, `"7"`, `"F5"`/`"f5"` -- this type's own
    /// labels and GPUI's key names (`Keystroke::key`, lower case).
    pub fn from_label(label: &str) -> Option<KeyName> {
        match *label.as_bytes() {
            [c] if c.is_ascii_alphanumeric() => Some(KeyName(c.to_ascii_uppercase())),
            [b'F' | b'f', digit @ b'1'..=b'9'] => Some(KeyName(VK_F1 + (digit - b'1'))),
            [b'F' | b'f', b'1', digit @ b'0'..=b'2'] => Some(KeyName(VK_F1 + 9 + (digit - b'0'))),
            _ => None,
        }
    }

    /// The key with this Windows virtual-key code, if it's one of the supported keys.
    pub fn from_virtual_key(vk: u16) -> Option<KeyName> {
        let vk = u8::try_from(vk).ok()?;
        (vk.is_ascii_uppercase() || vk.is_ascii_digit() || (VK_F1..=VK_F12).contains(&vk))
            .then_some(KeyName(vk))
    }

    /// Windows' virtual-key code for the key.
    pub fn virtual_key(self) -> u16 {
        self.0.into()
    }

    fn is_function_key(self) -> bool {
        self.0 >= VK_F1
    }

    /// The key as `global-hotkey` names it.
    fn code(self) -> Code {
        const LETTERS: [Code; 26] = [
            Code::KeyA,
            Code::KeyB,
            Code::KeyC,
            Code::KeyD,
            Code::KeyE,
            Code::KeyF,
            Code::KeyG,
            Code::KeyH,
            Code::KeyI,
            Code::KeyJ,
            Code::KeyK,
            Code::KeyL,
            Code::KeyM,
            Code::KeyN,
            Code::KeyO,
            Code::KeyP,
            Code::KeyQ,
            Code::KeyR,
            Code::KeyS,
            Code::KeyT,
            Code::KeyU,
            Code::KeyV,
            Code::KeyW,
            Code::KeyX,
            Code::KeyY,
            Code::KeyZ,
        ];
        const DIGITS: [Code; 10] = [
            Code::Digit0,
            Code::Digit1,
            Code::Digit2,
            Code::Digit3,
            Code::Digit4,
            Code::Digit5,
            Code::Digit6,
            Code::Digit7,
            Code::Digit8,
            Code::Digit9,
        ];
        const FUNCTION_KEYS: [Code; 12] = [
            Code::F1,
            Code::F2,
            Code::F3,
            Code::F4,
            Code::F5,
            Code::F6,
            Code::F7,
            Code::F8,
            Code::F9,
            Code::F10,
            Code::F11,
            Code::F12,
        ];
        match self.0 {
            b'A'..=b'Z' => LETTERS[usize::from(self.0 - b'A')],
            b'0'..=b'9' => DIGITS[usize::from(self.0 - b'0')],
            vk => FUNCTION_KEYS[usize::from(vk - VK_F1)],
        }
    }
}

impl fmt::Display for KeyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_function_key() {
            write!(f, "F{}", self.0 - VK_F1 + 1)
        } else {
            write!(f, "{}", char::from(self.0))
        }
    }
}

impl TryFrom<String> for KeyName {
    type Error = String;

    fn try_from(label: String) -> Result<Self, Self::Error> {
        KeyName::from_label(&label).ok_or_else(|| format!("unsupported hotkey key {label:?}"))
    }
}

impl From<KeyName> for String {
    fn from(key: KeyName) -> Self {
        key.to_string()
    }
}

/// Reads a choice this version doesn't know -- a variant or key a newer version added, read back
/// after a downgrade -- as the type's default instead of failing the whole file. Only for types
/// whose `Default` is also their [`Settings::default`] value.
fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).unwrap_or_default())
}

/// This user's settings: the saved file, or defaults when there is none or it's unusable (see the
/// module doc). Values come back within the ranges the settings window offers.
pub fn load() -> Settings {
    match paths::settings_file() {
        Some(path) => load_from(&path),
        None => Settings::default(),
    }
}

/// Writes `settings` for [`load`] to find: into a temporary file, flushed to disk, then renamed over
/// the old one, so a crash or power loss mid-save leaves the previous settings, never half of the
/// new ones.
pub fn save(settings: &Settings) -> Result<()> {
    let path =
        paths::settings_file().ok_or_else(|| anyhow!("no settings directory for this user"))?;
    save_to(&path, settings)
}

fn load_from(path: &Path) -> Settings {
    let parsed = match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Settings>(&text).map_err(anyhow::Error::from),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Settings::default(),
        Err(err) => Err(err.into()),
    };
    match parsed {
        Ok(settings) => settings.normalized(),
        Err(err) => {
            let backup = path.with_extension("json.bak");
            log::warn!(
                "{} is unusable ({err:#}), setting it aside as {} and using defaults",
                path.display(),
                backup.display()
            );
            if let Err(err) = fs::rename(path, &backup) {
                log::warn!("setting {} aside failed: {err}", path.display());
            }
            Settings::default()
        }
    }
}

fn save_to(path: &Path, settings: &Settings) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let body = serde_json::to_vec_pretty(settings).context("serializing settings")?;
    let temp = path.with_extension("json.tmp");
    let mut file = File::create(&temp).with_context(|| format!("creating {}", temp.display()))?;
    file.write_all(&body)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("writing {}", temp.display()))?;
    // Windows can't rename a file that is still open.
    drop(file);
    fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A fresh directory under the system temp dir, removed on drop: this crate has no `tempfile`
    /// dependency, the same hand-rolled scheme as `trade_client::cache`'s tests.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> TempDir {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            TempDir(std::env::temp_dir().join(format!(
                "poe2-oracle-settings-test-{}-{n}",
                std::process::id()
            )))
        }

        /// Nested, so saving also has to create the directories.
        fn settings_file(&self) -> PathBuf {
            self.0.join("config").join("settings.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_file(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn key(label: &str) -> KeyName {
        KeyName::from_label(label).unwrap()
    }

    #[test]
    fn saved_settings_load_back() {
        let dir = TempDir::new();
        let path = dir.settings_file();
        assert_eq!(load_from(&path), Settings::default(), "no file yet");

        let settings = Settings {
            league: LeagueChoice::Named("HC Forbidden Rites".to_owned()),
            client_language: ClientLanguage::Russian,
            autostart: true,
            hotkey: Hotkey {
                ctrl: false,
                shift: true,
                alt: true,
                key: key("7"),
            },
            listing_status: ListingStatusChoice::Any,
            show_seller_column: false,
            ui_scale: 1.25,
            xp_overlay: false,
            xp_show_percent: true,
            xp_map_timer: false,
            xp_rate_window_minutes: 30,
            check_updates: false,
            waystone_marks: BTreeMap::from([
                ("explicit.stat_1".to_owned(), WaystoneMark::Danger),
                ("explicit.stat_2".to_owned(), WaystoneMark::Wanted),
            ]),
            panel_positions: PanelPositions {
                inventory: Some(0.3125),
                stash: None,
            },
            ..Settings::default()
        };
        save_to(&path, &settings).unwrap();
        assert_eq!(load_from(&path), settings);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the temporary file becomes the settings file"
        );
    }

    #[test]
    fn missing_fields_take_defaults_and_unknown_ones_are_ignored() {
        let dir = TempDir::new();
        let path = dir.settings_file();
        write_file(
            &path,
            r#"{"hotkey": {"alt": true, "key": "F5"}, "show_seller_column": false, "added_later": [1, 2]}"#,
        );

        let expected = Settings {
            hotkey: Hotkey {
                ctrl: false,
                shift: false,
                alt: true,
                key: key("F5"),
            },
            show_seller_column: false,
            ..Settings::default()
        };
        assert_eq!(load_from(&path), expected);
    }

    #[test]
    fn corrupt_file_is_set_aside_for_defaults() {
        let dir = TempDir::new();
        let path = dir.settings_file();
        write_file(&path, r#"{"league": "#);

        assert_eq!(load_from(&path), Settings::default());
        assert!(!path.exists(), "the next save must not find it in the way");
        assert_eq!(
            fs::read_to_string(path.with_extension("json.bak")).unwrap(),
            r#"{"league": "#
        );
    }

    #[test]
    fn unknown_choice_falls_back_alone() {
        // Choices a newer version might write, read back after a downgrade.
        let dir = TempDir::new();
        let path = dir.settings_file();
        write_file(
            &path,
            r#"{
                "league": {"named": "Standard"},
                "client_language": "german",
                "hotkey": {"ctrl": true, "key": "NumPad1"},
                "listing_status": "online_league",
                "xp_overlay": false
            }"#,
        );

        let expected = Settings {
            league: LeagueChoice::Named("Standard".to_owned()),
            xp_overlay: false,
            ..Settings::default()
        };
        assert_eq!(load_from(&path), expected);
        assert!(path.exists(), "a readable file stays where it is");
    }

    #[test]
    fn a_version_1_file_loads_with_instant_buyout_once() {
        let dir = TempDir::new();
        let path = dir.settings_file();
        // As version 1 saved it, with the value tolerance and the buyers' requests it had then.
        write_file(
            &path,
            r#"{
                "version": 1,
                "league": {"named": "Standard"},
                "client_language": "russian",
                "autostart": false,
                "hotkey": {"ctrl": true, "shift": false, "alt": false, "key": "E"},
                "search_tolerance_percent": 10,
                "listing_status": "available",
                "show_seller_column": false,
                "ui_scale": 1.25,
                "xp_overlay": true,
                "xp_show_percent": false,
                "xp_map_timer": true,
                "xp_rate_window_minutes": 10,
                "check_updates": true,
                "waystone_marks": {"explicit.stat_1": "danger"},
                "quick_actions": [{"kind": "chat_command", "text": "/hideout", "hotkey": null}],
                "trade_requests": true,
                "trade_sound": true
            }"#,
        );
        let loaded = load_from(&path);
        assert!(path.exists(), "a readable old file stays where it is");
        assert_eq!(
            loaded,
            Settings {
                league: LeagueChoice::Named("Standard".to_owned()),
                client_language: ClientLanguage::Russian,
                listing_status: ListingStatusChoice::Securable,
                show_seller_column: false,
                ui_scale: 1.25,
                waystone_marks: BTreeMap::from([(
                    "explicit.stat_1".to_owned(),
                    WaystoneMark::Danger
                )]),
                ..Settings::default()
            }
        );

        // Saved as version 2, the player's switch back to both kinds of sellers stays.
        let chosen = Settings {
            listing_status: ListingStatusChoice::Available,
            ..loaded
        };
        save_to(&path, &chosen).unwrap();
        assert_eq!(load_from(&path), chosen);

        // A version 1 file on another choice keeps it.
        write_file(&path, r#"{"version": 1, "listing_status": "online"}"#);
        assert_eq!(load_from(&path).listing_status, ListingStatusChoice::Online);
    }

    #[test]
    fn a_typed_league_loads_trimmed_and_an_empty_one_as_auto() {
        let dir = TempDir::new();
        let path = dir.settings_file();
        write_file(&path, r#"{"league": {"custom": " My League (PL12345) "}}"#);
        assert_eq!(
            load_from(&path).league,
            LeagueChoice::Custom("My League (PL12345)".to_owned())
        );
        write_file(&path, r#"{"league": {"custom": "  "}}"#);
        assert_eq!(load_from(&path).league, LeagueChoice::Auto);
    }

    #[test]
    fn loaded_values_are_brought_into_range() {
        let dir = TempDir::new();
        let path = dir.settings_file();
        write_file(
            &path,
            r#"{"version": 7, "ui_scale": 3.0, "hotkey": {"key": "E"}, "xp_rate_window_minutes": 27}"#,
        );
        let loaded = load_from(&path);
        assert_eq!(
            loaded.version, SETTINGS_VERSION,
            "a save writes this layout"
        );
        assert_eq!(loaded.ui_scale, MAX_UI_SCALE);
        assert_eq!(
            loaded.xp_rate_window_minutes, 30,
            "the nearest window offered"
        );
        assert_eq!(
            loaded.hotkey,
            Hotkey::default(),
            "a bare E fails Hotkey::check"
        );

        write_file(&path, r#"{"ui_scale": 0.1, "xp_rate_window_minutes": 0}"#);
        let loaded = load_from(&path);
        assert_eq!(loaded.ui_scale, MIN_UI_SCALE);
        assert_eq!(loaded.xp_rate_window_minutes, 5);
    }

    #[test]
    fn quick_actions_keep_real_text_and_free_hotkeys_only() {
        let dir = TempDir::new();
        let path = dir.settings_file();
        // Ctrl+E is the price check's; F5 is claimed twice; one entry has no text at all.
        write_file(
            &path,
            r#"{"quick_actions": [
                {"kind": "chat_command", "text": " /hideout ", "hotkey": {"key": "F5"}},
                {"kind": "stash_search", "text": "\"rare\"", "hotkey": {"key": "F5"}},
                {"kind": "chat_command", "text": "/exit", "hotkey": {"ctrl": true, "key": "E"}},
                {"kind": "stash_search", "text": "   ", "hotkey": {"key": "F6"}},
                {"kind": "teleport", "text": "/remaining"}
            ]}"#,
        );
        let actions = load_from(&path).quick_actions;
        let summary: Vec<_> = actions
            .iter()
            .map(|action| (action.kind, action.text.as_str(), action.hotkey))
            .collect();
        let f5 = Hotkey {
            ctrl: false,
            shift: false,
            alt: false,
            key: key("F5"),
        };
        assert_eq!(
            summary,
            [
                (QuickActionKind::ChatCommand, "/hideout", Some(f5)),
                (QuickActionKind::StashSearch, "\"rare\"", None),
                (QuickActionKind::ChatCommand, "/exit", None),
                // A kind this version doesn't know reads as a chat command.
                (QuickActionKind::ChatCommand, "/remaining", None),
            ]
        );
    }

    #[test]
    fn hotkey_rules() {
        let hotkey = |ctrl, shift, alt, label| Hotkey {
            ctrl,
            shift,
            alt,
            key: key(label),
        };
        assert_eq!(hotkey(true, false, false, "E").check(), Ok(()));
        assert_eq!(hotkey(false, false, true, "Q").check(), Ok(()));
        assert_eq!(hotkey(false, false, false, "F5").check(), Ok(()));
        assert_eq!(hotkey(true, true, false, "4").check(), Ok(()));
        assert_eq!(
            hotkey(false, false, false, "E").check(),
            Err(HotkeyProblem::NeedsModifier)
        );
        assert_eq!(
            hotkey(false, true, false, "1").check(),
            Err(HotkeyProblem::NeedsModifier)
        );
        assert_eq!(
            hotkey(true, false, true, "C").check(),
            Err(HotkeyProblem::CopyCombo)
        );
        assert_eq!(
            hotkey(false, false, true, "F4").check(),
            Err(HotkeyProblem::CloseCombo)
        );
        assert_eq!(
            hotkey(true, true, true, "E").to_string(),
            "Ctrl+Shift+Alt+E"
        );
    }

    #[test]
    fn every_key_registers_as_itself() {
        let labels = ('A'..='Z')
            .chain('0'..='9')
            .map(String::from)
            .chain((1..=12).map(|n| format!("F{n}")));
        for label in labels {
            let parsed = key(&label);
            assert_eq!(parsed.to_string(), label);
            assert_eq!(KeyName::from_label(&label.to_lowercase()), Some(parsed));
            assert_eq!(
                KeyName::from_virtual_key(parsed.virtual_key()),
                Some(parsed)
            );
            // global-hotkey's own label parser is the independent reference for the code table.
            let registered = Hotkey {
                ctrl: false,
                shift: false,
                alt: false,
                key: parsed,
            }
            .to_global();
            assert_eq!(
                registered,
                HotKey::try_from(label.as_str()).unwrap(),
                "{label}"
            );
        }
        assert_eq!(key("E").virtual_key(), 0x45);
        assert_eq!(key("F12").virtual_key(), 0x7B);
        for rejected in ["", "F0", "F13", "f1 ", "Й", "Esc", "10"] {
            assert_eq!(KeyName::from_label(rejected), None, "{rejected:?}");
        }
    }

    #[test]
    fn league_choice_resolves_against_the_listed_leagues() {
        let listed = ["Forbidden Rites", "HC Forbidden Rites", "Standard"].map(String::from);
        let named = |name: &str| LeagueChoice::Named(name.to_owned());
        assert_eq!(LeagueChoice::Auto.resolve(&listed), Some("Forbidden Rites"));
        assert_eq!(named("Standard").resolve(&listed), Some("Standard"));
        assert_eq!(
            named("Dawn of the Hunt").resolve(&listed),
            Some("Forbidden Rites"),
            "an ended league gives way to the current one"
        );
        assert_eq!(named("Standard").resolve(&[]), Some("Standard"));
        assert_eq!(LeagueChoice::Auto.resolve(&[]), None);
        let custom = LeagueChoice::Custom("My League (PL12345)".to_owned());
        assert_eq!(
            custom.resolve(&listed),
            Some("My League (PL12345)"),
            "a private league is never listed, and searched anyway"
        );
        assert_eq!(custom.resolve(&[]), Some("My League (PL12345)"));
    }
}
