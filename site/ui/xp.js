// The XP overlay's plates (crates/poe2-oracle/src/ui/xp_overlay.rs) where the app sets them on
// the game's HUD: the level plate with its gear on the flask panel's rail, the map plate on the
// skill panel's -- each figure a capture of its rail (img/hud-flask.webp, img/hud-skill.webp),
// side by side when both are given, with the app's own pixels of its plate laid over it
// (img/plate-*.png, which crates/poe2-oracle/examples/plate_art.rs draws from the app's code),
// ends and all. A plate reads its words the way the app puts them together (xp_tracker.rs Word):
// values in the HUD's cream, the words saying what they are muted, the rate in its gold, a small
// diamond between the parts. Like the app, a plate says as much as fits it, measured in its own
// typeface: its `words`, else the first of its `shorter` wordings that fits.

import { diamond, h } from "./dom.js";

const HUD = {
    flask: new URL("img/hud-flask.webp", import.meta.url).href,
    skill: new URL("img/hud-skill.webp", import.meta.url).href,
};
const ART = {
    flask: new URL("img/plate-flask.png", import.meta.url).href,
    skill: new URL("img/plate-skill.png", import.meta.url).href,
};

// The room a plate leaves its words, in HUD px: its width less its padding at either end
// (xp_overlay.rs room) -- the level plate's without the gear's square.
const ROOM = { flask: 210 - 2 * 7, skill: 234.5 - 2 * 7 };
// The space between a part's words, and the diamond between parts with its gaps (laid_width).
const WORD_GAP = 3;
const PART_GAP = 2 * 5 + 4;
// The plates' typeface: the interface language's game-styled one (fonts.rs interface_font).
const FONTS = { en: '500 12px "Alegreya SC"', ru: '700 12px "Philosopher"' };

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
                h("img", { class: "oui-xp-art", src: ART[plate.rail], alt: "", decoding: "async" }),
                plate.rail === "flask" ? levelPlate(plate, lang) : mapPlate(plate, lang),
            ),
        ),
    );
}

// The level plate's words, and its gear with the line between them.
function levelPlate(plate, lang) {
    return [
        h("div", plateClass("level", plate), words(fit(plate, ROOM.flask, lang))),
        h("div", "oui-xp-plate oui-xp-plate--gear", h("span", "oui-xp-divider"), gear()),
    ];
}

// The map plate's words.
function mapPlate(plate, lang) {
    return h("div", plateClass("map", plate), words(fit(plate, ROOM.skill, lang)));
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
