#!/usr/bin/env python3
"""Builds crates/stat-filters/data/mod-tiers.tsv: how many tiers each explicit mod family has on
each kind of item and the item level each tier needs, from the game's own mod and base item
tables as RePoE exports them (mods.json and base_items.json, https://repoe-fork.github.io/poe2/;
RePoE is MIT, the data belongs to Grinding Gear Games), keyed by the trade stats of the family's
printed lines, which Exiled Exchange 2's stat database names (renderer/public/data/en/stats.ndjson,
MIT, https://github.com/Kvan7/Exiled-Exchange-2).

The advanced item copy prints a mod's tier, T1 the best, but not how many tiers its family has on
that item, nor which of them the item's level let it roll: `stat_filters` needs both to tell a T3
of thirteen life tiers from a T3 of four.

A family is the game's mod type in one affix slot: its mods are the tiers the client numbers, T1
the one needing the highest item level. (The `groups` exclusivity group is coarser: `FireResistance`
also holds the resistance-and-maximum hybrid, and counting tiers by it contradicts the tiers live
items print.) A mod rolls on a base when their domains match and the first of its spawn weights
whose tag the base has is above zero; essence-only mods never roll. A family's tiers on a trade
category merge those of every released base the category files: each required level counts as
often as it repeats on any one base. Desecrated (Abyssal) mods are left out, each the only tier of
its family, as are relics, tablets and waystones, whose mods the search picks by other rules.

A row: the family's key -- the trade stat hashes of its printed lines, sorted and joined with `+`,
as the parser resolves a modifier's lines -- its slot (`p` or `s`), the trade category, and the
tiers' required levels, T1 first, comma-joined. A line's trade stats are those of the EE2 stat
printing it that way (EE2 names a stat's game id only for some stats, and never the local twin of
a stat printed alike, `local_energy_shield` for `# to maximum Energy Shield`). A line two trade
stats print alike (`# to Armour` and its `(Local)` twin, one of which the parser picks by the
item's kind) gives a row per pick.

Usage (from the repo root, inside the lane):
    python3 packaging/data/generate_mod_tiers.py <RePoE data dir> <EE2 renderer/public/data dir>
"""

import itertools
import json
import re
import sys
from collections import Counter, defaultdict
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
SLOTS = {"prefix": "p", "suffix": "s"}
# `[Tag|Shown text]` or `[Shown]` keyword markup, and a number or `(min-max)` range.
MARKUP = re.compile(r"\[([^\]|]+)(?:\|([^\]]+))?\]")
NUMBER = re.compile(r"\(-?\d+(?:\.\d+)?--?\d+(?:\.\d+)?\)|-?\d+(?:\.\d+)?")


def load_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def load_ndjson(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def explicit_hashes(stat):
    """An EE2 stat's explicit trade ids without their `explicit.` prefix."""
    ids = ((stat.get("trade") or {}).get("ids") or {}).get("explicit", [])
    return [trade_id.split(".", 1)[1] for trade_id in ids]


def printed_forms(stats):
    """EE2's printed forms, numbers as `#` without a `+`, to their explicit trade hashes."""
    forms = defaultdict(list)
    for stat in stats:
        for matcher in stat.get("matchers", []):
            form = matcher["string"].strip().replace("+#", "#")
            forms[form] += [h for h in explicit_hashes(stat) if h not in forms[form]]
    return forms


def line_form(line):
    """A line of a mod's text as EE2 writes its forms."""
    line = MARKUP.sub(lambda m: m.group(2) or m.group(1), line)
    return NUMBER.sub("#", line).replace("+#", "#").strip()


def weight(mod, tags):
    """The spawn weight of the first of `mod`'s tags the base has (the game's rule)."""
    return next((w["weight"] for w in mod["spawn_weights"] if w["tag"] in tags), 0)


def report_coverage(rows, ee2, affixes, domains):
    """How many of EE2's explicit trade stats got a family, and the stats a rare may carry that
    didn't: `stat_filters` judges a mod of those by its plain tier."""
    keyed = {h for _, _, key, _ in rows for h in key.split("+")}
    game_ids, refs = defaultdict(set), {}
    for stat in ee2:
        for trade_hash in explicit_hashes(stat):
            game_ids[trade_hash].add(stat.get("id"))
            refs.setdefault(trade_hash, stat["ref"])
    print(f"EE2 explicit trade stats with a family: {len(keyed & game_ids.keys())} of {len(game_ids)}")

    carried = defaultdict(set)
    for domain, domain_mods in affixes.items():
        kind = "affix" if domain in domains else "desecrated" if domain == "desecrated" else None
        for mod in domain_mods if kind else []:
            for stat in mod["stats"]:
                carried[stat["id"]].add(kind)
    unkeyed = defaultdict(list)
    for trade_hash, ids in game_ids.items():
        kinds = set().union(*(carried[i] for i in ids))
        if trade_hash not in keyed and kinds:
            unkeyed["affix" if "affix" in kinds else "desecrated"].append(trade_hash)
    print(
        f"without one: {len(unkeyed['desecrated'])} stats only single-tier desecrated mods carry, "
        f"and these {len(unkeyed['affix'])} of affixes that never roll by themselves "
        "(essence, alloy, crafted, influenced):"
    )
    for trade_hash in sorted(unkeyed["affix"], key=refs.get):
        print(f"  {trade_hash}\t{refs[trade_hash]!r}")


def main(repoe_dir, ee2_dir):
    mods = load_json(Path(repoe_dir) / "mods.json")
    bases = [
        (CATEGORIES[base["item_class"]], base)
        for base in load_json(Path(repoe_dir) / "base_items.json").values()
        if base["item_class"] in CATEGORIES and base["release_state"] == "released"
    ]
    ee2 = load_ndjson(Path(ee2_dir) / "en" / "stats.ndjson")
    forms = printed_forms(ee2)

    affixes = defaultdict(list)
    for mod in mods.values():
        if mod["generation_type"] in SLOTS and not mod["is_essence_only"]:
            affixes[mod["domain"]].append(mod)

    # (category, mod type, slot) -> required level -> the most tiers needing it on one base.
    tiers = defaultdict(Counter)
    texts = {}
    for category, base in bases:
        tags = set(base["tags"])
        on_base = defaultdict(Counter)
        for mod in affixes[base["domain"]]:
            if weight(mod, tags) > 0:
                family = (mod["type"], mod["generation_type"])
                on_base[(category, *family)][mod["required_level"]] += 1
                texts.setdefault(family, mod["text"])
        for family, levels in on_base.items():
            tiers[family] |= levels

    rows, families, unprinted = [], 0, set()
    for (category, mod_type, slot), levels in tiers.items():
        lines = [forms.get(line_form(line)) for line in texts[(mod_type, slot)].split("\n")]
        if not all(lines):
            unprinted.add(mod_type)
            continue
        families += 1
        ordered = ",".join(map(str, sorted(levels.elements(), reverse=True)))
        keys = {"+".join(sorted(set(pick))) for pick in itertools.product(*lines)}
        rows += [(category, SLOTS[slot], key, ordered) for key in keys]
    rows.sort()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(
        "".join(f"{key}\t{slot}\t{category}\t{levels}\n" for category, slot, key, levels in rows),
        encoding="utf-8",
        newline="\n",
    )
    print(f"{families} families, {len(rows)} rows -> {OUT}")
    if unprinted:
        print(f"left out, a line EE2 doesn't print: {', '.join(sorted(unprinted))}")
    report_coverage(rows, ee2, affixes, {base["domain"] for _, base in bases})


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
