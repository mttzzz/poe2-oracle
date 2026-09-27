// PoE2 Oracle next to other price checkers, and its own speed and size, all from data/bench.json:
// the one file that holds every such number the landing pages show, so that a new measurement
// changes that file and nothing else. Each app there has what was measured of it in two states of
// the game: `play`, the game in front, its CPU with the mouse moving and at rest; `background`, the
// game in the background, its processes, memory, idle CPU and GPU memory, in one state of the app
// or more. [data-bench-chart="play"] and [data-bench-chart="background"] each get that comparison,
// a table whose first measured column is a bar chart; each [data-bench="<name>"] gets one value, in
// the page's language:
//
//   memory, processes        PoE2 Oracle's memory with the game in the background, in MB to one
//                            decimal as Task Manager shows it, and its process count
//   play_moving              PoE2 Oracle's CPU in the game with the mouse moving
//   play_summary             how PoE2 Oracle's CPU in the game compares with the others', with the
//                            mouse moving and at rest, a sentence each: the least, about even with
//                            the apps within one step of the table's last digit, or more than those
//                            below that
//   play_measured,           each comparison's "Measured on <date>", naming PoE2 Oracle's date
//   background_measured      apart once it differs from the others'
//   background_lower         where the background comparison shows another app needing less than
//                            PoE2 Oracle, a sentence; empty when none does
//   ready                    from launch to ready to price
//   item_text, processing    a price check: the item's text after the copy, then reading it,
//                            its filters and the panel
//   exchange                 an exchange item's price, or a repeated search's, from loaded data
//   listings,                a search with listings, and the part of it pathofexile.com took
//   listings_trade_site
//   installer, exe           the installer's size and the app's, in whole MB
//   version, facts_measured  the version and the date of the timings and sizes
//
// Values compare as the tables round them, so a difference the tables can't show isn't claimed
// either way. `window.benchReady` settles once every one is filled.

import { h } from "./dom.js";

const lang = document.documentElement.lang === "ru" ? "ru" : "en";
const locale = lang === "ru" ? "ru-RU" : "en-GB";
const nbsp = "\u00a0";

