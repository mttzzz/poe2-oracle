// The site's report form (report.html and ru/report.html). It sends the service the same JSON
// report the app's window does (crates/oracle-protocol: `kind` bug or idea, `source` "site") and
// says how it went, in the page's own words: they live on the form's data-* attributes, so both
// languages share this script. Without JavaScript the form stays hidden and a <noscript> note
// says why.

const ENDPOINT = "/api/v1/reports";
// Report::check's limits: the text, trimmed, is at most 8000 characters, the contact 200.
const MAX_TEXT = 8000;
const MAX_CONTACT = 200;

const form = document.getElementById("report");
const sent = document.getElementById("report-sent");
const words = form.dataset;
const { text, contact, website } = form.elements;
const send = form.querySelector("button[type=submit]");
const counter = document.getElementById("text-count");
let busy = false;
const status = form.querySelector(".status");

function fill(template, values) {
    return template.replace(/\{(\w+)\}/g, (whole, key) => values[key] ?? whole);
}

/** Characters as the service counts them: Unicode scalar values, not UTF-16 units. */
function length(value) {
    return [...value].length;
}

function valid() {
    const body = text.value.trim();
    return body !== "" && length(body) <= MAX_TEXT && length(contact.value.trim()) <= MAX_CONTACT;
}

function refresh() {
    counter.textContent = `${text.value.length} / ${MAX_TEXT}`;
    send.disabled = busy || !valid();
}

function kindChanged() {
    text.placeholder = text.dataset[`placeholder${form.elements.kind.value === "idea" ? "Idea" : "Bug"}`];
}

/** Why the service refused the report, in the app window's words and by its rules. */
function reason(answer) {
    switch (answer.status) {
        case 400:
            return words.invalid;
        case 413:
            return words.tooLarge;
        case 429: {
            // Seconds; without them, an hour, as the app assumes.
            const seconds = Number.parseInt(answer.headers.get("Retry-After") ?? "", 10);
            const minutes = Number.isFinite(seconds) ? Math.max(1, Math.ceil(seconds / 60)) : 60;
            return fill(words.rateLimited, { minutes });
        }
        default:
            return words.unavailable;
    }
}

function settle(message) {
    busy = false;
    send.textContent = words.send;
    status.textContent = message ? fill(words.failed, { reason: message }) : "";
    status.classList.toggle("error", Boolean(message));
    refresh();
}

form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (busy || !valid()) return;
    busy = true;
    send.disabled = true;
    send.textContent = words.sending;
    status.textContent = "";
    const report = {
        kind: form.elements.kind.value,
        source: "site",
        text: text.value.trim(),
        website: website.value,
    };
    const reply = contact.value.trim();
    if (reply) report.contact = reply;

    let answer;
    try {
        answer = await fetch(ENDPOINT, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(report),
        });
    } catch {
        settle(fill(words.noConnection, { host: location.host }));
        return;
    }
    if (!answer.ok) {
        settle(reason(answer));
        return;
    }
    const { id } = await answer.json().catch(() => ({ id: 0 }));
    settle("");
    form.hidden = true;
    sent.hidden = false;
    sent.querySelector(".status").textContent = id ? fill(words.sent, { id }) : words.sentNoId;
    sent.querySelector("button").focus();
});

sent.querySelector("button").addEventListener("click", () => {
    form.reset();
    kindChanged();
    refresh();
    sent.hidden = true;
    form.hidden = false;
    text.focus();
});

for (const radio of form.elements.kind) radio.addEventListener("change", kindChanged);
text.addEventListener("input", refresh);
contact.addEventListener("input", refresh);

kindChanged();
refresh();
form.hidden = false;
