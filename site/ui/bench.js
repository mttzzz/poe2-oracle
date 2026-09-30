// PoE2 Oracle next to other price checkers, and its own speed and size, all from data/bench.json:
// the one file that holds every such number the landing pages show, so that a new measurement
// changes that file and nothing else. Each app there has what was measured of it two ways:
// `background`, its memory and processes a few minutes after launch, while it waits for a price
// check, in one state of the app or more; `play`, its CPU in each minute of real play, with all four
// apps running. [data-bench-chart="compare"] gets the comparison of the apps measured both ways, a
// table whose memory and CPU are bar charts; each [data-bench="<name>"] gets one value, in the
// page's language:
//
//   memory, processes       PoE2 Oracle's memory after launch, in MB to one decimal as Task Manager
//                           shows it, and its process count
//   memory_less, cpu_less   how many times less memory after launch, and CPU while playing, PoE2
//                           Oracle needs than the others: whole times, the least to the most
//   others_processes        the others' process counts, the least to the most
//   measured                the days of the comparison
//   ready                   from launch to ready to price
//   panel, listings         from the key press to the panel, and to a search's listings
//   installer               the installer's size, in whole MB
//   facts_measured          the days of those times
//
// An app measured in several states is shown in the one that needs least memory, with the most
// beside it. The table rounds ratios to one decimal and the claims to whole times; a claim the
// numbers don't bear out is left empty and said in the console. PoE2 Oracle's row names its latest
// release (release.js), and the version measured only when the service names none: the owner's
// call, 2026-09-30. `window.benchReady` settles once every value is filled.

import { h } from "./dom.js";
import { latestVersion } from "./release.js";

const lang = document.documentElement.lang === "ru" ? "ru" : "en";
const locale = lang === "ru" ? "ru-RU" : "en-GB";
const nbsp = "\u00a0";

/** «раз» or «раза» after a number of times: a fraction and 2-4 take «раза» («в 3,4 раза», «в 4
 * раза»), the rest «раз» («в 5 раз», «в 21 раз»). `whole` is the number when it is whole. */
const timesWord = (whole) => {
    const form = whole === null ? "other" : new Intl.PluralRules("ru").select(whole);
    return form === "few" || form === "other" ? "раза" : "раз";
};

