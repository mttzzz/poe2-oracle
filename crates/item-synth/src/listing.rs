//! The trade site's item JSON: `result[].item` of `GET /api/trade2/fetch/{ids}`, in the language
//! of the site it was fetched from. Every field the site sends has a place here, so an item with
//! one this crate has never seen is refused (`deny_unknown_fields`) rather than written without
//! it: the game may print it, and a text missing a line is no copy of the item.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Item {
    // What the copy never shows: where the item is, its art and size, the seller's note (a copy of
    // an item in the player's own inventory has none), the site's own sums.
    pub realm: Option<String>,
    pub verified: Option<bool>,
    pub w: u32,
    pub h: u32,
    pub icon: String,
    pub league: Option<String>,
    pub id: Option<String>,
    pub frame_type_id: Option<String>,
    #[serde(default, deserialize_with = "object_or_empty")]
    pub extended: Extended,
    pub note: Option<String>,
    pub icon_tier_text: Option<String>,
    pub gem_background: Option<String>,
    pub gem_skill: Option<String>,

    pub name: String,
    pub type_line: String,
    pub base_type: String,
    /// `Normal`, `Magic`, `Rare` or `Unique`; absent for gems and currency, which `frame_type` names.
    pub rarity: Option<String>,
    /// GGG's frame: 0-3 the rarities, 4 a gem, 5 currency, 6 a divination card, 7 a quest item, 13
    /// and 14 a Runeforged rare or unique (`RunicRare`, `RunicUnique`). See [`Item::frame`].
    pub frame_type: u32,
    pub ilvl: u32,
    pub identified: bool,
    pub unidentified_tier: Option<u32>,
    #[serde(default)]
    pub corrupted: bool,
    #[serde(default)]
    pub double_corrupted: bool,
    /// The game's Mirrored.
    #[serde(default)]
    pub duplicated: bool,
    #[serde(default)]
    pub sanctified: bool,
    #[serde(default)]
    pub fractured: bool,
    #[serde(default)]
    pub desecrated: bool,
    /// A unique Vaal Cultivation changed: its changed mods carry `flags.mutated`.
    #[serde(default)]
    pub mutated: bool,
    pub stack_size: Option<u32>,
    pub max_stack_size: Option<u32>,
    /// A gem: whether it is a support gem.
    pub support: Option<bool>,

    #[serde(default)]
    pub properties: Vec<Property>,
    #[serde(default)]
    pub requirements: Vec<Property>,
    #[serde(default)]
    pub weapon_requirements: Vec<Property>,
    #[serde(default)]
    pub support_gem_requirements: Vec<Property>,
    #[serde(default)]
    pub granted_skills: Vec<Property>,
    #[serde(default)]
    pub sockets: Vec<Socket>,
    /// A Time-Lost diamond's or a Megalomaniac's passives, which the copy shows in a way not
    /// known yet: an item with any is refused.
    #[serde(default)]
    pub notable_properties: Vec<Property>,
    /// A skill gem's support sockets, a letter each.
    #[serde(default)]
    pub gem_sockets: Vec<String>,
    /// What fills the sockets: whole items, of which the copy shows only what `rune_mods` says.
    #[serde(default)]
    pub socketed_items: Vec<Value>,

    #[serde(default)]
    pub enchant_mods: Vec<ModLine>,
    #[serde(default)]
    pub rune_mods: Vec<ModLine>,
    /// A rune's or a soul core's effects as a Bonded socketed one gives them: lines of text.
    #[serde(default)]
    pub bonded_mods: Vec<ModLine>,
    #[serde(default)]
    pub implicit_mods: Vec<ModLine>,
    /// Fractured, desecrated, crafted, a relic's and a mutated unique's mods come here too, each
    /// line telling what it is by its `hash` and `flags`.
    #[serde(default)]
    pub explicit_mods: Vec<ModLine>,

    #[serde(default)]
    pub gem_tabs: Vec<GemTab>,
    #[serde(default)]
    pub flavour_text: Vec<String>,
    pub descr_text: Option<String>,
    pub sec_descr_text: Option<String>,
}

/// `extended`: the site's own sums (`dps`, `es`, ...), which the copy doesn't show, and `hashes`,
/// which tell which of the item's mods stand behind each mod line. The site sends an empty array
/// for an item it has nothing to say about.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Extended {
    #[serde(default)]
    pub hashes: Hashes,
    #[serde(flatten)]
    pub sums: BTreeMap<String, Value>,
}

