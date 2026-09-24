// The XP overlay's plates (crates/poe2-oracle/src/ui/xp_overlay.rs) where the app sets them on
// the game's HUD: the level plate with its gear on the flask panel's rail, the map plate on the
// skill panel's -- each drawn over a capture of its rail (img/hud-flask.webp, img/hud-skill.webp),
// side by side when both are given, and run on over the gap to its globe's frame as the app runs
// it. A plate reads its words the way the app puts them together (xp_tracker.rs Word): values in
// the HUD's cream, the words saying what they are muted, the rate in its gold, a small diamond
// between the parts. Like the app, a plate says as much as fits it, measured in its own typeface:
// its `words`, else the first of its `shorter` wordings that fits.

import { diamond, h } from "./dom.js";

const HUD = {
    flask: new URL("img/hud-flask.webp", import.meta.url).href,
    skill: new URL("img/hud-skill.webp", import.meta.url).href,
};

// The room a plate leaves its words, in HUD px: its width less its padding at either end
// (xp_overlay.rs room) -- the level plate's without the gear's square.
const ROOM = { flask: 210 - 2 * 7, skill: 234.5 - 2 * 7 };
// The space between a part's words, and the diamond between parts with its gaps (laid_width).
const WORD_GAP = 3;
const PART_GAP = 2 * 5 + 4;
// The plates' typeface: the interface language's game-styled one (fonts.rs interface_font).
const FONTS = { en: '500 12px "Alegreya SC"', ru: '700 12px "Philosopher"' };
// Where a plate meets its globe (overlay_layout.rs LIFE_GLOBE_GAP, MANA_GLOBE_GAP): down the
// plate's 40 rows of a 2160-row game, the run of the world between its outer end and the globe's
// frame, [from, to) pixels out from the end. The app covers exactly that; the tables' one-pixel
// runs over the frames' anti-aliased rims are left out here, where a pixel is half a CSS px.
const GAP = {
    flask: [
        [0, 49], [0, 48], [0, 47], [0, 47], [0, 46], [0, 45], [0, 45], [0, 44], [0, 40], [0, 38],
        [0, 36], [0, 34], [0, 32], [0, 31], [0, 30], [0, 29], [0, 28], [0, 27], [0, 27], [0, 26],
        [0, 25], [0, 25], [0, 24], [0, 24], [0, 16], [0, 14], [0, 13], [0, 12], [0, 10], [0, 8],
        [0, 5], [0, 6], [0, 7], [0, 10], [2, 12], [2, 12], [3, 12], [4, 12],
    ],
    skill: [
        [0, 49], [0, 48], [0, 47], [0, 46], [0, 46], [0, 45], [0, 44], [0, 43], [0, 39], [0, 37],
        [0, 35], [0, 33], [0, 32], [0, 31], [0, 29], [0, 28], [0, 28], [0, 27], [0, 26], [0, 26],
        [0, 25], [0, 24], [0, 24], [0, 24], [0, 15], [0, 13], [0, 12], [0, 11], [0, 10], [0, 8],
        [0, 5], [0, 6], [0, 7], [0, 10], [2, 12], [2, 12], [3, 12], [4, 11],
    ],
};
// The plate's rows, and how far the widest run reaches (the plates' CSS widths add half of it).
const ROWS = 40;
const REACH = 49;

// The clip-path that shows a plate `width` HUD px wide on its rail plus the gap to its globe --
// `rail` "flask" for the life globe on its left, "skill" for the mana globe on its right -- in
// percentages of the element, which is REACH/2 px wider than the plate: along the top to the
// gap's far end, down the frame's side row by row, back up the side of the rail's end cap the
// last rows leave to the game, then down the plate's own end.
function globeClip(rail, width) {
    const runs = GAP[rail];
    const whole = 2 * width + REACH;
    // A distance out from the plate's end, as x from the element's left edge (in 2160-row px).
    const out = rail === "flask" ? (d) => REACH - d : (d) => 2 * width + d;
    const end = out(0);
    const outer = runs.flatMap(([, to], row) => [
        [out(to), row],
        [out(to), row + 1],
    ]);
    const capped = runs.findIndex(([from]) => from > 0);
    const inner = runs
        .slice(capped)
        .map(([from], index) => [out(from), capped + index])
        .reverse()
        .flatMap(([x, row]) => [
            [x, row + 1],
            [x, row],
        ]);
    const far = rail === "flask" ? whole : 0;
    const points = [
        [far, 0],
        ...outer,
        ...inner,
        [end, capped],
        [end, ROWS],
        [far, ROWS],
    ];
    const pct = (value, of) => `${+((100 * value) / of).toFixed(3)}%`;
    return `polygon(${points.map(([x, y]) => `${pct(x, whole)} ${pct(y, ROWS)}`).join(", ")})`;
}

