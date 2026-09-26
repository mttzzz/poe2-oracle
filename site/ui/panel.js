// The price-check panel (crates/poe2-oracle/src/ui/panel/) drawn from data, top to bottom: the
// title bar (title_bar.rs), the nameplate and the info chips (nameplate.rs), the profile row, the
// filter sections (filters.rs), the search row, the Found line and the listings (results.rs), all
// in the game frame.
//
// The data (data/panel.<lang>.json), every word already in the interface language and every
// number already written as the app writes it there:
//   height    the panel's height, px at 100 % scale (the app's panel is as tall as the game);
//             none for just its content
//   icons     currency id -> icon URL, for `rate`, `search.currency` and `listings`
//   league    the league select's choice
//   rate      Exalted Orbs per Divine Orb, or none
//   item      { art, name, base, rarity: normal|magic|rare|unique|currency|gem, lang: the client
//             language the item was copied in, which sets its name's face }
//   links     the nameplate's links, arrows included
//   chips     [{ label, value, toggle: the edged ↔ kind, color: "value"|"warning" }]
//   profile   { label, choice }, or none
//   bounds    the min and max boxes' placeholders { min, max }
//   sections  [{ title, rows, chips }]: a row { checked, tier, source: { kind: desecrated|
//             fractured|enchant|crafted|augment, label }, sum: its "sum" badge, text: the stat
//             with # where the roll goes, value, min, max, slider: { at, roll, max } (0-1 along
//             the track: the thumb and the item's own roll; `max` where lower is better), free:
//             an empty affix slot, note: said under a row the search can't use, which has no
//             checkbox or boxes }; a chip { text, value }, an unchecked property folded under
//             its section's rows
//   folded    the line that unfolds the rows kept out of sight, or none
//   search    { button, sellers, currency: a string, or strings and { icon: id } in a row }
//   found     { label, count }
//   trade     the trade site link
//   table     the column headings { price, level, seller (none: the column hidden), listed }
//   listings  [{ amount, currency: an icon id or a name, about + unit: what it comes to (≈),
//             marker: "× 3" or "?", level, seller, status: online|afk|offline (none for an
//             instant buyout), listed }], or none before a search

import {
    checkbox,
    checkChip,
    chip,
    gameFrame,
    h,
    icon,
    link,
    ornamentRule,
    sectionHeading,
    select,
    toggleChip,
} from "./dom.js";

const NAME_COLORS = {
    normal: "var(--rarity-normal)",
    magic: "var(--rarity-magic)",
    rare: "var(--rarity-rare)",
    unique: "var(--rarity-unique)",
    currency: "var(--currency-name)",
    gem: "var(--gem-name)",
};

const CHIP_COLORS = { value: "var(--text-value)", warning: "var(--text-warning)" };

export function render(data, lang) {
    const panel = h(
        "div",
        { class: "oui-panel", "--panel-height": data.height ?? null },
        titleBar(data),
        h(
            "div",
            "oui-panel-scroll",
            nameplate(data.item, data.links, lang),
            h(
                "div",
                "oui-panel-body",
                h(
                    "div",
                    "oui-panel-chips",
                    (data.chips ?? []).map((fact) =>
                        fact.toggle
                            ? toggleChip(fact.label, fact.value)
                            : chip(fact.label, fact.value, CHIP_COLORS[fact.color]),
                    ),
                ),
                data.profile &&
                    h(
                        "div",
                        "oui-panel-toolbar",
                        h(
                            "div",
                            "oui-panel-profile",
                            h("span", "oui-panel-profile-label", data.profile.label),
                            h("div", "oui-panel-profile-select", select(data.profile.choice, true)),
                        ),
                    ),
                h(
                    "div",
                    "oui-panel-sections",
                    (data.sections ?? []).map((section) => filterSection(section, data.bounds)),
                    data.folded && h("div", "oui-panel-folded", link(data.folded)),
                ),
                searchRow(data.search, data.icons),
                data.listings && h("div", "oui-panel-ornament", ornamentRule()),
                data.listings && results(data),
            ),
        ),
        gameFrame(),
    );
    // GPUI lays every length out in whole device pixels (gpui/src/taffy.rs): the lengths in
    // panel.css round to this page's, as the app's round to its screen's.
    panel.style.setProperty("--dp", `${1 / (window.devicePixelRatio || 1)}px`);
    return panel;
}

