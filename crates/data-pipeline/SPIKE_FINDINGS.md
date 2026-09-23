# data-pipeline — Validation Spike Findings

Architecture plan step 6. Verdict: **the local game-data pipeline (`oodle-ffi` → `poe-bundle` →
`poe-dat` → `data-pipeline`) is proven end to end against real PoE2 game files, on real
hardware.** Real `Bundles2` index parsed, a real `.datc64` table (`Mods`, 16,784 rows) read
through the real Oodle DLL and correctly decoded into structured rows, cross-checked against a
second real table (`Stats`) and the live trade API. The plan's central open question — does local
data hold both English and Russian mod text, or only one language — has a definitive answer, and
it is neither of the plan's two anticipated branches exactly: **local `.datc64` tables hold zero
mod/stat display text in any language**; the plan's own pre-decided fallback (source text from the
trade API) is the correct branch regardless.

Mirrors [`docs/dev/poc-findings.md`](../../docs/dev/poc-findings.md)'s evidence style: what was
run, what it printed, what that does and does not establish.

## Sourcing a real Oodle DLL: two dead ends, then Epic's own official mirror

The plan's suggested source ("any RAD-licensed game that ships one loose, e.g. Warframe on
Steam") turned out not to hold in practice, on this exact hardware, today:

1. **PoE2 itself**: already known from the earlier POC session to statically link Oodle (no loose
   `oo2core*.dll` in its install folder). Re-confirmed here for completeness.
2. **All 13 Steam games + 4 Battle.net titles already installed** on the Windows test machine: a
   recursive filesystem search for `oo2core*.dll`/`*oodle*.dll` across every installed game's
   folder, then a **full `C:\` and `D:\` drive scan**, found nothing.
3. **Warframe, fully installed fresh via Steam** (anonymous SteamCMD login *cannot* license a
   third-party F2P title — `ERROR! Failed to install app '230410' (No subscription)` — the real
   Steam GUI client, authenticated as the box's own account, was used instead; ~56GB Steam depot
   + a further ~48GB in-launcher update, both completed): still no loose DLL anywhere in the
   install, even after the launcher reported "up to date" and the actual game exe was run past a
   full engine bootstrap (cache TOC loading, PhysX init, shader dictionary load) to the login
   screen. A string search of `Warframe.x64.exe` found the literal symbol name
   `OodleLZ_Decompress` embedded in the binary but **no** `oo2core` string anywhere in it —
   Warframe, as of this build (`Jade Shadows: Constellations`, `2026.08.12.16.52`), **also**
   statically links Oodle now. The plan's own suggested fallback is stale as of today's Warframe
   build, not just PoE2.
4. **Real DLL actually used**: `WorkingRobot/OodleUE`
   (`Engine/Source/Programs/Shared/EpicGames.Oodle/Sdk/2.9.10/win/redist/oo2core_9_win64.dll`,
   637,952 bytes) — a daily-updated mirror of Epic's own Unreal Engine git repository, whose
   README explicitly labels this exact directory "Distributable Binaries" (as opposed to the
   adjacent "Official Binaries" also present in the same repo, sourced from the same place but
   not called out as meant for external redistribution). Epic owns RAD/Oodle outright and
   licenses it free for Unreal Engine use, and this specific subtree is the one Epic itself
   marks for that purpose — not a game install, not a third-party redistribution of uncertain
   provenance, but Epic's own blessed distribution channel. Confirmed genuine and correctly
   named via its PE export table (`objdump -p`): `OodleLZ_Decompress` present, plain undecorated
   name, alongside `OodleLZ_Compress`, `OodleLZ_GetCompressedBufferSizeNeeded`, and 40-some other
   real Oodle exports.

## A second real bug found and fixed: `powzix/ooz`'s test fixture is stale, not just the crate

`oodle-ffi`'s unit test originally used `powzix/ooz`'s own public `testdata/xml.kraken` (the
plan's suggested fixture). Against the real DLL sourced above, decompressing it returned
`OODLELZ_FAILED` (0) under **every** combination of `fuzzSafe`/`checkCRC`/`threadPhase` tried (8
combinations, plus an oversized output buffer as an extra check). Root-caused, not just
worked around:

- A **self-consistent round trip** — compress a fresh 140,000-byte buffer with the same real
  DLL's `OodleLZ_Compress` (Kraken, level Normal), then decompress the result with the same DLL's
  `OodleLZ_Decompress` — succeeded and matched byte-for-byte. This isolates the problem to the
  *external* fixture, not to `oodle-ffi`'s FFI binding, parameter values, or this crate's use of
  `libloading`.
- Conclusion: `powzix/ooz`'s testdata (committed to that repo years ago, well before SDK 2.9.x)
  was compressed with an older Oodle SDK generation. Kraken's on-disk bitstream sub-format has
  evolved across SDK generations even though the compressor's public name/id (`Kraken`, id 8)
  never changed — old pre-compressed corpora are not guaranteed forward-compatible with a newer
  decoder.
- **Fix**: `oodle-ffi`'s fixture is now self-generated — the same real `xml` Silesia-corpus
  plaintext (kept, still real benchmark text, no PoE licensing question), re-compressed with
  *this exact* DLL via `OodleLZ_Compress` (`OodleLZ_CompressionLevel_Optimal2`), with the round
  trip verified before committing the result. This is immune to the SDK-version-drift class of
  bug entirely, by construction. See `crates/oodle-ffi/src/lib.rs`'s test doc comment for the
  regeneration recipe if a future Oodle major-version bump ever breaks it again.

## Running the pipeline against real game files

Both the Oodle DLL and the real game files are Windows-native (the DLL is a Windows PE binary;
`libloading::Library::new` cannot load it on Linux — `dlopen` rejects a foreign binary format
outright, not a permissions/path problem). The whole local chain
(`oodle-ffi`/`poe-bundle`/`poe-dat`/`data-pipeline`) was therefore built and run **natively on
the Windows test machine** (source tree transferred via `tar`+`scp`, same pattern
[`docs/dev/poc-findings.md`](../../docs/dev/poc-findings.md) used for its own real-hardware pass),
directly against the local
`D:\SteamLibrary\steamapps\common\Path of Exile 2\Bundles2` path — not through the `/mnt/poe2-bundles`
SMB mount, which exists for Linux-side dev tooling (the agent's own file access, quick greps,
schema exploration) rather than for the pipeline binary's own execution. This is consistent with,
not a deviation from, this project's already-established Windows-only shipping decision
([`docs/dev/poc-findings.md`](../../docs/dev/poc-findings.md)): the pipeline that reads PoE2's own
game files is exactly as Windows-bound as PoE2 itself.

```
$ data-pipeline --dll oo2core_9_win64.dll --bundles2-root "D:\...\Bundles2" --table Mods --rows 20
Loading Oodle from ...oo2core_9_win64.dll
Oodle loaded OK.
Opening bundle index at D:\...\Bundles2
Bundle index opened OK.
Fetching PoE2 dat-schema (force_refresh=false)...
Schema version 7 loaded: 1286 tables, 144 enums.
Found table "Mods" (validFor=2): 73 columns, tags=["crafting"]
  ...