const words = {
    en: {
        app: "App",
        units: { mb: "MB", s: "s", ms: "ms" },
        percent: (number) => `${number}%`,
        percentRange: (low, high) => `${low}–${high}%`,
        times: (number) => `${number}× as much`,
        measured: (date) => `Measured on ${date}`,
        measuredApart: (ours, others) => `PoE2 Oracle measured on ${ours}, the others on ${others}`,
        date: (dayMonth, year) => `${dayMonth} ${year}`,
        play: {
            caption: "CPU of PoE2 Oracle and other price checkers with the game in front, with the mouse moving and at rest",
            moving: "CPU with the mouse moving, % of a core",
            movingCard: "CPU, mouse moving",
            still: "Mouse at rest",
            stillCard: "CPU, mouse at rest",
            when: { moving: "With the mouse moving", still: "With the mouse at rest" },
            least: (when, ours, others) =>
                `${when}, PoE2 Oracle used the least CPU: ${ours} of a core, against ${others} for the others.`,
            even: (when, names, values) => `${when}, ${names} were about even: ${values} of a core.`,
            lower: (when, names, values, ours) =>
                `${when}, ${names} used less CPU than PoE2 Oracle: ${values} of a core, against ${ours}.`,
        },
        background: {
            caption: "Processes, memory, idle CPU and GPU memory of PoE2 Oracle and other price checkers with the game in the background",
            memory: "Memory in Task Manager",
            processes: "Processes",
            cpu: "Idle CPU, % of a core",
            cpuCard: "Idle CPU",
            gpu: "GPU memory",
            lower: (parts) => `Where others need less than PoE2 Oracle: ${parts}.`,
            metrics: { memory: "memory", processes: "processes", cpu: "idle CPU", gpu: "GPU memory" },
        },
    },
    ru: {
        app: "Программа",
        units: { mb: "МБ", s: "с", ms: "мс" },
        percent: (number) => `${number}${nbsp}%`,
        percentRange: (low, high) => `${low}–${high}${nbsp}%`,
        // «в 3,7 раза», «в 4 раза», «в 7 раз»: a fraction and 2-4 take «раза».
        times: (number, whole) => {
            const form = whole === null ? "other" : new Intl.PluralRules("ru").select(whole);
            return `в ${number} ${form === "few" || form === "other" ? "раза" : "раз"} больше`;
        },
        measured: (date) => `Замер ${date}`,
        measuredApart: (ours, others) => `PoE2 Oracle мерили ${ours}, остальные программы — ${others}`,
        date: (dayMonth, year) => `${dayMonth} ${year} года`,
        play: {
            caption: "Процессор PoE2 Oracle и других программ проверки цен с игрой на переднем плане — пока мышь двигается и пока стоит",
            moving: "ЦП, пока мышь двигается, % ядра",
            movingCard: "ЦП, мышь двигается",
            still: "Мышь стоит",
            stillCard: "ЦП, мышь стоит",
            when: { moving: "Пока мышь двигается", still: "Пока мышь стоит" },
            least: (when, ours, others) =>
                `${when}, процессора меньше всех нужно PoE2 Oracle: ${ours} ядра против ${others} у остальных.`,
            even: (when, names, values) => `${when}, ${names} примерно вровень: ${values} ядра.`,
            lower: (when, names, values, ours) =>
                `${when}, меньше процессора, чем PoE2 Oracle, нужно ${names}: ${values} ядра против ${ours}.`,
        },
        background: {
            caption: "Процессы, память, процессор в простое и видеопамять PoE2 Oracle и других программ проверки цен, пока игра в фоне",
            memory: "Память в диспетчере задач",
            processes: "Процессы",
            cpu: "ЦП в простое, % ядра",
            cpuCard: "ЦП в простое",
            gpu: "Видеопамять",
            lower: (parts) => `Где другим нужно меньше, чем PoE2 Oracle: ${parts}.`,
            metrics: { memory: "память", processes: "процессы", cpu: "процессор в простое", gpu: "видеопамять" },
        },
    },
}[lang];

const format = (value, digits) =>
    new Intl.NumberFormat(locale, {
        minimumFractionDigits: digits,
        maximumFractionDigits: digits,
    }).format(value);

/** A number and its unit, which never part at a line break. */
const unit = (number, name) => `${number}${nbsp}${words.units[name]}`;

/** How many decimals the tables show of each metric's `value`: memory and GPU memory to one, CPU
 * to two below 10 % and whole from there, processes whole. */
const places = {
    processes: () => 0,
    memory: () => 1,
    cpu: (value) => (value < 10 ? 2 : 0),
    gpu: () => 1,
};

/** `metric`'s `value` rounded as the tables show it, by the same rounding as `format`'s. */
const shown = (metric, value) => {
    const digits = places[metric](value);
    return Number(
        new Intl.NumberFormat("en-US", {
            minimumFractionDigits: digits,
            maximumFractionDigits: digits,
            useGrouping: false,
        }).format(value),
    );
};

const megabytes = (value, digits) => unit(format(value, digits), "mb");

const range = ([low, high], name) => unit(`${format(low, 0)}–${format(high, 0)}`, name);

const percent = (value) => words.percent(format(value, places.cpu(value)));

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

/** PoE2 Oracle first, then the others by `key`, the least first. */
const ordered = (apps, key) =>
    [...apps].sort((a, b) => (a.id === "oracle" ? -1 : b.id === "oracle" ? 1 : key(a) - key(b)));

/** A comparison's table, `kind` naming it for the style sheet. The roles keep it a table for screen
 * readers where a phone's layout makes its rows blocks: a browser drops the implicit ones with
 * `display`. */
function table(kind, caption, headings, rows) {
    return h(
        "table",
        { class: `bench-table bench-table--${kind}`, role: "table" },
        h("caption", null, caption),
        h(
            "thead",
            { role: "rowgroup" },
            h(
                "tr",
                { role: "row" },
                headings.map((label) => h("th", { scope: "col", role: "columnheader" }, label)),
            ),
        ),
        h("tbody", { role: "rowgroup" }, rows),
    );
}

