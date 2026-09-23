#!/usr/bin/env python3
"""Builds crates/stat-filters/data/mod-tiers.tsv: every tier of each explicit mod family on each
kind of item -- the item level it needs, its mod id, its tags, its roll ranges and the order of its
stats -- from the game's own mod and base item tables as RePoE exports them (mods.json and
base_items.json, https://repoe-fork.github.io/poe2/; RePoE is MIT, the data belongs to Grinding
Gear Games), keyed by the trade stats of the family's printed lines, which Exiled Exchange 2's stat
database names (renderer/public/data/en/stats.ndjson, MIT,
https://github.com/Kvan7/Exiled-Exchange-2).

The advanced item copy prints a mod's tier, T1 the best, but not how many tiers its family has on
that item, which of them the item's level let it roll, nor the rolls of the other tiers:
`stat_filters` needs all three to score a mod the way PoE Overlay II does and to show where its
roll sits. Nor does it print the mod's id: Craft of Exile's item import takes a mod by its id and
its rolls in the game's order of its stats.

A family is the game's mod type in one affix slot: its mods are the tiers the client numbers, T1
the one needing the highest item level. (The `groups` exclusivity group is coarser: `FireResistance`
also holds the resistance-and-maximum hybrid, and counting tiers by it contradicts the tiers live
items print.) A mod rolls on a base when their domains match and the first of its spawn weights
whose tag the base has is above zero; essence-only mods never roll. A family's tiers on a trade
category merge those of every released base the category files: each required level counts as
often as it repeats on any one base, as the mods of the base holding it most often. Desecrated
(Abyssal) mods form families of their own, in their own pool: the client numbers them apart from
the ordinary tiers of the same stats. Relics and tablets are left out: the search picks their mods
by other rules. So does it a waystone's, but Craft of Exile's link needs their ids: each waystone
tier gets families of its own, in a category the search never looks one up in,
`map.waystone:<tier>` (see `waystone_rows`).

A row is one tier: the family's key -- the trade stat hashes of its printed lines, sorted and joined
with `+`, as the parser resolves a modifier's lines -- its slot (`p` or `s`), the trade category,
the pool (`a` for the ordinary affixes, `d` for the desecrated ones), then the tier's required
level, its RePoE mod id, its tags, the roll range of each key stat and the order of its stats. A
family's rows are consecutive, T1 first. A line's trade stats are those of the EE2 stat printing it
that way (EE2 names a stat's game id only for some stats, and never the local twin of a stat
printed alike, `local_energy_shield` for `# to maximum Energy Shield`). A line two trade stats
print alike (`# to Armour` and its `(Local)` twin, one of which the parser picks by the item's
kind) gives a family per pick.

Tags are the mod's RePoE `implicit_tags` that PoE Overlay II's weight tables name -- its tag list
`Gem, Caster, Fire, Cold, Lightning, Chaos, Physical, Life, Defences, Elemental, Attack, Minion,
Aura, Mana, Speed, Critical, Damage, Resistance, Attribute, Ailment, Curse` (9398.bundle.js,
module 81020), each the id of the `Tag<Name>` client string it reads printed tags by --
comma-joined, capitalized as it names them. PoE Overlay II reads the tags the item prints, and
the live client prints a defence mod's own kind (`Energy Shield`, `Armour`, `Evasion`) without the
`defences` RePoE also gives it; the table keeps `Defences`, the tag those weights were written
for. A range is `min:max` as the item prints it, ascending, two or four numbers on a line taken at
their mean the way the parser rolls them (`Adds (5-8) to (12-15)` rolls 8.5 to 11.5), a line
without a number at the one its EE2 form stands for (`Loads an additional bolt`: 1:1), or `_` for
a flag; ranges are comma-joined in key order. The order names the key stat printing each of the
mod's stats, in the game's order, the one RePoE's text prints their numbers in: key indexes
comma-joined, a line of two numbers giving its stat twice (`Adds (1-4) to (53-76) Lightning
Damage`: 0,0), or `_` when the item's text can't give each stat its roll -- a line without a
number, a hidden stat, a number in the wording.

Usage (from the repo root, inside the lane):
    python3 packaging/data/generate_mod_tiers.py <RePoE data dir> <EE2 renderer/public/data dir>
"""

import itertools
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

OUT = Path(__file__).resolve().parents[2] / "crates/stat-filters/data/mod-tiers.tsv"

