#!/usr/bin/env python3
"""Builds crates/item-parser/data/stat-matchers-{en,ru}.tsv: the ways the game client prints a
stat, each with the trade stat ids it counts toward, from Exiled Exchange 2's stat database
(renderer/public/data/{en,ru}/stats.ndjson, MIT, https://github.com/Kvan7/Exiled-Exchange-2).

The trade API's stat catalog holds one text per stat, while the client prints many stats in other
ways too: a negative roll in its own words (`20% снижение требований к характеристикам` is the
catalog's `#% увеличение ...` at -20), a 100% chance as a plain effect (`Dazes on Hit` for
`#% chance to Daze on Hit`), a count of one as a word (`You can apply one fewer Curse`).
`item_parser`'s catalog matching falls back on these rows once the catalog's own texts fail.

A row: the printed form (numbers as `#`, without a `+` before them -- the parser templates `+10`
to `#`, and line breaks as `\\n`), `n` when the form counts the stat the other way (the roll is
negated), the value a form that prints no number stands for (a flag's 100, "one fewer"'s -1), and
the trade ids, comma-joined. Left out: option stats (`+# to Level of all Spark Skills`), whose
options the catalog lists as entries of their own that the parser matches directly, and inverted
stats (`#% less Damage`), whose trade filter runs against the printed roll.

Usage (from the repo root):
    python3 packaging/data/generate_stat_matchers.py <EE2 renderer/public/data dir>
"""

import json
import sys
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parents[2] / "crates/item-parser/data"


def rows(stats_path):
    out, seen = [], set()
    with open(stats_path, encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            stat = json.loads(line)
            trade = stat.get("trade") or {}
            if trade.get("option") or trade.get("inverted"):
                continue
            # An item never prints a pseudo total; those ids only ever point the search.
            ids = [
                i
                for group in (trade.get("ids") or {}).values()
                for i in group
                if not i.startswith("pseudo.")
            ]
            if not ids:
                continue
            for matcher in stat.get("matchers", []):
                form = matcher["string"].strip().replace("+#", "#").replace("\n", "\\n")
                negate = "n" if matcher.get("negate") else ""
                value = matcher.get("value")
                value = "" if value is None else f"{value:g}"
                if "\t" in form:
                    sys.exit(f"a form of {stat.get('ref')!r} holds a tab")
                row = "\t".join((form, negate, value, ",".join(ids)))
                if row not in seen:
                    seen.add(row)
                    out.append(row)
    return out


def main(data_dir):
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for lang in ("en", "ru"):
        written = rows(Path(data_dir) / lang / "stats.ndjson")
        out = OUT_DIR / f"stat-matchers-{lang}.tsv"
        out.write_text("\n".join(written) + "\n", encoding="utf-8", newline="\n")
        print(f"{len(written)} forms -> {out}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
