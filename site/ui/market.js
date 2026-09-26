// The price panel as it shows a Currency Exchange item, down to its market card
// (crates/poe2-oracle/src/ui/panel/market.rs): the title bar with the league and the divine rate
// (title_bar.rs), the item's nameplate and chips (nameplate.rs), and the card -- the value in the
// unit that reads best, the other core currencies, how many a divine buys, poe2scout's week as a
// line over a fading area, the volume, the busiest pair, the copied stack's worth and the hours
// the rate is for. The data is the app's own: the market's numbers (trade_client::cx::Market,
// MarketPrice), written here as the app writes them (crate::i18n); the words are the app's, with
// the Russian of its catalog (assets/i18n/ru/panel.json).

import { h, chip, gameFrame, icon, link, ornamentRule, select } from "./dom.js";

// The app's words in Russian; English is the key.
const RUSSIAN = {
    "wiki ↗": "вики ↗",
    "report a problem": "сообщить о проблеме",
    "Stack Size:": "В стопке:",
    "Last 7 days": "За 7 дней",
    "not enough data": "мало данных",
    "Volume per hour": "Оборот в час",
    "Most traded pair": "Чаще всего меняют",
    "Your stack": "Ваша стопка",
    "×{count} ≈": "{count} шт. ≈",
    "{category} · rate for {hours}": "{category} · курс за {hours}",
    Currency: "Валюта",
    Fragments: "Фрагменты",
    Verisium: "Веризий",
    Runes: "Руны",
    Expedition: "Экспедиция",
    Vaal: "Ваал",
    Delirium: "Делириум",
    Breach: "Разлом",
    Ritual: "Ритуал",
    "Abyssal Bones": "Кости Бездны",
    Essences: "Сущности",
    "Uncut Gems": "Неогранённые камни",
    "Lineage Support Gems": "Династические камни поддержки",
    Waystones: "Путевые камни",
};

// The trade site's exchange groups by id, named as the site names them (market.rs category_name).
const CATEGORIES = { Abyss: "Abyssal Bones", UncutGems: "Uncut Gems", LineageSupportGems: "Lineage Support Gems" };

// A name's colour by the item's rarity, or by its kind where it has none (nameplate.rs name_color).
const NAME_COLORS = {
    normal: "var(--rarity-normal)",
    magic: "var(--rarity-magic)",
    rare: "var(--rarity-rare)",
    unique: "var(--rarity-unique)",
    currency: "var(--currency-name)",
    gem: "var(--gem-name)",
};

// Divines at or below this read in exalted (trade_client::cx DIVINE_UNIT_CUTOVER, EE2's rule).
const DIVINE_UNIT_CUTOVER = 0.94;

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// Numbers the sparklines' gradients, unique on a page that draws several cards.
let fades = 0;

/** Draws the panel for `data` (see data/market.en.json) in `lang`, "en" or "ru". */
export function render(data, lang) {
    const say = (text, args = {}) =>
        (lang === "ru" ? (RUSSIAN[text] ?? text) : text).replace(/\{(\w+)\}/g, (_, name) => args[name]);
    const numbers = formats(lang);
    const icons = data.icons;
    const currency = (id, size) => icon(icons[id], size, id);
    const amountIn = (amount, id, size = 14) => h("span", "oui-market-amount", amount, currency(id, size));

    return h(
        "div",
        "oui-market",
        titleBar(data, numbers, currency),
        nameplate(data.item, icons, say),
        h(
            "div",
            "oui-market-body",
            chips(data.item, say),
            marketCard(data, numbers, say, currency, amountIn),
        ),
        gameFrame(),
    );
}

// The league select, the divine rate, the gear and the × (title_bar.rs render_title_bar).
function titleBar(data, { compact }, currency) {
    return h(
        "div",
        "oui-titlebar oui-market-title",
        select(data.league, true),
        h(
            "div",
            "oui-market-drag",
            h(
                "div",
                "oui-market-rate",
                "1",
                currency("divine", 16),
                `= ${compact(data.market.exaltedPerDivine)}`,
                currency("exalted", 16),
            ),
        ),
        h("span", "oui-market-button", gear()),
        h("span", "oui-market-button", "×"),
    );
}

// The gear GPUI's font fallback draws for the bar's ⚙: Segoe UI Emoji's, flat and grey -- eight
// rounded teeth on a light wheel, a darker groove, a light rim around its hole.
function gear() {
    const root = svg("svg", { class: "oui-market-gear", viewBox: "-15 -17 32 32", "aria-hidden": "true" });
    const wheel = svg("g", { fill: "#b4acbc" });
    wheel.append(
        svg("path", {
            "fill-rule": "evenodd",
            d: "M12 0A12 12 0 1 1-12 0A12 12 0 1 1 12 0ZM5 0A5 5 0 1 0-5 0A5 5 0 1 0 5 0Z",
        }),
    );
    for (let tooth = 0; tooth < 8; tooth += 1) {
        wheel.append(
            svg("rect", { x: -3.8, y: -15.8, width: 7.6, height: 6.8, rx: 1.9, transform: `rotate(${tooth * 45})` }),
        );
    }
    root.append(wheel, svg("circle", { r: 8.05, fill: "none", stroke: "#998ea4", "stroke-width": 2.7 }));
    return root;
}

