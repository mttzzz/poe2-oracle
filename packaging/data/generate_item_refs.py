#!/usr/bin/env python3
"""Builds crates/poe2-oracle/assets/data/item-refs.tsv: every game item by kind, English
reference name, Russian name and art, from Exiled Exchange 2's generated item database
(renderer/public/data/{en,ru}/items.ndjson, MIT, https://github.com/Kvan7/Exiled-Exchange-2), and
for a base Craft of Exile crafts, the game's own ids of the bases so named and of their implicits'
stats: from the game's base item and mod tables as RePoE exports them (base_items.json and
mods.json, https://repoe-fork.github.io/poe2/; RePoE is MIT, the data belongs to Grinding Gear
Games), a line keyed by the trade stats EE2's stat database names (en/stats.ndjson).

The two EE2 item files list the same items in the same order, one per line, so a row's English and
Russian names are the same item. Rows without a reference name are skipped, repeats of an
item already written (a gem listed once per variant) are dropped, and art the database doesn't
have (`%NOT_FOUND%`) is left empty. Art URLs are stored without the CDN prefix every one of them
shares; `crate::item_refs` puts it back. The uniques in `unique-art.tsv` take their art from it
(the trade site's, where the database has none, stale art or their base's), and those the
database doesn't list at all are added after its rows. A unique drawn differently on its bases
gets each art, space-joined, the one the panel shows first.

The fifth field lists, space-joined, every released base of the row's English name in the item
classes Craft of Exile's item groups cover (`CRAFTED_CLASSES`; bases for sale nowhere and `[DNT]`
placeholders left out): its metadata id, then for each implicit `;` and the trade stats of its
lines -- comma-joined, a line two stats print alike giving both joined by `|`, `?` for a line EE2
doesn't print -- and, when the item's text gives each of the implicit's stats its roll, `=` and the
line printing each stat in the game's order of them (line indexes, a line of two numbers twice:
the order mod-tiers.tsv's `generate_mod_tiers.py` explains). Bases sharing a name (`Two-Stone
Ring`, one per pair of resistances) are told apart by their implicits.

Usage (from the repo root, inside the lane):
    python3 packaging/data/generate_item_refs.py <EE2 renderer/public/data dir> <RePoE data dir>
"""

import sys
from collections import defaultdict
from pathlib import Path

from generate_mod_tiers import ROLL, line_form, load_json, load_ndjson, printed_forms, shown

ICON_PREFIX = "https://web.poecdn.com/gen/image/"
KINDS = {"GEM": "gem", "ITEM": "item", "UNIQUE": "unique"}
OUT = Path(__file__).resolve().parents[2] / "crates/poe2-oracle/assets/data/item-refs.tsv"
# Uniques whose art the database lacks or gets wrong, and the few it doesn't list: their names and
# art as the trade site shows them (see the file's own comment).
UNIQUE_ART = Path(__file__).with_name("unique-art.tsv")
# RePoE's item classes (`base_items.json`'s `item_class`) of Craft of Exile's item groups: body
# armours, boots, charms, flasks, gloves, helmets, jewels, jewellery, offhands, one- and two-handed
# weapons, relics, tablets and waystones (its strongboxes are no items).
CRAFTED_CLASSES = {
    "Body Armour",
    "Boots",
    "UtilityFlask",
    "LifeFlask",
    "ManaFlask",
    "Gloves",
    "Helmet",
    "Jewel",
    "Amulet",
    "Ring",
    "Belt",
    "Shield",
    "Buckler",
    "Focus",
    "Quiver",
    "Claw",
    "Dagger",
    "Wand",
    "One Hand Sword",
    "One Hand Axe",
    "One Hand Mace",
    "Sceptre",
    "Spear",
    "Flail",
    "Bow",
    "Staff",
    "Two Hand Sword",
    "Two Hand Axe",
    "Two Hand Mace",
    "Warstaff",
    "Crossbow",
    "Talisman",
    "Relic",
    "TowerAugmentation",
    "Map",
}