/// Per mod kind (`explicit`, `fractured`, `desecrated`, `crafted`, `implicit`, `enchant`, `rune`,
/// `sanctum`, `skill`): each line's trade stat id and the indexes of the item's mods of that kind
/// behind it, in the order the item holds them -- the order the game's copy lists them in.
pub type Hashes = BTreeMap<String, Vec<(String, Option<Vec<usize>>)>>;

fn object_or_empty<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Extended, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::Array(values) if values.is_empty() => Ok(Extended::default()),
        other => serde_json::from_value(other).map_err(serde::de::Error::custom),
    }
}

/// What an item's `Rarity:` line says it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    Normal,
    Magic,
    Rare,
    Unique,
    Gem,
    Currency,
}

impl Item {
    /// The item's rarity by `rarity`, which a Runeforged item's own frame (13, 14) leaves to it;
    /// a gem's and currency's by their frame. Anything else -- a divination card, a quest item --
    /// isn't written yet.
    pub fn frame(&self) -> anyhow::Result<Frame> {
        Ok(match (self.frame_type, self.rarity.as_deref()) {
            (4, _) => Frame::Gem,
            (5, _) => Frame::Currency,
            (0 | 13 | 14, Some("Normal")) | (0, None) => Frame::Normal,
            (1 | 13 | 14, Some("Magic")) | (1, None) => Frame::Magic,
            (2 | 13 | 14, Some("Rare")) | (2, None) => Frame::Rare,
            (3 | 13 | 14, Some("Unique")) | (3, None) => Frame::Unique,
            (frame, rarity) => anyhow::bail!(
                "frameType {frame} ({:?}) with rarity {rarity:?} is not written yet",
                self.frame_type_id
            ),
        })
    }
}

/// One line of the item's properties, requirements or granted skills: `{"name": "[Evasion|Evasion
/// Rating]", "values": [["158", 1]], "displayMode": 0, "type": 17}`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Property {
    pub name: String,
    #[serde(default)]
    pub values: Vec<(String, u32)>,
    pub display_mode: u32,
    #[serde(rename = "type")]
    pub kind: Option<u32>,
    pub progress: Option<f64>,
    pub suffix: Option<String>,
    pub icon: Option<String>,
    pub domain: Option<String>,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Socket {
    pub group: u32,
    #[serde(rename = "type")]
    pub kind: String,
}

/// One mod line: plain text (a rune's or a currency's effect), or the line the site describes --
/// its text, trade stat id and the mods behind it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ModLine {
    Text(String),
    Described(DescribedLine),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DescribedLine {
    pub description: String,
    pub domain: Option<String>,
    /// `stat.explicit.stat_3299347043`: the kind before the id says whether the line is
    /// explicit, fractured, desecrated, crafted, ...
    pub hash: Option<String>,
    #[serde(default)]
    pub mods: Vec<Affix>,
    #[serde(default)]
    pub flags: BTreeMap<String, bool>,
}

impl ModLine {
    pub fn description(&self) -> &str {
        match self {
            ModLine::Text(text) => text,
            ModLine::Described(line) => &line.description,
        }
    }
}

/// One of the item's mods behind a line.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Affix {
    pub name: Option<String>,
    /// `P4` (a prefix of tier 4), `S0` (a suffix of no tier, an essence's or a crafted one).
    pub tier: Option<String>,
    pub level: Option<u32>,
    #[serde(default)]
    pub magnitudes: Vec<Magnitude>,
}

/// The range a mod rolls one number of its line in, as the site writes its bounds: `"23.1"`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Magnitude {
    pub hash: Option<String>,
    #[serde(deserialize_with = "number_text")]
    pub min: String,
    #[serde(deserialize_with = "number_text")]
    pub max: String,
}

fn number_text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::String(text) => Ok(text),
        Value::Number(number) => Ok(number.to_string()),
        other => Err(serde::de::Error::custom(format!("a bound {other}"))),
    }
}

/// A gem's tab of its skill: what the game lists under the gem's description.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GemTab {
    pub name: String,
    #[serde(default)]
    pub pages: Vec<GemPage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GemPage {
    pub skill_name: Option<String>,
    #[serde(default)]
    pub properties: Vec<Property>,
    #[serde(default)]
    pub stats: Vec<String>,
}