// The item's art, its name in its colour and its client's face, the links, the rule under them
// (nameplate.rs render_nameplate).
function nameplate(item, icons, say) {
    const color = NAME_COLORS[item.rarity] ?? item.rarity ?? NAME_COLORS.normal;
    const art = item.art ?? icons[item.id];
    const links = item.links.map((key) =>
        link({ poe2db: "poe2db ↗", wiki: say("wiki ↗"), craft: "Craft of Exile ↗", report: say("report a problem") }[key] ?? key),
    );
    return h(
        "div",
        { class: "oui-market-nameplate", "--mk-name": color },
        h(
            "div",
            "oui-market-head",
            art && h("img", { class: "oui-market-art", src: art, alt: "", decoding: "async" }),
            h(
                "div",
                "oui-market-names",
                h("div", { class: "oui-market-name", "data-face": item.site }, item.name),
            ),
            // An empty column as wide as the art keeps the name centred.
            art && h("span", "oui-market-art"),
        ),
        h("div", "oui-market-links", links),
        h(
            "div",
            "oui-market-rule",
            ornamentRule(`color-mix(in srgb, var(--bg-nameplate) 55%, ${color})`),
        ),
    );
}

// The item's class and its stack (nameplate.rs render_chips, as it reads for an exchange item).
function chips(item, say) {
    return h(
        "div",
        "oui-market-chips",
        item.class && chip(null, item.class),
        item.stack !== undefined && chip(say("Stack Size:"), String(item.stack)),
    );
}

function marketCard(data, numbers, say, currency, amountIn) {
    const { number, compact, percent, dayMonth } = numbers;
    const { market, price, item } = data;
    const divines = price.divineValue;
    const [value, unit] = valueNotInItself(market, item.id, divines);
    // The other core currencies it's worth: "= 0,0024 [div] · 0,019 [chaos]".
    const equivalents = [
        [divines, "divine"],
        [divines * market.exaltedPerDivine, "exalted"],
        [divines * market.chaosPerDivine, "chaos"],
    ]
        .filter(([, id]) => id !== unit && id !== item.id)
        .flatMap(([amount, id], index) => [index > 0 && "·", amountIn(number(amount), id)]);
    const change = price.change7d;
    const falling = change !== null && change !== undefined && change < 0;
    const stack = item.stack > 1 ? item.stack : null;

    const top = h(
        "div",
        "oui-market-price",
        currency(item.id, 44),
        h(
            "div",
            "oui-market-values",
            h("div", "oui-market-value", `≈ ${number(value)}`, currency(unit, 24)),
            h("div", "oui-market-equivalents", "=", equivalents),
            divines < 1 &&
                h("div", "oui-market-per-divine", amountIn("1", "divine"), "=", amountIn(compact(1 / divines), item.id)),
        ),
    );

    const week = h(
        "div",
        "oui-market-week",
        h(
            "div",
            "oui-market-week-head",
            h("span", null, say("Last 7 days")),
            h(
                "span",
                `oui-market-change oui-market-change--${falling ? "fall" : "rise"}`,
                change === null || change === undefined ? say("not enough data") : percent(signed(change)),
            ),
        ),
        sparkline(price.sparkline ?? [], falling ? "fall" : "rise"),
    );

    const line = (label, content) => h("div", "oui-market-line", h("span", null, label), content);
    const [currencyAmount, itemAmount] =
        price.mostTradedRate >= 1 ? [1, price.mostTradedRate] : [1 / price.mostTradedRate, 1];
    const stackLine = () => {
        const [total, totalUnit] = valueNotInItself(market, item.id, divines * stack);
        return line(
            say("Your stack"),
            h("span", "oui-market-stack", say("×{count} ≈", { count: stack }), amountIn(number(total), totalUnit)),
        );
    };
    const lines = h(
        "div",
        "oui-market-lines",
        line(
            say("Volume per hour"),
            h("span", "oui-market-volume", compact(price.volumeDivine), currency("divine", 14)),
        ),
        line(
            say("Most traded pair"),
            h(
                "span",
                "oui-market-pair",
                compact(currencyAmount),
                currency(price.mostTradedWith, 14),
                `⇆ ${compact(itemAmount)}`,
                currency(item.id, 14),
            ),
        ),
        stack && stackLine(),
    );

    const hours = price.hours;
    const when = `${hours.day ? `${dayMonth(hours.day, hours.month)} ` : ""}${hours.from}–${hours.to}`;
    const category = say(CATEGORIES[price.category] ?? price.category);
    const footer = h(
        "div",
        "oui-market-footer",
        h("span", null, say("{category} · rate for {hours}", { category, hours: when })),
        price.poe2scout && link("poe2scout ↗"),
    );

    return h("div", "oui-market-card", top, week, lines, footer);
}