const words = {
    en: {
        app: "App",
        caption: "Memory after launch, CPU load while playing and processes of PoE2 Oracle and other price checkers",
        memory: "Memory after launch",
        cpu: "CPU load while playing",
        processes: "Processes",
        units: { mb: "MB", s: "s" },
        times: (number) => `${number}× as much`,
        less: (span) => `${span}×`,
        least: "the least",
        notMore: "no more than PoE2 Oracle's",
        upTo: (value, state) => `up to ${value} ${state}`,
        date: (dayMonth, year) => `${dayMonth} ${year}`,
    },
    ru: {
        app: "Программа",
        caption: "Память после запуска, нагрузка на процессор во время игры и процессы PoE2 Oracle и других программ проверки цен",
        memory: "Память после запуска",
        cpu: "Нагрузка на процессор в игре",
        processes: "Процессы",
        units: { mb: "МБ", s: "с" },
        times: (number, whole) => `в ${number} ${timesWord(whole)} больше`,
        less: (span, high) => `в ${span} ${timesWord(high)}`,
        least: "меньше всех",
        notMore: "не больше, чем у PoE2 Oracle",
        upTo: (value, state) => `${state} — до ${value}`,
        date: (dayMonth, year) => `${dayMonth} ${year} года`,
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

/** The least and the most of `values` as the page writes them, or one value when they're the same. */
const span = (values, digits) => {
    const [low, high] = [Math.min(...values), Math.max(...values)].map((value) => format(value, digits));
    return low === high ? low : `${low}–${high}`;
};

/** Milliseconds from `low` to `high`, in seconds: to one decimal where that keeps the ends above
 * zero and apart, else to two. */
function seconds([low, high]) {
    const oneDecimal = Math.round(low / 100) > 0 && Math.round(low / 100) !== Math.round(high / 100);
    return unit(span([low / 1000, high / 1000], oneDecimal ? 1 : 2), "s");
}

/** Text given once for both languages, or per language. */
const text = (value) => (typeof value === "string" ? value : value[lang]);

const median = (values) => {
    const sorted = [...values].sort((a, b) => a - b);
    const middle = sorted.length >> 1;
    return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};

/** An app's CPU while playing: its median minute. */
const playCpu = (app) => median(app.play.minutes);

/** The app's state after launch it is shown in, the one that needs least memory, and the one that
 * needs most. */
const launch = (app) => app.background.states.reduce((a, b) => (b.memory < a.memory ? b : a));
const heaviest = (app) => app.background.states.reduce((a, b) => (b.memory > a.memory ? b : a));

/** The days of `isos` (ISO dates), the first to the last, as the page's language writes them. */
function period(isos) {
    const days = [...new Set(isos)].sort().map((iso) => new Date(`${iso}T00:00:00Z`));
    const [first, last] = [days[0], days[days.length - 1]];
    const dayMonth = new Intl.DateTimeFormat(locale, { day: "numeric", month: "long", timeZone: "UTC" });
    const year = (day) => day.getUTCFullYear();
    if (days.length === 1) return words.date(dayMonth.format(first), year(first));
    if (year(first) !== year(last)) {
        return `${words.date(dayMonth.format(first), year(first))} – ${words.date(dayMonth.format(last), year(last))}`;
    }
    const from = first.getUTCMonth() === last.getUTCMonth() ? `${first.getUTCDate()}–` : `${dayMonth.format(first)} – `;
    return words.date(`${from}${dayMonth.format(last)}`, year(last));
}

/** How many times `ours` `value` is, to one decimal; null when that rounds to 1 or less. */
function times(value, ours) {
    const rounded = Math.round((value / ours) * 10) / 10;
    if (rounded <= 1) return null;
    const whole = Number.isInteger(rounded);
    return words.times(format(rounded, whole ? 0 : 1), whole ? rounded : null);
}

/** The apps measured both ways: PoE2 Oracle first, then the others from the least memory after
 * launch to the most. */
function compared(apps) {
    const measured = apps.filter((app) => app.background && app.play);
    const ours = measured.find((app) => app.id === "oracle");
    if (!ours) throw new Error("PoE2 Oracle isn't measured both ways");
    const others = measured.filter((app) => app !== ours);
    return [ours, ...others.sort((a, b) => launch(a).memory - launch(b).memory)];
}

/** The comparison's table. The roles keep it a table for screen readers where a phone's layout
 * makes its rows blocks: a browser drops the implicit ones with `display`. */
function table(caption, headings, rows) {
    return h(
        "table",
        { class: "bench-table", role: "table" },
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

const appCell = (app) =>
    h(
        "th",
        { scope: "row", role: "rowheader", class: "bench-app" },
        h("span", "bench-name", app.name, h("span", "bench-version", ` ${app.version}`)),
    );

/** A bar `share` of the column's most long with its value beside it, and under them how many times
 * PoE2 Oracle's the value is and the app's states, a line each, where they are known. */
const barCell = (kind, label, share, value, ratio, states = []) =>
    h(
        "td",
        { role: "cell", class: `bench-bars bench-${kind}`, "data-label": label },
        h(
            "span",
            "bench-track",
            h("span", { class: "bench-bar", "--share": share.toFixed(4) }),
            value && h("span", "bench-value", value),
        ),
        ratio && h("span", "bench-ratio", ratio),
        states.map((state) => h("span", "bench-state", state)),
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
        format(count, 0),
    );

/** One row per app: its memory after launch, the bars scaled to the most; its CPU while playing as
 * how many times PoE2 Oracle's it is, the bars scaled the same way; and its processes. */
function compareChart(apps) {
    const entries = compared(apps);
    const [ours, ...others] = entries;
    const oursMemory = launch(ours).memory;
    const oursCpu = playCpu(ours);
    const mostMemory = Math.max(...entries.map((app) => launch(app).memory));
    const mostCpu = Math.max(...entries.map(playCpu));
    const leastCpu = others.every((app) => times(playCpu(app), oursCpu) !== null);
    return table(
        words.caption,
        [words.app, words.memory, words.cpu, words.processes],
        entries.map((app) => {
            const mine = app === ours;
            const state = launch(app);
            const most = heaviest(app);
            const cpu = playCpu(app);
            return h(
                "tr",
                { class: mine ? "bench-row bench-ours" : "bench-row", role: "row" },
                appCell(app),
                barCell(
                    "memory",
                    words.memory,
                    state.memory / mostMemory,
                    megabytes(state.memory, 1),
                    mine ? null : times(state.memory, oursMemory),
                    most === state
                        ? []
                        : [text(state.label), words.upTo(megabytes(most.memory, 0), text(most.label))],
                ),
                barCell(
                    "cpu",
                    words.cpu,
                    cpu / mostCpu,
                    mine ? leastCpu && words.least : (times(cpu, oursCpu) ?? words.notMore),
                    null,
                    app.play.label ? [text(app.play.label)] : [],
                ),
                processesCell(words.processes, state.processes),
            );
        }),
    );
}

const charts = { compare: compareChart };

/** How many times less PoE2 Oracle needs of `what` than the others, `ratios` being theirs over
 * its: whole times, the least to the most. Empty, and said in the console, when one of them is under
 * twice: the claim would need other words. */
function less(what, ratios) {
    if (ratios.some((ratio) => ratio < 2)) {
        console.error(`bench: another app needs less than twice PoE2 Oracle's ${what}; reword the claim`);
        return "";
    }
    const whole = ratios.map(Math.round);
    return words.less(span(whole, 0), Math.max(...whole));
}

/** The others' process counts, the least to the most; empty, and said in the console, unless each
 * runs more processes than PoE2 Oracle. */
function othersProcesses(ours, others) {
    const counts = others.map((app) => launch(app).processes);
    if (counts.some((count) => count <= ours)) {
        console.error("bench: another app runs no more processes than PoE2 Oracle; reword the claim");
        return "";
    }
    return span(counts, 0);
}

function values(apps, facts) {
    const entries = compared(apps);
    const [ours, ...others] = entries;
    const state = launch(ours);
    return {
        memory: megabytes(state.memory, 1),
        processes: format(state.processes, 0),
        memory_less: less(
            "memory after launch",
            others.map((app) => launch(app).memory / state.memory),
        ),
        cpu_less: less(
            "CPU while playing",
            others.map((app) => playCpu(app) / playCpu(ours)),
        ),
        others_processes: othersProcesses(state.processes, others),
        measured: period(entries.flatMap((app) => [app.background.measured, app.play.measured])),
        ready: unit(format(facts.ready_s, 2), "s"),
        panel: seconds(facts.panel_ms),
        listings: seconds(facts.listings_ms),
        installer: megabytes(facts.installer_bytes / 2 ** 20, 0),
        facts_measured: period(facts.measured),
    };
}

async function draw() {
    const [answer, latest] = await Promise.all([
        fetch(new URL("data/bench.json", import.meta.url)),
        latestVersion,
    ]);
    if (!answer.ok) throw new Error(`bench.json: HTTP ${answer.status}`);
    const { apps, facts } = await answer.json();
    const ours = apps.find((app) => app.id === "oracle");
    if (ours && latest) ours.version = latest;
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