/** The league select, the divine rate, the ⚙ and the ×. */
export function titleBar(data) {
    return h(
        "div",
        "oui-panel-title oui-titlebar",
        h("div", "oui-panel-league", select(data.league, true)),
        h(
            "div",
            "oui-panel-drag",
            h("div", "oui-panel-spacer"),
            data.rate &&
                h(
                    "div",
                    "oui-panel-rate",
                    h("span", null, "1"),
                    icon(data.icons.divine, 16),
                    h("span", null, `= ${data.rate}`),
                    icon(data.icons.exalted, 16),
                ),
        ),
        h("div", "oui-panel-title-button", gear()),
        h("div", "oui-panel-title-button", "×"),
    );
}

/**
 * The ⚙ as Windows draws it in GPUI: Segoe UI Emoji's lavender gear, whatever font the visitor
 * has.
 */
function gear() {
    const ns = "http://www.w3.org/2000/svg";
    const node = (tag, attrs) => {
        const element = document.createElementNS(ns, tag);
        for (const [name, value] of Object.entries(attrs)) element.setAttribute(name, value);
        return element;
    };
    const drawing = node("svg", { class: "oui-panel-gear", viewBox: "0 0 16 16", "aria-hidden": "true" });
    for (let turn = 0; turn < 8; turn++) {
        drawing.append(
            node("rect", {
                x: 6,
                y: 0.2,
                width: 4,
                height: 4.6,
                rx: 1.2,
                fill: "#b4acbc",
                transform: `rotate(${turn * 45} 8 8)`,
            }),
        );
    }
    drawing.append(
        node("circle", { cx: 8, cy: 8, r: 4.35, fill: "none", stroke: "#b4acbc", "stroke-width": 4.1 }),
        node("circle", { cx: 8, cy: 8, r: 3.85, fill: "none", stroke: "#998ea4", "stroke-width": 0.9 }),
    );
    return drawing;
}

/**
 * The item's art, its name and base in its rarity's colour and its own client's face on the
 * banner that colour tints, its links, and the rule under it all.
 */
export function nameplate(item, links, lang) {
    const color = NAME_COLORS[item.rarity] ?? NAME_COLORS.normal;
    return h(
        "div",
        { class: "oui-panel-nameplate", "--name": color },
        h(
            "div",
            "oui-panel-head",
            item.art && h("img", { class: "oui-panel-art", src: item.art, alt: "", decoding: "async" }),
            h(
                "div",
                { class: "oui-panel-names", "data-face": item.lang ?? lang },
                h("div", "oui-panel-name", item.name),
                item.base && h("div", "oui-panel-base", item.base),
            ),
            item.art && h("div", "oui-panel-art"),
        ),
        h("div", "oui-panel-links", (links ?? []).map((label) => link(label))),
        h(
            "div",
            "oui-panel-plate-rule",
            ornamentRule("color-mix(in srgb, var(--name) 45%, var(--bg-nameplate))"),
        ),
    );
}

/** A section: its heading, its rows, and its unchecked properties folded into chips. */
function filterSection(section, bounds) {
    return h(
        "div",
        "oui-panel-section",
        h("div", "oui-panel-heading", sectionHeading(section.title)),
        (section.rows ?? []).map((row) => filterRow(row, bounds)),
        section.chips?.length &&
            h(
                "div",
                "oui-panel-property-chips",
                section.chips.map((property) =>
                    checkChip(h("span", "oui-panel-chip-text", statLine(property.text, property.value))),
                ),
            ),
    );
}

/**
 * A filter row: the checkbox, the tier and source badges, the stat with its roll, the min and
 * max boxes -- and, for a checked mod the tier table knows, its roll slider.
 */
function filterRow(row, bounds) {
    const searchable = !row.note;
    const badges = (row.tier || row.source || row.sum) &&
        h(
            "div",
            "oui-panel-badges",
            row.tier && h("span", `oui-panel-tier oui-panel-tier--${Math.min(row.tier, 3)}`, `T${row.tier}`),
            row.source && h("span", `oui-panel-source oui-panel-source--${row.source.kind}`, row.source.label),
            row.sum && h("span", "oui-panel-sum", row.sum),
        );
    const text = row.free
        ? h("div", "oui-panel-text oui-panel-text--free", row.text)
        : h("div", "oui-panel-text", statLine(row.text, row.value));
    return h(
        "div",
        `oui-panel-row${row.checked && searchable ? " oui-panel-row--on" : ""}`,
        h(
            "div",
            "oui-panel-line",
            h("div", "oui-panel-check", searchable && checkbox(Boolean(row.checked))),
            badges,
            text,
            searchable &&
                row.value !== undefined &&
                h(
                    "div",
                    "oui-panel-bounds",
                    boundBox(row.min, bounds.min),
                    boundBox(row.max, bounds.max),
                ),
        ),
        searchable && row.checked && row.slider && rollSlider(row.slider),
        !searchable && h("div", "oui-panel-note", row.note),
    );
}

