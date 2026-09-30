// The download buttons ([data-download]): what a press on one leads to, the release under them, and
// the bar that brings a button back once the hero's has scrolled away. Every word is the page's.
//
// A press downloads the installer, as the plain link does, and opens the hint for what comes next
// ([data-after-download]) under that button's group ([data-cta]).
//
// [data-release] names the latest version (release.js) once the service names it, and stays hidden
// when it doesn't.

import { latestVersion } from "./release.js";

const after = document.querySelector("[data-after-download]");
const live = document.querySelector("[data-live]");
const bar = document.querySelector("[data-stickybar]");
const buttons = [...document.querySelectorAll("main a[data-download]")];

/** The button whose press opened the hint, for the focus to go back to when the hint closes. */
let opener = null;

/** Says `text` to a screen reader, again when it's what was said last. */
function say(text) {
    live.textContent = "";
    setTimeout(() => {
        live.textContent = text.replace(/\s+/g, " ").trim();
    }, 50);
}

/** Shows the hint under the group of the pressed `button`. */
function open(button) {
    button.closest("[data-cta]").append(after);
    after.hidden = false;
    opener = button;
    showBar();
    say(after.querySelector(".after-title").textContent);
}

function close() {
    const hadFocus = after.contains(document.activeElement);
    after.hidden = true;
    if (hadFocus) opener?.focus();
    showBar();
}

document.addEventListener("click", (event) => {
    const button = event.target.closest("a[data-download]");
    if (button) open(button);
    else if (event.target.closest("[data-after-close]")) close();
});

document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !after.hidden && after.contains(document.activeElement)) close();
});

// The bar: shown once the hero's button is above the screen, unless another of the page's download
// buttons is in view, and while the hint opened from it is open.
const inView = new Set();
let pastHero = false;

function showBar() {
    if (!bar) return;
    const holding = !after.hidden && bar.contains(after);
    bar.classList.toggle("is-shown", holding || (pastHero && inView.size === 0));
}

if (bar && buttons.length) {
    const watch = new IntersectionObserver((entries) => {
        for (const entry of entries) {
            if (entry.isIntersecting) inView.add(entry.target);
            else inView.delete(entry.target);
            if (entry.target === buttons[0]) {
                pastHero = !entry.isIntersecting && entry.boundingClientRect.top < 0;
            }
        }
        showBar();
    });
    for (const button of buttons) watch.observe(button);
}

latestVersion.then((version) => {
    if (!version) return;
    for (const place of document.querySelectorAll("[data-release]")) {
        place.querySelector("[data-release-version]").textContent = version;
        place.hidden = false;
    }
});