def implicit_field(mod, forms):
    """An implicit's lines by their trade stats, then `=` and the line printing each of its stats
    when every line prints a number and they number its stats."""
    lines = (mod["text"] or "").split("\n")
    hashes = [forms.get(line_form(line)) for line in lines]
    counts = [sum(1 for _ in ROLL.finditer(shown(line))) for line in lines]
    field = ",".join("|".join(h) if h else "?" for h in hashes)
    if all(hashes) and all(counts) and sum(counts) == len(mod["stats"]):
        field += "=" + ",".join(str(line) for line, count in enumerate(counts) for _ in range(count))
    return field


def crafted_bases(ee2_dir, repoe_dir):
    """Each English base name's bases, as the fifth field writes them."""
    forms, _ = printed_forms(load_ndjson(Path(ee2_dir) / "en" / "stats.ndjson"), ("implicit", "explicit"))
    mods = load_json(Path(repoe_dir) / "mods.json")
    bases = defaultdict(list)
    for base_id, base in load_json(Path(repoe_dir) / "base_items.json").items():
        if (
            base["item_class"] not in CRAFTED_CLASSES
            or base["release_state"] != "released"
            or "not_for_sale" in base["tags"]
            or base["name"].startswith("[DNT]")
        ):
            continue
        implicits = (implicit_field(mods[mod_id], forms) for mod_id in base["implicits"])
        bases[base["name"]].append(";".join([base_id, *implicits]))
    return {name: " ".join(sorted(entries)) for name, entries in bases.items()}


def unique_art():
    """`unique-art.tsv`: each unique's English name -> (Russian name, its arts space-joined)."""
    art = {}
    for line in UNIQUE_ART.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        en, ru, icon = line.split("\t")
        if " " in icon:
            sys.exit(f"{UNIQUE_ART}: {en}'s art holds a space")
        russian, icons = art.get(en, (ru, ""))
        if russian != ru:
            sys.exit(f"{UNIQUE_ART}: {en} is {russian} and {ru}")
        art[en] = (ru, f"{icons} {icon}".strip())
    return art


def main(ee2_dir, repoe_dir):
    en = load_ndjson(Path(ee2_dir) / "en" / "items.ndjson")
    ru = load_ndjson(Path(ee2_dir) / "ru" / "items.ndjson")
    if len(en) != len(ru):
        sys.exit(f"{ee2_dir}: en has {len(en)} items, ru {len(ru)}: not the same database")
    bases = crafted_bases(ee2_dir, repoe_dir)
    art = unique_art()
    rows, seen, crafted = [], set(), 0
    for e, r in zip(en, ru):
        if e.get("refName") != r.get("refName") or e.get("namespace") != r.get("namespace"):
            sys.exit(f"the files disagree at {e.get('refName')!r} / {r.get('refName')!r}")
        kind = KINDS.get(e.get("namespace"))
        ref_name, ru_name = e.get("refName"), r.get("name")
        if not kind or not ref_name or not ru_name:
            continue
        key = (kind, ref_name, ru_name)
        if key in seen:
            continue
        seen.add(key)
        icon = e.get("icon") or ""
        if icon == "%NOT_FOUND%":
            icon = ""
        elif icon.startswith(ICON_PREFIX):
            icon = icon[len(ICON_PREFIX):]
        if kind == "unique" and ref_name in art:
            icon = art[ref_name][1]
        base_ids = bases.get(ref_name, "") if kind == "item" else ""
        crafted += bool(base_ids)
        rows.append((kind, ref_name, ru_name, icon, base_ids))
    listed = {row[1] for row in rows if row[0] == "unique"}
    rows += [
        ("unique", name, russian, icon, "")
        for name, (russian, icon) in art.items()
        if name not in listed
    ]
    for fields in rows:
        if any("\t" in field or "\n" in field for field in fields):
            sys.exit(f"a field of {fields[1]!r} holds a tab or newline")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    lines = ["\t".join(fields) for fields in rows]
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(rows)} items, {crafted} of them bases Craft of Exile crafts -> {OUT}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
