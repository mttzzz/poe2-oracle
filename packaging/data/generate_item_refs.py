#!/usr/bin/env python3
"""Builds crates/poe2-oracle/assets/data/item-refs.tsv: every game item by kind, English
reference name, Russian name and art, from Exiled Exchange 2's generated item database
(renderer/public/data/{en,ru}/items.ndjson, MIT, https://github.com/Kvan7/Exiled-Exchange-2).

The two EE2 files list the same items in the same order, one per line, so a row's English and
Russian names are the same item. Rows without a reference name are skipped, repeats of an
item already written (a gem listed once per variant) are dropped, and art the database doesn't
have (`%NOT_FOUND%`) is left empty. Art URLs are stored without the CDN prefix every one of them
shares; `crate::item_refs` puts it back.

Usage (from the repo root, inside the lane):
    python3 packaging/data/generate_item_refs.py <items-en.ndjson> <items-ru.ndjson>
"""

import json
import sys
from pathlib import Path

ICON_PREFIX = "https://web.poecdn.com/gen/image/"
KINDS = {"GEM": "gem", "ITEM": "item", "UNIQUE": "unique"}
OUT = Path(__file__).resolve().parents[2] / "crates/poe2-oracle/assets/data/item-refs.tsv"


def load(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def main(en_path, ru_path):
    en, ru = load(en_path), load(ru_path)
    if len(en) != len(ru):
        sys.exit(f"{en_path} has {len(en)} items, {ru_path} {len(ru)}: not the same database")
    rows, seen = [], set()
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
        fields = (kind, ref_name, ru_name, icon)
        if any("\t" in field or "\n" in field for field in fields):
            sys.exit(f"a field of {ref_name!r} holds a tab or newline")
        rows.append("\t".join(fields))
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(rows) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(rows)} items -> {OUT}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