/** A row: PoE2 Oracle's stands out, and a second state of the same app stays with its first. */
const row = (mine, again, ...cells) =>
    h(
        "tr",
        {
            class: ["bench-row", mine && "bench-ours", again && "bench-again"].filter(Boolean).join(" "),
            role: "row",
        },
        cells,
    );

/** The app's name, version and what it is built on, and the app's own state when it has one; a
 * second state of the same app repeats only its name. */
const appCell = (app, state, again) =>
    h(
        "th",
        { scope: "row", role: "rowheader", class: "bench-app" },
        h("span", "bench-name", app.name, !again && h("span", "bench-version", ` ${app.version}`)),
        !again && h("span", "bench-stack", text(app.stack)),
        state?.label && h("span", "bench-state", text(state.label)),
    );

/** A bar `share` of the column's most long, its value beside it, and under it how many times
 * PoE2 Oracle's the value is, if more. */
const barCell = (label, share, value, ratio) =>
    h(
        "td",
        { role: "cell", class: "bench-bars", "data-label": label },
        h(
            "span",
            "bench-track",
            h("span", { class: "bench-bar", "--share": share.toFixed(4) }),
            h("span", "bench-value", value),
        ),
        ratio && h("span", "bench-ratio", ratio),
    );

const processesCell = (label, count) =>
    h(
        "td",
        { role: "cell", class: "bench-processes", "data-label": label },
        h(
            "span",
            { class: "bench-pips", "aria-hidden": "true" },
            Array.from({ length: count }, () => h("i")),
        ),
        format(count, places.processes()),
    );

const numberCell = (label, value) => h("td", { role: "cell", class: "bench-number", "data-label": label }, value);

/** CPU in the game: one row per app, the least CPU with the mouse moving first, its bars scaled to
 * the most. */
function playChart(apps) {
    const entries = ordered(
        apps.filter((app) => app.play),
        (app) => app.play.moving,
    );
    const ours = entries.find((app) => app.id === "oracle").play;
    const most = Math.max(...entries.map((app) => app.play.moving));
    const w = words.play;
    return table(
        "play",
        w.caption,
        [words.app, w.moving, w.still],
        entries.map((app) => {
            const mine = app.id === "oracle";
            return row(
                mine,
                false,
                appCell(app, null, false),
                barCell(
                    w.movingCard,
                    app.play.moving / most,
                    percent(app.play.moving),
                    mine ? null : times(app.play.moving, ours.moving),
                ),
                numberCell(w.stillCard, percent(app.play.still)),
            );
        }),
    );
}

/** The game in the background: one row per app and state, the least memory first, its bars scaled
 * to the most. */
function backgroundChart(apps) {
    const entries = ordered(
        apps.filter((app) => app.background),
        (app) => app.background.states[0].memory,
    );
    const ours = entries.find((app) => app.id === "oracle").background.states[0];
    const rows = entries.flatMap((app) =>
        app.background.states.map((state, index) => ({ app, state, again: index > 0 })),
    );
    const most = Math.max(...rows.map(({ state }) => state.memory));
    const w = words.background;
    return table(
        "background",
        w.caption,
        [words.app, w.memory, w.processes, w.cpu, w.gpu],
        rows.map(({ app, state, again }) => {
            const mine = app.id === "oracle";
            return row(
                mine,
                again,
                appCell(app, state, again),
                barCell(
                    w.memory,
                    state.memory / most,
                    megabytes(state.memory, places.memory()),
                    mine ? null : times(state.memory, ours.memory),
                ),
                processesCell(w.processes, state.processes),
                numberCell(w.cpuCard, percent(state.cpu)),
                numberCell(w.gpu, megabytes(state.gpu, places.gpu())),
            );
        }),
    );
}

const charts = { play: playChart, background: backgroundChart };

