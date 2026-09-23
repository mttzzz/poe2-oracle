#!/usr/bin/env python3
"""Builds crates/trade-client/data/cx-items.tsv: the trade site's id of every item GGG's public
Currency Exchange history names (https://web.poecdn.com/api/currency-exchange/poe2/<hour>), from
the game's base item table as RePoE exports it (base_items.json, https://repoe-fork.github.io/poe2/;
RePoE is MIT, the data belongs to Grinding Gear Games) and the trade site's list of exchange items
(`GET https://www.pathofexile.com/api/trade2/data/static`, the English site).

The exchange history names an item by its base item's metadata id
(`Metadata/Items/Currency/CurrencyModValues`); the trade site, its listings and the app's market
by the static list's id (`divine`). The two meet at the item's English name: a base's `name` is
the static entry's `text`. On 2026-09-23 that gave every static entry an id, and it agreed with all
734 metadata-to-id pairs poe2scout lists. Quest copies of an item (item class `QuestItem`) share its
name but never trade and are left out; the other bases sharing a name, such as a legacy pinnacle
key beside the current one, all get the entry's id.

A row: the metadata id, the trade id and the static list's group id (`Currency`, `Ritual`, ...),
tab-separated, sorted by metadata id.

Usage (from the repo root, inside the lane):
    python3 packaging/data/generate_cx_ids.py <base_items.json> <static.json> [<exchange hour.json>]

Given a saved exchange hour, it also lists the metadata ids the hour trades that got no trade id.
"""

import json
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parents[2] / "crates/trade-client/data/cx-items.tsv"


def load_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def main(base_items_path, static_path, hour_path=None):
    entries = {}
    for group in load_json(static_path)["result"]:
        for entry in group["entries"]:
            # The site's list separators, not items.
            if entry["id"] == "sep":
                continue
            if entry["text"] in entries:
                sys.exit(f"two static entries are named {entry['text']!r}")
            entries[entry["text"]] = (entry["id"], group["id"])

    rows = []
    for metadata_id, base in load_json(base_items_path).items():
        entry = entries.get(base["name"])
        if entry and base["item_class"] != "QuestItem":
            rows.append((metadata_id, *entry))
    rows.sort()
    fields = [field for row in rows for field in row]
    if any("\t" in field or "\n" in field for field in fields):
        sys.exit("a field holds a tab or newline")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("".join("\t".join(row) + "\n" for row in rows), encoding="utf-8", newline="\n")
    named = {trade_id for _, trade_id, _ in rows}
    print(f"{len(rows)} metadata ids, {len(named)} of {len(entries)} trade ids -> {OUT}")
    unnamed = sorted(text for text, (trade_id, _) in entries.items() if trade_id not in named)
    if unnamed:
        print(f"static entries no base is named: {', '.join(unnamed)}")

    if hour_path:
        mapped = {metadata_id for metadata_id, _, _ in rows}
        traded = {
            metadata_id
            for market in load_json(hour_path)["markets"]
            for metadata_id in market["market_pair"]
        }
        print(f"the hour trades {len(traded)} items; without a trade id:")
        for metadata_id in sorted(traded - mapped):
            print(f"  {metadata_id}")


if __name__ == "__main__":
    if len(sys.argv) not in (3, 4):
        sys.exit(__doc__)
    main(*sys.argv[1:])