// `divines` in the unit that reads best for it -- but a core currency is never priced in itself:
// a Divine Orb reads in exalted, an Exalted Orb in divines (market.rs value_not_in_itself).
function valueNotInItself(market, id, divines) {
    const shown = divines <= DIVINE_UNIT_CUTOVER ? [divines * market.exaltedPerDivine, "exalted"] : [divines, "divine"];
    if (shown[1] === "divine" && id === "divine") return [divines * market.exaltedPerDivine, "exalted"];
    if (shown[1] === "exalted" && id === "exalted") return [divines, "divine"];
    return shown;
}

// poe2scout's week: each day's change against its first, as a line over a fading area in the
// colour of the week's change -- evenly spaced days, the line broken where a day had no price,
// a y axis spanning at least -5..+5 % (market.rs render_sparkline). Drawn in the card's px (the
// app's at 100 % scale), 2 px in from the edges, the line 1.5 px wide.
function sparkline(points, tone) {
    const width = 464;
    const height = 48;
    const inset = 2;
    const values = points.filter((value) => value !== null && value !== undefined);
    const low = Math.min(-5, ...values);
    const high = Math.max(5, ...values);
    const step = (width - inset * 2) / Math.max(points.length - 1, 1);
    const at = (index, value) => [
        inset + step * index,
        inset + (height - inset * 2) * ((high - value) / (high - low)),
    ];
    const root = svg("svg", {
        class: `oui-market-sparkline oui-market-change--${tone}`,
        viewBox: `0 0 ${width} ${height}`,
        preserveAspectRatio: "none",
        "aria-hidden": "true",
    });
    // Each run of trading days is its own line.
    const runs = [[]];
    points.forEach((value, index) => {
        if (value === null || value === undefined) runs.push([]);
        else runs.at(-1).push(at(index, value));
    });
    const xy = ([x, y]) => `${x.toFixed(2)} ${y.toFixed(2)}`;
    for (const run of runs.filter((run) => run.length >= 2)) {
        fades += 1;
        const id = `oui-market-fade-${fades}`;
        const fade = svg("linearGradient", { id, x1: 0, y1: 0, x2: 0, y2: 1 });
        fade.append(
            svg("stop", { offset: 0, "stop-color": "currentColor", "stop-opacity": 0.35 }),
            svg("stop", { offset: 1, "stop-color": "currentColor", "stop-opacity": 0 }),
        );
        const line = run.map(xy).join(" L");
        const [first, last] = [run[0][0].toFixed(2), run.at(-1)[0].toFixed(2)];
        root.append(
            fade,
            svg("path", { d: `M${first} ${height} L${line} L${last} ${height} Z`, fill: `url(#${id})` }),
            svg("path", {
                d: `M${line}`,
                fill: "none",
                stroke: "currentColor",
                "stroke-width": 1.5,
                "stroke-linejoin": "round",
            }),
        );
    }
    return root;
}

/** An SVG element: `tag` with `attrs`. */
function svg(tag, attrs) {
    const element = document.createElementNS("http://www.w3.org/2000/svg", tag);
    for (const [name, value] of Object.entries(attrs)) element.setAttribute(name, value);
    return element;
}

// `+3`, `-14`: a whole percent with its sign, as Rust's `{:+.0}` writes it.
function signed(value) {
    const text = value.toFixed(0);
    return text.startsWith("-") ? text : `+${text}`;
}

// The app's number formats (crate::i18n) in `lang`.
function formats(lang) {
    const separated = (text) => (lang === "ru" ? text.replace(".", ",") : text);
    // Two decimals under 10, one under 100, none above, trailing zeros dropped; two significant
    // digits below 1.
    const number = (value) => {
        let places;
        if (value >= 100) places = 0;
        else if (value >= 10) places = 1;
        else if (value >= 1 || value <= 0) places = 2;
        else places = Math.min(8, Math.max(2, 1 - Math.floor(Math.log10(value))));
        let fixed = value.toFixed(places);
        if (fixed.includes(".")) fixed = fixed.replace(/0+$/, "").replace(/\.$/, "");
        return separated(fixed);
    };
    const compact = (value) =>
        value >= 1e6 ? `${number(value / 1e6)}M` : value >= 1e3 ? `${number(value / 1e3)}k` : number(value);
    const percent = (text) => (lang === "ru" ? `${text} %` : `${text}%`);
    const dayMonth = (day, month) =>
        lang === "ru"
            ? `${String(day).padStart(2, "0")}.${String(month).padStart(2, "0")}`
            : `${MONTHS[month - 1]} ${day}`;
    return { number, compact, percent, dayMonth };
}