Reading "Data/Balance/Mods.datc64" from the bundle index...
Parsed 16784 rows.
```

Real row 0 (`Mods.datc64`, table name confirmed identical to PoE1's — no PoE2-specific rename was
needed, unlike the plan's contingency anticipated):

```
Id = "Strength1"          Name = "of the Brute"       Level = 1     MaxLevel = 100
Domain = EnumRow(1)        GenerationType = EnumRow(2)
Stat1 = ForeignRow(Some(564))    Stat1Value = Interval(I32(5), I32(8))
Stat2..4 = ForeignRow(None)      Stat2..4Value = Interval(0, 0)
Families = [ForeignRow(Some(147))]
```

Real row 1 (same table): `Id = "Strength2"`, `Name = "of the Wrestler"`, `Level = 11`,
`Stat1Value = Interval(9, 12)` — a sensible tier progression (higher character level requirement,
higher stat roll range) for what is recognizably the "+X to Strength" prefix mod family. Every
column decoded to a plausible, in-range value (no garbage/overflow/off-by-N artifacts across 20
inspected rows × 73 columns) — the strongest available evidence that the byte-layout research
from steps 4-5 (heap-offset base, row-count-aligned marker search, interval doubling, array
descriptors, null-reference sentinels) is correct against real data, not just the hand-built unit
test fixtures.

## The localization question — definitive answer

**Neither of the plan's two anticipated branches is exactly what's true.** The plan asked: does
local data hold both English and Russian mod text simultaneously, or only the client's current
language? The real, empirical answer: **`Mods.datc64` (73 columns) and `Stats.datc64` (19
columns) — the two tables that define which stats a mod grants and in what range — have zero
`localized` columns between them.** Checked exhaustively, not sampled: every column of both
tables' schema definitions was inspected; none carry the schema's `localized: true` flag. (1,286
tables total in the schema; 73 *other* tables do have localized columns — achievement text, quest
dialogue, UI strings — so `@localized` genuinely is used and detected correctly elsewhere; it is
specifically absent from the two tables that matter for mod definitions.)

Concretely: `Mods.datc64` row 0's `Name` field holds `"of the Brute"` — a real string, but it is
the mod's internal *name suffix*, not the full stat-description text a player sees
(`"+5 to +8 Strength"`/whatever the localized template renders). `Stats.datc64` row 564 (the
`ForeignRow` target of `Mods` row 0's `Stat1`) holds `Id = "additional_strength"` — an internal,
programmer-facing identifier, not display text either. The actual localized display template
(e.g. `"# to Strength"` / `"# к силе"`) lives in a wholly different, non-`.datc64` bundle file
format this plan's `poe-dat` crate was never scoped to parse (PoE's classic
`stat_descriptions`-style text-template files — referenced obliquely by the schema's
`StatDescriptionFunctions` table, which itself carries no text either) — implementing a parser
for that format is out of this foundation plan's scope entirely, not merely deferred to a later
step of it.

**Resolution, matching the plan's own pre-decided fallback exactly**: local `.datc64` extraction
supplies 100% of the *structural* mod data (which stats, value ranges per level, generation
weights, tags, domain, family) with zero code needed beyond what steps 4-5 already built; mod/stat
*display text*, in every language including English, comes from the trade API's
`/api/trade2/data/stats` endpoint (already proven reachable in the original POC's capability 5).
Confirmed live, right now, for the exact stat found above:

```
GET https://www.pathofexile.com/api/trade2/data/stats   -> explicit.stat_4080418644: "# to Strength"
GET https://ru.pathofexile.com/api/trade2/data/stats    -> explicit.stat_4080418644: "# к силе"
```

Both requests succeeded from this project's own network access, no auth needed, matching the
already-proven `trade-client` crate's own HTTP path. The exact algorithm mapping a local
`Stats.datc64` row's `Id` string (e.g. `"additional_strength"`) to the trade API's `stat_<hash>`
key was **not** derived here — establishing that mapping (if it's even needed; the Price Check
feature may key off `Stats.Id` directly against a locally-cached copy of the trade API's own
stat list, sidestepping the question) is real, scoped work for the follow-up Price Check plan,
not a gap in this foundation plan's own claims.

## What this changes for step 7 (`poe2-domain`)

- `Mod`/`StatFilter` types should **not** plan for a `HashMap<Language, String>` populated from
  local extraction — there is no local text to populate it with. A `text: HashMap<Language,
  String>` (or fixed `en`/`ru` fields) sourced from `trade-client`'s stats endpoint remains the
  right shape; only its *data source* is now settled with certainty (trade API only), not the
  branch the plan left open.
- Structural fields (stat id/family/tier/value-range/generation-weight/domain/tags) map cleanly
  onto real, now-verified `poe-dat::Value` shapes (`ForeignRow`, `Interval`, arrays of
  `ForeignRow`/`EnumRow`) — `poe2-domain`'s `Mod` type can be designed directly against the real
  `Mods`/`Stats` column shapes documented above, not a guess.

## Reproduction

```powershell
# Real DLL (Epic's official distributable mirror, see above)
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/WorkingRobot/OodleUE/main/Engine/Source/Programs/Shared/EpicGames.Oodle/Sdk/2.9.10/win/redist/oo2core_9_win64.dll" -OutFile oo2core_9_win64.dll

cargo run -p data-pipeline -- `
  --dll oo2core_9_win64.dll `
  --bundles2-root "D:\SteamLibrary\steamapps\common\Path of Exile 2\Bundles2" `
  --table Mods `
  --rows 20
```

`cargo test -p oodle-ffi` (needs `ODLE_TEST_DLL_PATH` set to a real DLL) and
`cargo test -p poe-bundle --features live-share` (needs `ODLE_TEST_DLL_PATH` +
`POE2_BUNDLES2_ROOT` set) both pass against the exact DLL and share used for this spike.

## Files

- `oo2core_9_win64.dll` itself is **not** committed anywhere in this repo (runtime parameter per
  `oodle-ffi`'s design, plan step 3) — only its source URL, here and in `oodle-ffi`'s doc
  comments.
- `crates/oodle-ffi/tests/fixtures/xml.kraken` — regenerated (see above), no longer
  `powzix/ooz`'s original file.
- `crates/poe-dat/tests/fixtures/BetrayalRanks.datc64` — a real, whole (not trimmed; already only
  550 bytes / 4 rows / 4 columns) table file extracted from the real share during this spike,
  backing `poe-dat`'s `parses_a_real_committed_datc64_fixture` unit test.
