// PoE2 Oracle next to other price checkers, and its own speed and size, all from data/bench.json:
// the one file that holds every such number the landing pages show, so that a new measurement
// changes that file and nothing else. [data-bench-chart] gets the comparison, a table whose memory
// column is a bar chart; each [data-bench="<name>"] gets one value, in the page's language:
//
//   memory, processes        PoE2 Oracle's memory in MB to one decimal, as Task Manager shows it,
//                            and its process count
//   ready                    from launch to ready to price
//   item_text, processing    a price check: the item's text after the copy, then reading it,
//                            its filters and the panel
//   exchange                 an exchange item's price, or a repeated search's, from loaded data
//   listings,                a search with listings, and the part of it pathofexile.com took
//   listings_trade_site
//   installer, exe           the installer's size and the app's, in whole MB
//   version, facts_measured  the version and the date of the timings and sizes
//   measured                 the comparison's "Measured on <date>", naming PoE2 Oracle's date
//                            apart once it differs from the others'
//   lower                    where another app needs less than PoE2 Oracle, a sentence; empty
//                            when none does
//
// `window.benchReady` settles once every one is filled.

import { h } from "./dom.js";

const lang = document.documentElement.lang === "ru" ? "ru" : "en";
const locale = lang === "ru" ? "ru-RU" : "en-GB";
const nbsp = "\u00a0";

const words = {
    en: {
        caption: "Processes, memory, idle CPU and GPU memory of PoE2 Oracle and other price checkers",
        app: "App",
        memory: "Memory in Task Manager",
        processes: "Processes",
        cpu: "Idle CPU, % of a core",
        cpuCard: "Idle CPU",
        gpu: "GPU memory",
        units: { mb: "MB", s: "s", ms: "ms" },
        percent: (number) => `${number}%`,
        times: (number) => `${number}× as much`,
        measured: (date) => `Measured on ${date}`,
        measuredApart: (ours, others) => `PoE2 Oracle measured on ${ours}, the others on ${others}`,
        date: (dayMonth, year) => `${dayMonth} ${year}`,
        lower: (parts) => `Where others need less than PoE2 Oracle: ${parts}.`,
        metrics: { memory: "memory", processes: "processes", cpu: "idle CPU", gpu: "GPU memory" },
    },
    ru: {
        caption: "Процессы, память, процессор в простое и видеопамять PoE2 Oracle и других программ проверки цен",
        app: "Программа",
        memory: "Память в диспетчере задач",
        processes: "Процессы",
        cpu: "ЦП в простое, % ядра",
        cpuCard: "ЦП в простое",
        gpu: "Видеопамять",
        units: { mb: "МБ", s: "с", ms: "мс" },
        percent: (number) => `${number}${nbsp}%`,
        // «в 3,7 раза», «в 4 раза», «в 7 раз»: a fraction and 2-4 take «раза».
        times: (number, whole) => {
            const form = whole === null ? "other" : new Intl.PluralRules("ru").select(whole);
            return `в ${number} ${form === "few" || form === "other" ? "раза" : "раз"} больше`;
        },
        measured: (date) => `Замер ${date}`,
        measuredApart: (ours, others) => `PoE2 Oracle мерили ${ours}, остальные программы — ${others}`,
        date: (dayMonth, year) => `${dayMonth} ${year} года`,
        lower: (parts) => `Где другим нужно меньше, чем PoE2 Oracle: ${parts}.`,
        metrics: { memory: "память", processes: "процессы", cpu: "процессор в простое", gpu: "видеопамять" },
    },
}[lang];

const format = (value, digits) =>
    new Intl.NumberFormat(locale, {
        minimumFractionDigits: digits,
        maximumFractionDigits: digits,
    }).format(value);

/** A number and its unit, which never part at a line break. */
const unit = (number, name) => `${number}${nbsp}${words.units[name]}`;

const megabytes = (value, digits) => unit(format(value, digits), "mb");

const range = ([low, high], name) => unit(`${format(low, 0)}–${format(high, 0)}`, name);

/** Idle CPU: to two decimals below 10 %, whole above. */
const percent = (value) => words.percent(format(value, value < 10 ? 2 : 0));

const list = (items) => new Intl.ListFormat(locale, { type: "conjunction" }).format(items);

/** Text given once for both languages, or per language. */
const text = (value) => (typeof value === "string" ? value : value[lang]);

/** An ISO date (2026-09-26) as the page's language writes it. */
function date(iso) {
    const day = new Date(`${iso}T00:00:00Z`);
    const dayMonth = new Intl.DateTimeFormat(locale, { day: "numeric", month: "long", timeZone: "UTC" });
    return words.date(dayMonth.format(day), day.getUTCFullYear());
}

/** How many times `ours` `value` is, to one decimal; null when that rounds to 1 or less. */
function times(value, ours) {
    const rounded = Math.round((value / ours) * 10) / 10;
    if (rounded <= 1) return null;
    const whole = Number.isInteger(rounded);
    return words.times(format(rounded, whole ? 0 : 1), whole ? rounded : null);
}

/** PoE2 Oracle first, then the others from the least memory to the most, each by its first state. */
const ordered = (apps) =>
    [...apps].sort((a, b) =>
        a.id === "oracle" ? -1 : b.id === "oracle" ? 1 : a.states[0].memory - b.states[0].memory,
    );

