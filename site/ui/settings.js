// The settings window (crates/poe2-oracle/src/ui/settings_view.rs, 1100 × 720 px) drawn from data:
// the title bar, the sidebar with the seven sections (the current one lit) and the version under
// them, and one section's page -- its title and summary over the ornament rule, then its groups,
// each a heading over a card of rows: a label, the notes under it and a control on the right.
//
// Data (data/settings.<lang>.json):
//   window    the title bar's word after the app's name ("Settings")
//   sections  the sidebar's seven section names; `section` the index of the one shown
//   summary   the shown section's line under its title
//   version   the sidebar's last line ("version {version}")
//   app_version  what {version} in `version` and in a row's label becomes when the service names
//             no release (release.js); the latest release's version when it does
//   groups    [{ title, rows: [{ label, note?, control }] }]
//     note    a line under the label, or several; a line is a string (dim) or
//             { text, tone: "dim" | "text" | "warning" }
//     control one control, or several side by side:
//             { type: "select", value }                a select showing `value`
//             { type: "segmented", options, picked }  choices side by side, `picked` lit
//             { type: "stepper", value, canDecrease?, canIncrease? }   false dims that end
//             { type: "switch", on }
//             { type: "keys", keys, empty? }           a hotkey recorder: keycaps, or `empty`
//             { type: "button", label, kind? }         kind: primary | secondary | danger
//             { type: "field", value?, placeholder?, width? }   a text field, 250 px wide

import {
    button,
    card,
    diamond,
    gameFrame,
    h,
    keycaps,
    ornamentRule,
    sectionHeading,
    segmented,
    select,
    stepper,
    toggleSwitch,
} from "./dom.js";
import { latestVersion } from "./release.js";

const APP_NAME = "PoE2 Oracle";

export async function render(data) {
    const version = (await latestVersion) ?? data.app_version;
    const named = (text) => text.replaceAll("{version}", version);
    const shown = {
        ...data,
        version: named(data.version),
        groups: data.groups.map((group) => ({
            ...group,
            rows: group.rows.map((row) => ({ ...row, label: named(row.label) })),
        })),
    };
    return h(
        "div",
        "oui-settings",
        titleBar(shown.window),
        h("div", "oui-settings-body", sidebar(shown), page(shown)),
        gameFrame(),
    );
}

function titleBar(word) {
    return h(
        "div",
        "oui-settings-titlebar oui-titlebar",
        h(
            "div",
            "oui-settings-title",
            diamond(8),
            h("span", "oui-display oui-settings-app", APP_NAME),
            h("span", "oui-settings-dot", "·"),
            h("span", "oui-display oui-settings-word", word),
        ),
        h("span", "oui-settings-close", "×"),
    );
}

function sidebar(data) {
    return h(
        "div",
        "oui-settings-sidebar",
        h(
            "div",
            "oui-settings-nav",
            h("span", { class: "oui-settings-marker", "--at": data.section }),
            data.sections.map((name, index) => {
                const current = index === data.section ? " oui-settings-current" : "";
                return h("div", `oui-display oui-settings-section${current}`, name);
            }),
        ),
        h(
            "div",
            "oui-settings-about",
            h("div", "oui-display oui-settings-brand", APP_NAME),
            h("div", "oui-settings-version", data.version),
        ),
    );
}

function page(data) {
    return h(
        "div",
        "oui-settings-content",
        h(
            "div",
            "oui-settings-header",
            h("div", "oui-display oui-settings-heading", data.sections[data.section]),
            h("div", "oui-settings-summary", data.summary),
            h("div", "oui-settings-rule", ornamentRule()),
        ),
        h("div", "oui-settings-page", data.groups.map(group)),
    );
}

/** A group: its heading over a card of its rows, an inset hairline between them. */
function group({ title, rows }) {
    return h(
        "div",
        "oui-settings-group",
        sectionHeading(title),
        card(rows.map((row, index) => [index > 0 && h("div", "oui-settings-hairline"), settingRow(row)])),
    );
}

function settingRow({ label, note, control }) {
    const notes = [note ?? []].flat();
    return h(
        "div",
        "oui-settings-row",
        h(
            "div",
            "oui-settings-label",
            h("div", null, label),
            notes.map((line) =>
                typeof line === "string"
                    ? h("div", "oui-settings-note", line)
                    : h("div", `oui-settings-note oui-settings-note--${line.tone || "dim"}`, line.text),
            ),
        ),
        h("div", "oui-settings-control", [control].flat().map(controlFor)),
    );
}

function controlFor(control) {
    switch (control.type) {
        case "select":
            return select(control.value);
        case "segmented":
            return segmented(control.options, control.picked);
        case "stepper": {
            const element = stepper(control.value);
            if (control.canDecrease === false) element.firstChild.classList.add("oui-settings-end");
            if (control.canIncrease === false) element.lastChild.classList.add("oui-settings-end");
            return element;
        }
        case "switch":
            return toggleSwitch(control.on);
        case "keys":
            return h(
                "span",
                "oui-settings-recorder",
                control.keys?.length ? keycaps(control.keys) : h("span", "oui-settings-muted", control.empty),
            );
        case "button":
            return button(control.label, control.kind);
        case "field":
            return h(
                "span",
                { class: "oui-settings-field", "--w": control.width },
                control.value
                    ? h("span", null, control.value)
                    : h("span", "oui-settings-muted", control.placeholder),
            );
        default:
            throw new Error(`settings: no control of type ${control.type}`);
    }
}
