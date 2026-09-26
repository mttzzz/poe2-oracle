// The price panel's link to Craft of Exile (crates/poe2-oracle/src/craft_link.rs) and what it hands
// the site. The panel's top as panel.js draws it -- the title bar and the nameplate of an item
// copied in the Russian client, the Craft of Exile link lit as the pointer lights it (style.rs
// `link`) and a note pointing at it -- and under it the item as the link carries it: the game's
// ids of its base and mods, each mod's rolls in the game's order, which read the same from either
// client.
//
// The data (data/craft.<lang>.json):
//   panel     the panel's top, as data/panel.<lang>.json has it: { icons, league, rate, item,
//             links }
//   craft     the index in `panel.links` of the Craft of Exile link
//   callout   the note on the link: what it does
//   carried   { title, lead, labels: an export key -> its row's name }: the words over the rows
//   export    the keys of the site's export the link fills for the item (craft_link.rs `Export`),
//             as its test of the same ring reads them back: `i` the base, `l` the item level, `r`
//             the rarity, `ip` the implicits' rolls, `m` the mods, each `k` with its rolls `v`
//   language  the site's language the link asks for, or none (`site_language`)

import { callouts } from "./callout.js";
import { gameFrame, h } from "./dom.js";
import { nameplate, titleBar } from "./panel.js";

export function render(data, lang) {
    const panel = h(
        "div",
        "oui-panel oui-craft-panel",
        titleBar(data.panel),
        nameplate(data.panel.item, data.panel.links, lang),
        gameFrame(),
    );
    panel.style.setProperty("--dp", `${1 / (window.devicePixelRatio || 1)}px`);
    const link = panel.querySelectorAll(".oui-panel-links > .oui-link")[data.craft];
    link.classList.add("oui-craft-link");
    return h(
        "div",
        "oui-craft",
        callouts(panel, [{ text: data.callout, target: link, side: "below" }]),
        carried(data),
    );
}

/** What the link carries, row by row: the export's keys, then the site's language if it asks one. */
function carried({ carried: { title, lead, labels }, export: item, language }) {
    const rolls = (values) => h("span", "oui-craft-rolls", `[${values.join(", ")}]`);
    const rows = [
        [labels.i, item.i],
        [labels.l, String(item.l)],
        [labels.r, item.r],
        [labels.ip, item.ip.map(rolls)],
        [labels.m, item.m.map((mod) => h("span", null, mod.k, " ", rolls(mod.v)))],
        language && [labels.language, language],
    ].filter(Boolean);
    return h(
        "div",
        "oui-craft-carried",
        h("div", "oui-craft-title", title),
        h("p", "oui-craft-lead", lead),
        h(
            "dl",
            null,
            rows.map(([label, value]) => [h("dt", null, label), h("dd", null, value)]),
        ),
    );
}