/** How PoE2 Oracle's CPU in the game with the mouse `metric` ("moving" or "still") compares with
 * `others`', as the table shows them: the apps more than a step of its last digit below it, else
 * the apps within a step as about even with it, else PoE2 Oracle as the least. */
function comparePlay(metric, ours, others) {
    const hundredths = (value) => Math.round(shown("cpu", value) * 100);
    const step = places.cpu(ours.play[metric]) === 2 ? 1 : 100;
    const mine = hundredths(ours.play[metric]);
    const least = (a, b) => a.play[metric] - b.play[metric];
    const lower = others.filter((app) => hundredths(app.play[metric]) < mine - step).sort(least);
    const even = others.filter((app) => Math.abs(hundredths(app.play[metric]) - mine) <= step).sort(least);
    const w = words.play;
    const when = w.when[metric];
    const value = (app) => percent(app.play[metric]);
    const names = (apps) => list(apps.map((app) => app.name));
    const values = (apps) => list(apps.map(value));
    if (lower.length) return w.lower(when, names(lower), values(lower), value(ours));
    if (even.length) return w.even(when, names([ours, ...even]), values([ours, ...even]));
    const rest = others.map((app) => app.play[metric]);
    const [low, high] = [Math.min(...rest), Math.max(...rest)];
    const span =
        shown("cpu", low) === shown("cpu", high)
            ? percent(low)
            : words.percentRange(format(low, places.cpu(low)), format(high, places.cpu(high)));
    return w.least(when, value(ours), span);
}

/** Each metric of the background comparison where another app needs less than PoE2 Oracle, as the
 * table shows them (`shown`), with the apps, the one that needs least first. */
function backgroundLower(apps) {
    const entries = apps.filter((app) => app.background);
    const ours = entries.find((app) => app.id === "oracle").background.states[0];
    const w = words.background;
    const parts = Object.entries(w.metrics).flatMap(([metric, name]) => {
        const names = entries
            .filter((app) => app.id !== "oracle")
            .flatMap((app) => {
                const states = app.background.states;
                const below = states.filter((state) => shown(metric, state[metric]) < shown(metric, ours[metric]));
                if (below.length === 0) return [];
                if (below.length === states.length) {
                    return [{ name: app.name, least: Math.min(...below.map((state) => state[metric])) }];
                }
                return below.map((state) => ({ name: `${app.name}, ${text(state.label)}`, least: state[metric] }));
            })
            .sort((a, b) => a.least - b.least)
            .map(({ name }) => name);
        return names.length ? [`${name} (${list(names)})`] : [];
    });
    return parts.length ? w.lower(list(parts)) : "";
}

/** A comparison's "Measured on <date>" over `entries`, `of` giving an app's date; PoE2 Oracle's is
 * named apart once it differs from the others'. */
function measured(entries, of) {
    const ours = of(entries.find((app) => app.id === "oracle"));
    const others = [...new Set(entries.filter((app) => app.id !== "oracle").map(of))];
    return others.length === 1 && others[0] === ours
        ? words.measured(date(ours))
        : words.measuredApart(date(ours), list(others.map(date)));
}

function values(apps, facts) {
    const oracle = apps.find((app) => app.id === "oracle");
    const playing = apps.filter((app) => app.play);
    const background = apps.filter((app) => app.background);
    const others = playing.filter((app) => app !== oracle);
    return {
        memory: megabytes(oracle.background.states[0].memory, places.memory()),
        processes: format(oracle.background.states[0].processes, places.processes()),
        play_moving: percent(oracle.play.moving),
        play_summary: ["moving", "still"].map((metric) => comparePlay(metric, oracle, others)).join(" "),
        play_measured: measured(playing, (app) => app.play.measured),
        background_measured: measured(background, (app) => app.background.measured),
        background_lower: backgroundLower(apps),
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
        const chart = charts[place.dataset.benchChart];
        if (chart) place.replaceChildren(chart(apps));
        else console.error(`bench: no chart named "${place.dataset.benchChart}"`);
    }
}

window.benchReady = draw().catch((error) => {
    console.error("bench:", error);
});