/** The comparison: one row per app and state, in `ordered` order, its bars scaled to the most. */
function chart(apps) {
    const ours = apps.find((app) => app.id === "oracle").states[0];
    const rows = ordered(apps).flatMap((app) =>
        app.states.map((state, index) => ({ app, state, again: index > 0 })),
    );
    const most = Math.max(...rows.map(({ state }) => state.memory));
    // The roles keep the table a table for screen readers where a phone's layout makes its rows
    // blocks: a browser drops the implicit ones with `display`.
    return h(
        "table",
        { class: "bench-table", role: "table" },
        h("caption", null, words.caption),
        h(
            "thead",
            { role: "rowgroup" },
            h(
                "tr",
                { role: "row" },
                [words.app, words.memory, words.processes, words.cpu, words.gpu].map((label) =>
                    h("th", { scope: "col", role: "columnheader" }, label),
                ),
            ),
        ),
        h(
            "tbody",
            { role: "rowgroup" },
            rows.map((row) => line(row, ours, most)),
        ),
    );
}

/** An app's row in one of its states; a second state of the same app repeats only its name. */
function line({ app, state, again }, ours, most) {
    const mine = app.id === "oracle";
    const ratio = mine ? null : times(state.memory, ours.memory);
    return h(
        "tr",
        {
            class: ["bench-row", mine && "bench-ours", again && "bench-again"].filter(Boolean).join(" "),
            role: "row",
        },
        h(
            "th",
            { scope: "row", role: "rowheader", class: "bench-app" },
            h("span", "bench-name", app.name, !again && h("span", "bench-version", ` ${app.version}`)),
            !again && h("span", "bench-stack", text(app.stack)),
            state.label && h("span", "bench-state", text(state.label)),
        ),
        h(
            "td",
            { role: "cell", class: "bench-memory", "data-label": words.memory },
            h(
                "span",
                "bench-track",
                h("span", { class: "bench-bar", "--share": (state.memory / most).toFixed(4) }),
                h("span", "bench-value", megabytes(state.memory, 1)),
            ),
            ratio && h("span", "bench-ratio", ratio),
        ),
        h(
            "td",
            { role: "cell", class: "bench-processes", "data-label": words.processes },
            h(
                "span",
                { class: "bench-pips", "aria-hidden": "true" },
                Array.from({ length: state.processes }, () => h("i")),
            ),
            format(state.processes, 0),
        ),
        h("td", { role: "cell", class: "bench-number", "data-label": words.cpuCard }, percent(state.cpu)),
        h("td", { role: "cell", class: "bench-number", "data-label": words.gpu }, megabytes(state.gpu, 1)),
    );
}

/** Each metric of the comparison where another app needs less than PoE2 Oracle, with the apps,
 * the one that needs least first. */
function lower(apps) {
    const ours = apps.find((app) => app.id === "oracle").states[0];
    const parts = Object.entries(words.metrics).flatMap(([metric, name]) => {
        const names = apps
            .filter((app) => app.id !== "oracle")
            .flatMap((app) => {
                const below = app.states.filter((state) => state[metric] < ours[metric]);
                if (below.length === 0) return [];
                if (below.length === app.states.length) {
                    return [{ name: app.name, least: Math.min(...below.map((state) => state[metric])) }];
                }
                return below.map((state) => ({ name: `${app.name}, ${text(state.label)}`, least: state[metric] }));
            })
            .sort((a, b) => a.least - b.least)
            .map(({ name }) => name);
        return names.length ? [`${name} (${list(names)})`] : [];
    });
    return parts.length ? words.lower(list(parts)) : "";
}

function values(apps, facts) {
    const ours = apps.find((app) => app.id === "oracle");
    const others = [...new Set(apps.filter((app) => app !== ours).map((app) => app.measured))];
    const measured =
        others.length === 1 && others[0] === ours.measured
            ? words.measured(date(ours.measured))
            : words.measuredApart(date(ours.measured), list(others.map(date)));
    return {
        memory: megabytes(ours.states[0].memory, 1),
        processes: format(ours.states[0].processes, 0),
        ready: unit(format(facts.ready_s, 2), "s"),
        item_text: unit(format(facts.item_text_ms, 0), "ms"),
        processing: unit(format(facts.processing_ms, 0), "ms"),
        exchange: range(facts.exchange_range_ms, "ms"),
        listings: unit(format(facts.listings_s, 2), "s"),
        listings_trade_site: unit(format(facts.listings_trade_site_s, 2), "s"),
        installer: megabytes(facts.installer_bytes / 2 ** 20, 0),
        exe: megabytes(facts.exe_bytes / 2 ** 20, 0),
        version: facts.version,
        facts_measured: date(facts.measured),
        measured,
        lower: lower(apps),
    };
}

async function draw() {
    const answer = await fetch(new URL("data/bench.json", import.meta.url));
    if (!answer.ok) throw new Error(`bench.json: HTTP ${answer.status}`);
    const { apps, facts } = await answer.json();
    const filled = values(apps, facts);
    for (const slot of document.querySelectorAll("[data-bench]")) {
        const value = filled[slot.dataset.bench];
        if (value === undefined) console.error(`bench: no value named "${slot.dataset.bench}"`);
        else slot.textContent = value;
    }
    for (const place of document.querySelectorAll("[data-bench-chart]")) {
        place.replaceChildren(chart(apps));
    }
}

window.benchReady = draw().catch((error) => {
    console.error("bench:", error);
});