# RePoE's item class ids (`base_items.json`'s `item_class`) with the trade category
# `item_parser::categories` files each under.
CATEGORIES = {
    "Amulet": "accessory.amulet",
    "Ring": "accessory.ring",
    "Belt": "accessory.belt",
    "Claw": "weapon.claw",
    "Dagger": "weapon.dagger",
    "Wand": "weapon.wand",
    "One Hand Sword": "weapon.onesword",
    "One Hand Axe": "weapon.oneaxe",
    "One Hand Mace": "weapon.onemace",
    "Bow": "weapon.bow",
    "Staff": "weapon.staff",
    "Two Hand Sword": "weapon.twosword",
    "Two Hand Axe": "weapon.twoaxe",
    "Two Hand Mace": "weapon.twomace",
    "Sceptre": "weapon.sceptre",
    "Warstaff": "weapon.warstaff",
    "Spear": "weapon.spear",
    "Crossbow": "weapon.crossbow",
    "Flail": "weapon.flail",
    "Talisman": "weapon.talisman",
    "Quiver": "armour.quiver",
    "Gloves": "armour.gloves",
    "Boots": "armour.boots",
    "Body Armour": "armour.chest",
    "Helmet": "armour.helmet",
    "Shield": "armour.shield",
    "Buckler": "armour.buckler",
    "Focus": "armour.focus",
    "Jewel": "jewel",
    "LifeFlask": "flask.life",
    "ManaFlask": "flask.mana",
    "UtilityFlask": "flask.charm",
}
# Waystones' item class, their trade category and the tier a waystone's name holds.
WAYSTONE_CLASS = "Map"
WAYSTONE = "map.waystone"
WAYSTONE_NAME = re.compile(r"Waystone \(Tier (\d+)\)")
SLOTS = {"prefix": "p", "suffix": "s"}
DESECRATED = "desecrated"
# PoE Overlay II's tag list, by RePoE's lower-case tag id.
TAGS = {
    tag.lower(): tag
    for tag in (
        "Gem Caster Fire Cold Lightning Chaos Physical Life Defences Elemental Attack Minion Aura "
        "Mana Speed Critical Damage Resistance Attribute Ailment Curse"
    ).split()
}
# `[Tag|Shown text]` or `[Shown]` keyword markup, and a number or `(min-max)` range.
MARKUP = re.compile(r"\[([^\]|]+)(?:\|([^\]]+))?\]")
NUMBER = re.compile(r"\(-?\d+(?:\.\d+)?--?\d+(?:\.\d+)?\)|-?\d+(?:\.\d+)?")
# The same, read: an optional sign before a range, its bounds, or a plain number.
ROLL = re.compile(r"(-?)\((-?\d+(?:\.\d+)?)-(-?\d+(?:\.\d+)?)\)|(-?\d+(?:\.\d+)?)")


