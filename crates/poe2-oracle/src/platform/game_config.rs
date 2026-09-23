//! What this app needs from the PoE2 client's own settings file: the "Advanced Mod Description"
//! key (held with Ctrl+C to copy an item with its mod tiers, see `synth_input`), the display mode
//! (an overlay can't show over exclusive fullscreen) and the client language.
//!
//! The file is `<Documents>\My Games\Path of Exile 2\poe2_production_Config.ini`, a flat INI of
//! `[SECTION]` headers and `key=value` lines -- no INI crate, as `exiled-exchange-2`'s own
//! `main/src/host-files/GameConfig.ts` reads the same file. The Documents folder is the known
//! folder, not `%USERPROFILE%\Documents`: OneDrive and a moved Documents folder put it elsewhere,
//! and the game follows the known folder.
//!
//! Values read live from the test machine's file (2026-09-22): `[ACTION_KEYS]`
//! `show_advanced_item_descriptions=18` -- a bare Win32 virtual-key code, as every other value
//! in that section (`toggle_skill_bar=17` is `VK_CONTROL`, `open_inventory_panel=73` is `VK_I`);
//! 18 is `VK_MENU`, Alt. `[DISPLAY]` `fullscreen=false` with `borderless_windowed_fullscreen=true`
//! for the game's "Windowed Fullscreen". `[LANGUAGE]` `language=ru`.
//!
//! Plain file parsing, no Windows API: it builds and is tested on every target.

use std::path::PathBuf;

/// Alt: the game's default advanced-description key, and EE2's `_showModsKey ?? "Alt"` fallback.
pub const DEFAULT_ADVANCED_MOD_DESC_KEY: u16 = 18;

/// How the game fills the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayMode {
    /// Exclusive fullscreen: the game owns the display, and no overlay shows over it.
    Fullscreen,
    WindowedFullscreen,
    Windowed,
}

/// What the game's settings file says, as far as this app cares.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GameConfig {
    /// Win32 virtual-key code of "Show advanced item descriptions".
    pub advanced_mod_desc_key: u16,
    /// `None` when the file doesn't say.
    pub display_mode: Option<DisplayMode>,
    /// The client's language code, `"ru"` or `"en"` and so on; `None` when the file doesn't say.
    pub language: Option<String>,
}

impl Default for GameConfig {
    fn default() -> GameConfig {
        GameConfig {
            advanced_mod_desc_key: DEFAULT_ADVANCED_MOD_DESC_KEY,
            display_mode: None,
            language: None,
        }
    }
}

/// The game's settings, or defaults when the file is missing (the game never ran on this
/// profile) or unreadable. Never fails: every caller treats "couldn't read it" and "it says the
/// defaults" alike.
pub fn read() -> GameConfig {
    config_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map_or_else(GameConfig::default, |content| parse(&content))
}

/// Where the game keeps the file; `None` without a Documents folder.
pub fn config_path() -> Option<PathBuf> {
    let documents = directories::UserDirs::new()?.document_dir()?.to_path_buf();
    Some(
        documents
            .join("My Games")
            .join("Path of Exile 2")
            .join("poe2_production_Config.ini"),
    )
}

fn parse(content: &str) -> GameConfig {
    // A value may carry a trailing " <modifier>" (`use_bound_skill9=81 2`): the code is the first
    // token.
    let advanced_mod_desc_key = value(content, "ACTION_KEYS", "show_advanced_item_descriptions")
        .and_then(|value| value.split_whitespace().next()?.parse().ok())
        .unwrap_or(DEFAULT_ADVANCED_MOD_DESC_KEY);
    let display_mode = value(content, "DISPLAY", "fullscreen").map(|fullscreen| {
        if fullscreen == "true" {
            DisplayMode::Fullscreen
        } else if value(content, "DISPLAY", "borderless_windowed_fullscreen") == Some("true") {
            DisplayMode::WindowedFullscreen
        } else {
            DisplayMode::Windowed
        }
    });
    let language = value(content, "LANGUAGE", "language")
        .filter(|language| !language.is_empty())
        .map(str::to_owned);
    GameConfig {
        advanced_mod_desc_key,
        display_mode,
        language,
    }
}

/// `key`'s value in `[section]`, trimmed.
fn value<'a>(content: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut in_section = false;
    for line in content.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            in_section = name == section;
            continue;
        }
        if in_section
            && let Some((name, value)) = line.split_once('=')
            && name.trim() == key
        {
            return Some(value.trim());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test machine's file, cut down to the lines that matter and one neighbour each.
    const LIVE: &str = "[LOGIN]\r\naccount_name=\r\n[DISPLAY]\r\nresolution_width=3840\r\n\
        fullscreen=false\r\nmaximize_window=false\r\nborderless_windowed_fullscreen=true\r\n\
        [ACTION_KEYS]\r\ntoggle_skill_bar=17\r\nshow_advanced_item_descriptions=18\r\n\
        use_bound_skill9=81 2\r\n[LANGUAGE]\r\nlanguage=ru\r\nchat_language=ru\r\n";

    #[test]
    fn reads_the_live_file() {
        assert_eq!(
            parse(LIVE),
            GameConfig {
                advanced_mod_desc_key: 18,
                display_mode: Some(DisplayMode::WindowedFullscreen),
                language: Some("ru".to_owned()),
            }
        );
    }

    #[test]
    fn fullscreen_wins_over_the_borderless_flag() {
        let exclusive = LIVE.replace("fullscreen=false", "fullscreen=true");
        assert_eq!(
            parse(&exclusive).display_mode,
            Some(DisplayMode::Fullscreen)
        );
        let windowed = LIVE.replace(
            "borderless_windowed_fullscreen=true",
            "borderless_windowed_fullscreen=false",
        );
        assert_eq!(parse(&windowed).display_mode, Some(DisplayMode::Windowed));
    }

    #[test]
    fn a_key_elsewhere_or_missing_falls_back_to_alt() {
        // The same key name under another section is not the setting.
        let moved =
            "[UI]\nshow_advanced_item_descriptions=16\n[ACTION_KEYS]\ntoggle_skill_bar=17\n";
        assert_eq!(
            parse(moved).advanced_mod_desc_key,
            DEFAULT_ADVANCED_MOD_DESC_KEY
        );
        assert_eq!(
            parse("").advanced_mod_desc_key,
            DEFAULT_ADVANCED_MOD_DESC_KEY
        );
        let shift = LIVE.replace(
            "show_advanced_item_descriptions=18",
            "show_advanced_item_descriptions=16 1",
        );
        assert_eq!(parse(&shift).advanced_mod_desc_key, 16);
    }

    #[test]
    fn a_file_without_the_sections_says_nothing() {
        let config = parse("[GENERAL]\nfoo=bar\n");
        assert_eq!((config.display_mode, config.language), (None, None));
    }
}