/** The stat with its roll where the `#` is, the roll in the game's blue -- `stat_text`. */
function statLine(template, value) {
    const pieces = template.split("#");
    if (value === undefined || pieces.length === 1) return template;
    const roll = h("b", "oui-panel-roll", value);
    if (pieces.length === 2) return [pieces[0].replace(/[+-]+$/, ""), roll, pieces[1]];
    return [template, ": ≈", roll];
}

/** A min or max box: its bound, or its placeholder dimmed. */
function boundBox(value, placeholder) {
    return value
        ? h("div", "oui-panel-bound", h("span", null, value))
        : h("div", "oui-panel-bound oui-panel-bound--empty", placeholder);
}

/** A track from the stat's lowest roll to its highest, the item's own roll marked, the thumb at
    the search's bound and the part of the track the search admits lit. */
function rollSlider({ at, roll, max }) {
    const [from, to] = max ? [0, at] : [at, 1];
    return h(
        "div",
        "oui-panel-slider",
        h(
            "div",
            "oui-panel-track",
            h("span", { class: "oui-panel-admitted", style: { left: pct(from), right: pct(1 - to) } }),
            roll !== undefined && h("span", { class: "oui-panel-notch", style: { left: pct(roll) } }),
            h("span", { class: "oui-panel-thumb", style: { left: pct(at) } }),
        ),
    );
}

const pct = (fraction) => `${fraction * 100}%`;

/** The «Поиск» plate, and the sellers and currency selects beside it. */
function searchRow(search, icons) {
    const choice = (Array.isArray(search.currency) ? search.currency : [search.currency]).map((part) =>
        typeof part === "string" ? h("span", null, part) : currency(icons, part.icon, 14),
    );
    return h(
        "div",
        "oui-panel-search-row",
        h("div", "oui-panel-search-cell", h("div", "oui-panel-search oui-display", search.button)),
        h(
            "div",
            "oui-panel-selects",
            h("div", "oui-panel-stepping", select(search.sellers, true)),
            h("div", "oui-panel-stepping", select(h("span", "oui-panel-currency", choice), true)),
        ),
    );
}

/** The Found line -- the count and the trade site link -- and the table. */
function results(data) {
    const { found, table } = data;
    return h(
        "div",
        "oui-panel-results",
        h(
            "div",
            "oui-panel-found",
            h("div", "oui-panel-count", h("span", "oui-panel-dim", found.label), h("span", null, found.count)),
            link(data.trade),
        ),
        h(
            "div",
            "oui-panel-table",
            h(
                "div",
                "oui-panel-table-row oui-panel-table-head",
                h("div", "oui-panel-price", table.price),
                h("div", "oui-panel-level", table.level),
                h("div", "oui-panel-seller", table.seller),
                h("div", "oui-panel-listed", table.listed),
            ),
            data.listings.map((listing) => listingRow(listing, data.icons)),
        ),
    );
}

/** A listing: its price and what that is worth, the item level, the seller, how long ago. */
function listingRow(listing, icons) {
    const marker =
        listing.marker &&
        h(
            "span",
            listing.marker === "?" ? "oui-panel-unsure" : "oui-panel-times",
            listing.marker,
        );
    return h(
        "div",
        "oui-panel-table-row oui-panel-listing",
        h(
            "div",
            "oui-panel-price",
            h("span", "oui-panel-amount", listing.amount),
            currency(icons, listing.currency, 20),
            marker,
            listing.about &&
                h(
                    "div",
                    "oui-panel-about",
                    h(
                        "div",
                        "oui-panel-amount-in",
                        h("span", null, `≈ ${listing.about}`),
                        currency(icons, listing.unit, 14),
                    ),
                ),
        ),
        h("div", "oui-panel-level", listing.level),
        h("div", "oui-panel-seller", listing.seller && h("div", null, listing.seller)),
        h(
            "div",
            "oui-panel-listed",
            h("span", `oui-panel-status${listing.status ? ` oui-panel-status--${listing.status}` : ""}`),
            listing.listed,
        ),
    );
}

/** A currency by its icon, or by its name where there's none: every price in the panel reads so. */
function currency(icons, id, size) {
    return icons?.[id] ? icon(icons[id], size) : h("span", "oui-panel-currency-name", id);
}