def load_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def load_ndjson(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def trade_hashes(stat, kinds=("explicit", DESECRATED)):
    """An EE2 stat's trade ids of `kinds` without their `explicit.`/... prefix, first seen first."""
    ids = (stat.get("trade") or {}).get("ids") or {}
    hashes = []
    for kind in kinds:
        for trade_id in ids.get(kind, []):
            trade_hash = trade_id.split(".", 1)[1]
            if trade_hash not in hashes:
                hashes.append(trade_hash)
    return hashes


def printed_forms(stats, kinds=("explicit", DESECRATED)):
    """EE2's printed forms, numbers as `#` without a `+`, to their trade hashes of `kinds` (an
    affix's explicit and desecrated ones), and the number a form without one stands for (`Loads an
    additional bolt`: 1)."""
    forms, implied = defaultdict(list), {}
    for stat in stats:
        for matcher in stat.get("matchers", []):
            form = matcher["string"].strip().replace("+#", "#")
            forms[form] += [h for h in trade_hashes(stat, kinds) if h not in forms[form]]
            if "value" in matcher and "#" not in form:
                implied.setdefault(form, float(matcher["value"]))
    return forms, implied


def shown(line):
    """A line of a mod's text as the item prints it, keyword markup reduced to its words."""
    return MARKUP.sub(lambda m: m.group(2) or m.group(1), line)


def line_form(line):
    """A line of a mod's text as EE2 writes its forms."""
    return NUMBER.sub("#", shown(line)).replace("+#", "#").strip()


def line_range(line, implied=None):
    """A line's roll range as the parser rolls the printed line: two or four numbers at their
    mean, anything else at the first, a line without a number at the one its form stands for
    (`implied`); `None` for a flag."""
    runs = []
    for m in ROLL.finditer(shown(line)):
        if m.group(4) is not None:
            value = float(m.group(4))
            runs.append((value, value))
            continue
        lo, hi = float(m.group(2)), float(m.group(3))
        if m.group(1):
            lo, hi = -lo, -hi
        runs.append((min(lo, hi), max(lo, hi)))
    if not runs:
        return None if implied is None else (implied, implied)
    rolled = runs if len(runs) in (2, 4) else runs[:1]
    return (
        sum(lo for lo, _ in rolled) / len(rolled),
        sum(hi for _, hi in rolled) / len(rolled),
    )


def stat_order(mod, text, pick, key):
    """Which of the `key` stats prints each of `mod`'s stats, in the game's own stat order:
    RePoE's text prints their numbers in that order, one per stat. Key indexes, comma-joined; a
    line printing two numbers (`Adds (1-4) to (53-76) Lightning Damage`) gives its stat twice.
    `_` when a line prints no number (a flag) or the text prints another count of numbers than
    the mod has stats (a hidden one, a number in the wording): the item's text can't give each
    stat its roll."""
    order = []
    for trade_hash, line in zip(pick, text):
        count = sum(1 for _ in ROLL.finditer(shown(line)))
        if not count:
            return "_"
        order += [str(key.index(trade_hash))] * count
    return ",".join(order) if len(order) == len(mod["stats"]) else "_"


def number(value):
    """`value` as short as it reads: `10`, `8.5`."""
    return str(int(value)) if value == int(value) else f"{value:.4f}".rstrip("0")


def weight(mod, tags):
    """The spawn weight of the first of `mod`'s tags the base has (the game's rule)."""
    return next((w["weight"] for w in mod["spawn_weights"] if w["tag"] in tags), 0)


def report_coverage(keyed, ee2, affixes, domains):
    """How many of EE2's explicit trade stats got a family, and the stats a rare may carry that
    didn't: `stat_filters` judges a mod of those by its plain tier."""
    game_ids, refs = defaultdict(set), {}
    for stat in ee2:
        for trade_hash in trade_hashes(stat, ("explicit",)):
            game_ids[trade_hash].add(stat.get("id"))
            refs.setdefault(trade_hash, stat["ref"])
    print(f"EE2 explicit trade stats with a family: {len(keyed & game_ids.keys())} of {len(game_ids)}")

    carried = defaultdict(set)
    for domain, domain_mods in affixes.items():
        if domain in domains:
            for _, mod in domain_mods:
                for stat in mod["stats"]:
                    carried[stat["id"]].add(domain)
    unkeyed = sorted(
        (refs[trade_hash], trade_hash)
        for trade_hash, ids in game_ids.items()
        if trade_hash not in keyed and set().union(*(carried[i] for i in ids))
    )
    print(
        f"without one: these {len(unkeyed)} of affixes that never roll by themselves "
        "(essence, alloy, crafted, influenced):"
    )
    for ref, trade_hash in unkeyed:
        print(f"  {trade_hash}\t{ref!r}")


def main(repoe_dir, ee2_dir):
    mods = load_json(Path(repoe_dir) / "mods.json")
    base_items = load_json(Path(repoe_dir) / "base_items.json")
    bases = [
        (CATEGORIES[base["item_class"]], base)
        for base in base_items.values()
        if base["item_class"] in CATEGORIES and base["release_state"] == "released"
    ]
    ee2 = load_ndjson(Path(ee2_dir) / "en" / "stats.ndjson")
    forms, implied = printed_forms(ee2)

    affixes = defaultdict(list)
    for mod_id, mod in mods.items():
        if mod["generation_type"] in SLOTS and not mod["is_essence_only"]:
            affixes[mod["domain"]].append((mod_id, mod))

    # (category, pool, mod type, slot) -> required level -> the mods needing it on the base that
    # holds most of them.
    tiers = defaultdict(dict)
    for category, base in bases:
        tags = set(base["tags"])
        on_base = defaultdict(lambda: defaultdict(list))
        for domain in (base["domain"], DESECRATED):
            pool = "d" if domain == DESECRATED else "a"
            for mod_id, mod in affixes[domain]:
                if weight(mod, tags) > 0:
                    family = (category, pool, mod["type"], mod["generation_type"])
                    on_base[family][mod["required_level"]].append(mod_id)
        for family, levels in on_base.items():
            merged = tiers[family]
            for level, mod_ids in levels.items():
                if len(mod_ids) > len(merged.get(level, ())):
                    merged[level] = sorted(mod_ids)

    rows, families, unprinted, owner = [], 0, set(), {}
    for (category, pool, mod_type, slot), levels in tiers.items():
        tier_mods = [
            mods[mod_id] | {"id": mod_id}
            for level in sorted(levels, reverse=True)
            for mod_id in levels[level]
        ]
        texts = [mod["text"].split("\n") for mod in tier_mods]
        # Every tier prints its lines as the same trade stats, some in a form of their own
        # (`Loads an additional bolt`, `Loads 2 additional bolts`).
        lines = [forms.get(line_form(line)) for line in texts[0]]
        if not all(lines) or any([forms.get(line_form(line)) for line in text] != lines for text in texts):
            unprinted.add(mod_type)
            continue
        families += 1
        for pick in itertools.product(*lines):
            key = "+".join(sorted(set(pick)))
            family = (category, SLOTS[slot], key, pool)
            if owner.setdefault(family, mod_type) != mod_type:
                sys.exit(f"{mod_type} and {owner[family]} print alike: {family}")
            for mod, text in zip(tier_mods, texts):
                ranges = {}
                for trade_hash, line in zip(pick, text):
                    ranges.setdefault(trade_hash, line_range(line, implied.get(line_form(line))))
                rows.append(
                    (
                        *family,
                        -mod["required_level"],
                        mod["id"],
                        ",".join(TAGS[tag] for tag in mod["implicit_tags"] if tag in TAGS),
                        ",".join(
                            "_" if ranges[h] is None else f"{number(ranges[h][0])}:{number(ranges[h][1])}"
                            for h in key.split("+")
                        ),
                        stat_order(mod, text, pick, key.split("+")),
                    )
                )
    rows += waystone_rows(base_items, affixes, forms, implied, owner)
    rows.sort()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(
        "".join(
            f"{key}\t{slot}\t{category}\t{pool}\t{-level}\t{mod_id}\t{tags}\t{ranges}\t{order}\n"
            for category, slot, key, pool, level, mod_id, tags, ranges, order in rows
        ),
        encoding="utf-8",
        newline="\n",
    )
    print(f"{families} families, {len(rows)} tiers -> {OUT}")
    if unprinted:
        print(f"left out, a line EE2 doesn't print: {', '.join(sorted(unprinted))}")
    keyed = {h for row in rows for h in row[2].split("+")}
    report_coverage(keyed, ee2, affixes, {base["domain"] for _, base in bases} | {DESECRATED})


def waystone_rows(base_items, affixes, forms, implied, owner):
    """Each waystone tier's mods, as rows of a category of its own, `map.waystone:<tier>`, which
    the search never looks a family up in: a waystone's mods roll by its tier rather than its item
    level, one mod of a family on each, and it prints only their lines EE2 prints -- the others
    (`20% more Waystones found in Area`) add to the waystone's own properties. A mod with a roll
    on such a line has a roll the item doesn't print, and gets no row."""
    rows = []
    for base in base_items.values():
        tier = WAYSTONE_NAME.fullmatch(base["name"])
        if base["item_class"] != WAYSTONE_CLASS or base["release_state"] != "released" or not tier:
            continue
        tags = set(base["tags"])
        for mod_id, mod in affixes[base["domain"]]:
            if weight(mod, tags) <= 0:
                continue
            text = mod["text"].split("\n")
            printed = [(line, forms.get(line_form(line))) for line in text]
            shown_lines = [(line, hashes) for line, hashes in printed if hashes]
            hidden_roll = any(
                m.group(2) is not None for line, hashes in printed if not hashes for m in ROLL.finditer(shown(line))
            )
            if not shown_lines or hidden_roll:
                continue
            for pick in itertools.product(*(hashes for _, hashes in shown_lines)):
                key = "+".join(sorted(set(pick)))
                family = (f"{WAYSTONE}:{tier.group(1)}", SLOTS[mod["generation_type"]], key, "a")
                if owner.setdefault(family, mod["type"]) != mod["type"]:
                    sys.exit(f"{mod['type']} and {owner[family]} print alike: {family}")
                ranges = {}
                for trade_hash, (line, _) in zip(pick, shown_lines):
                    ranges.setdefault(trade_hash, line_range(line, implied.get(line_form(line))))
                rows.append(
                    (
                        *family,
                        -mod["required_level"],
                        mod_id,
                        ",".join(TAGS[tag] for tag in mod["implicit_tags"] if tag in TAGS),
                        ",".join(
                            "_" if ranges[h] is None else f"{number(ranges[h][0])}:{number(ranges[h][1])}"
                            for h in key.split("+")
                        ),
                        "_" if len(shown_lines) < len(text) else stat_order(mod, text, pick, key.split("+")),
                    )
                )
    return rows


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
