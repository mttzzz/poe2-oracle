// The landing page's hero, the README's picture and the social preview: parts of the app, each
// drawn by its own component from its own data, laid out in columns on the game's dark.
//
// data: {
//   width?, height?          fix the frame, px at scale 1 (the social preview's 1200x630);
//   title?, tagline?         a heading block, drawn in a column that asks for it;
//   columns: [[ item ]]      left to right, each top to bottom. An item is { "part": "panel" |
//                            "market" | "xp" | "settings", "set"?, "scale"?, "listings"?,
//                            "plates"? } -- its data set (default: the part's), UI scale, and
//                            for the panel how many listings it keeps, for the XP overlay how many
//                            plates -- or { "heading": true } for the title and tagline.
// }

import { h } from "./dom.js";

const here = new URL(".", import.meta.url);

async function part(spec, lang) {
    const name = spec.part;
    const [module, data] = await Promise.all([
        import(new URL(`${name}.js`, here).href),
        fetch(new URL(`data/${spec.set ?? name}.${lang}.json`, here)).then((answer) => answer.json()),
    ]);
    if (spec.listings && Array.isArray(data.listings)) data.listings = data.listings.slice(0, spec.listings);
    if (spec.plates && Array.isArray(data.plates)) data.plates = data.plates.slice(0, spec.plates);
    return h(
        "div",
        { class: `oui-hero-part oui-hero-${name}`, "--part": spec.scale ?? 1 },
        await module.render(data, lang),
    );
}

function heading(data) {
    return h(
        "div",
        "oui-hero-heading",
        h("div", "oui-hero-title oui-display", data.title),
        data.tagline && h("div", "oui-hero-tagline", data.tagline),
    );
}

export async function render(data, lang) {
    const columns = await Promise.all(
        data.columns.map(async (column) =>
            h(
                "div",
                "oui-hero-column",
                await Promise.all(column.map((item) => (item.heading ? heading(data) : part(item, lang)))),
            ),
        ),
    );
    return h(
        "div",
        {
            class: "oui-hero",
            style: data.width
                ? { width: `calc(${data.width} * var(--u))`, height: `calc(${data.height} * var(--u))` }
                : null,
        },
        columns,
    );
}
