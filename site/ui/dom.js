// Builders for the app's widgets (crates/poe2-oracle/src/ui/style.rs), under the same names, as
// DOM for base.css. The components build their drawings from these and their data.

/** An element: `tag` with a class list (a string, "a b") or attributes, then its children. */
export function h(tag, attrs, ...children) {
    const element = document.createElement(tag);
    if (typeof attrs === "string") {
        element.className = attrs;
    } else if (attrs) {
        for (const [name, value] of Object.entries(attrs)) {
            if (value === undefined || value === null || value === false) continue;
            if (name === "class") element.className = value;
            else if (name === "style" && typeof value === "object") Object.assign(element.style, value);
            else if (name.startsWith("--")) element.style.setProperty(name, value);
            else element.setAttribute(name, value === true ? "" : value);
        }
    }
    append(element, children);
    return element;
}

function append(element, children) {
    for (const child of children.flat(Infinity)) {
        if (child === undefined || child === null || child === false) continue;
        element.append(child instanceof Node ? child : String(child));
    }
}

/** A diamond `size` px across. */
export const diamond = (size = 7, color) =>
    h("span", { class: "oui-diamond", "--d": size, style: color ? { background: color } : null });

/** A rule with a diamond at its centre. */
export const ornamentRule = (color) =>
    h("div", { class: "oui-rule", "--rule": color }, diamond(7));

/** A group's heading: a small diamond, the name in capitals, a rule fading to the right. */
export const sectionHeading = (title) => h("div", "oui-heading", diamond(6), h("span", null, title));

/**
 * The game's double gold frame, laid over a surface that is `position: relative`: its lengths
 * round to this page's device pixels (--dp), as the app's round to its screen's.
 */
export const gameFrame = () =>
    h(
        "div",
        {
            class: "oui-frame",
            "aria-hidden": "true",
            "--dp": `${1 / (window.devicePixelRatio || 1)}px`,
        },
        ["tl", "tr", "bl", "br"].map((corner) => h("span", `oui-corner oui-corner--${corner}`)),
    );

/** A card of rows, a hairline between them. */
export const card = (...rows) => h("div", "oui-card", rows);

/** A button: `kind` "primary", "secondary" or "danger"; `small` for a card's tight rows. */
export const button = (label, kind = "secondary", small = false) =>
    h("span", `oui-btn oui-btn--${kind}${small ? " oui-btn--small" : ""}`, label);

/** A select showing `choice`; `compact` is the panel's smaller one. */
export const select = (choice, compact = false) =>
    h("span", `oui-select${compact ? " oui-select--compact" : ""}`, h("span", null, choice));

/** A switch, gold when `on`. */
export const toggleSwitch = (on) => h("span", `oui-switch${on ? " oui-switch--on" : ""}`);

/** A checkbox, gold with a tick when `checked`. */
export const checkbox = (checked) => h("span", `oui-check${checked ? " oui-check--on" : ""}`, "✓");

/** Choices side by side, `picked` (an index) lit gold. */
export const segmented = (options, picked) =>
    h(
        "span",
        "oui-segmented",
        options.map((option, index) => h("span", index === picked ? "oui-picked" : null, option)),
    );

/** A fact chip: `label` dimmed (none for a bare value), then `value` in `color`. */
export const chip = (label, value, color) =>
    h(
        "span",
        "oui-chip",
        label && h("span", "oui-label", label),
        h("span", { style: color ? { color } : null }, value),
    );

/** A chip a press would switch: edged, with a gold ↔. */
export const toggleChip = (label, value) =>
    h("span", "oui-chip oui-chip--toggle", label && h("span", "oui-label", label), h("span", null, value));

/** A chip a press would check: an empty box before `content`. */
export const checkChip = (...content) => h("span", "oui-chip oui-chip--check", content);

/** A key as a keycap. */
export const keycap = (label) => h("span", "oui-keycap", label);

/** A hotkey as keycaps joined by +. */
export const keycaps = (keys) =>
    h(
        "span",
        "oui-keys",
        keys.flatMap((key, index) => (index ? ["+", keycap(key)] : [keycap(key)])),
    );

/** A number with − and + either side. */
export const stepper = (value) => h("span", "oui-stepper", h("span", null, "−"), h("b", null, value), h("span", null, "+"));

/** A text link. */
export const link = (label) => h("span", "oui-link", label);

/** A game icon from `url`, `size` px square. */
export const icon = (url, size = 14, alt = "") =>
    h("img", { class: "oui-icon", src: url, alt, "--i": size, decoding: "async" });