/** Draws `data.plates` (see data/xp.en.json) in `lang`, "en" or "ru". */
export function render(data, lang) {
    const plates = ["flask", "skill"]
        .map((rail) => data.plates.find((plate) => plate.rail === rail))
        .filter(Boolean);
    return h(
        "div",
        "oui-xp",
        plates.map((plate) =>
            h(
                "div",
                `oui-xp-figure oui-xp-figure--${plate.rail}`,
                h("img", { class: "oui-xp-hud", src: HUD[plate.rail], alt: "", decoding: "async" }),
                plate.rail === "flask" ? levelPlate(plate, lang) : mapPlate(plate, lang),
            ),
        ),
    );
}

// The level plate and its gear: one frame over two windows, run on to the life globe on the left,
// a post at the gear's end.
function levelPlate(plate, lang) {
    return [
        h(
            "div",
            { class: plateClass("level", plate), style: { clipPath: globeClip("flask", 210) } },
            words(fit(plate, ROOM.flask, lang)),
        ),
        h(
            "div",
            "oui-xp-plate oui-xp-plate--gear",
            h("span", "oui-xp-divider"),
            gear(),
            h("span", "oui-xp-post oui-xp-post--right"),
        ),
    ];
}

// The map plate: a post at its left end, run on to the mana globe on the right.
function mapPlate(plate, lang) {
    return h(
        "div",
        { class: plateClass("map", plate), style: { clipPath: globeClip("skill", 234.5) } },
        words(fit(plate, ROOM.skill, lang)),
        h("span", "oui-xp-post oui-xp-post--left"),
    );
}

function plateClass(kind, plate) {
    return `oui-xp-plate oui-xp-plate--${kind}${plate.dimmed ? " oui-xp-plate--dim" : ""}`;
}

// The longest wording that fits `room`, the last if none does (xp_overlay.rs Fit::choose), as
// parts of words.
function fit(plate, room, lang) {
    const wordings = [plate.words, ...(plate.shorter ?? [])].map(parts);
    const context = document.createElement("canvas").getContext("2d");
    context.font = FONTS[lang] ?? FONTS.en;
    const width = (wording) =>
        wording.reduce(
            (total, part, index) =>
                total +
                (index > 0 ? PART_GAP : 0) +
                part.reduce(
                    (sum, word, at) => sum + (at > 0 ? WORD_GAP : 0) + context.measureText(text(word)).width,
                    0,
                ),
            0,
        );
    return wordings.find((wording) => width(wording) <= room) ?? wordings.at(-1);
}

// A wording's words split into its parts at each "diamond".
function parts(words) {
    return words.reduce(
        (all, word) => {
            if (word === "diamond") all.push([]);
            else all.at(-1).push(word);
            return all;
        },
        [[]],
    );
}

function text(word) {
    return word === "dot" ? "·" : (word.value ?? word.rate ?? word.label);
}

// Each word in its tone: a value, the rate, or a label -- and the dot between a part's groups,
// which reads as a label.
function words(wording) {
    return h(
        "div",
        "oui-xp-words",
        wording.map((part, index) => [
            index > 0 && diamond(4),
            h(
                "span",
                "oui-xp-part",
                part.map((word) => {
                    const kind = word === "dot" ? "label" : ["value", "rate", "label"].find((key) => key in word);
                    return h("span", `oui-xp-${kind}`, text(word));
                }),
            ),
        ]),
    );
}

// The gear glyph Segoe UI Symbol draws for ⚙, which the app sets there: a ring with eight square
// teeth round a dot, in em hundredths.
function gear() {
    const svg = (tag, attrs) => {
        const element = document.createElementNS("http://www.w3.org/2000/svg", tag);
        for (const [name, value] of Object.entries(attrs)) element.setAttribute(name, value);
        return element;
    };
    const root = svg("svg", { class: "oui-xp-gear", viewBox: "-50 -50 100 100", "aria-hidden": "true" });
    const teeth = svg("g", { fill: "currentColor" });
    for (let tooth = 0; tooth < 8; tooth += 1) {
        teeth.append(svg("rect", { x: -3.5, y: -39.5, width: 7, height: 9.5, transform: `rotate(${tooth * 45})` }));
    }
    root.append(
        svg("circle", { r: 27.75, fill: "none", stroke: "currentColor", "stroke-width": 8 }),
        svg("circle", { r: 6.5, fill: "currentColor" }),
        teeth,
    );
    return root;
}
