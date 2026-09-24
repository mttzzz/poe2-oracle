// Draws PoE2 Oracle's interface into the page from data, in place of screenshots: every element
// with `data-oracle="<component>"` gets that component (panel, market, xp, settings, hero) drawn
// from `data/<data-set or component>.<lang>.json` next to this file, `lang` being the element's
// `data-lang` or the page's. A component's `render(data, lang)` returns its element, or a promise
// of it. `data-scale` sets the app's UI scale (default 1), `data-alt` what a screen reader says
// for the picture. `window.oracleUiReady` settles once every drawing is done, its fonts and icons
// loaded.

const here = new URL(".", import.meta.url);
const loaded = new Set();

function stylesheet(name) {
    const href = new URL(name, here).href;
    if (loaded.has(href)) return;
    loaded.add(href);
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = href;
    document.head.append(link);
}

async function stylesheetsReady() {
    const links = [...document.querySelectorAll('link[rel="stylesheet"]')].filter((link) =>
        loaded.has(link.href),
    );
    await Promise.all(
        links.map((link) =>
            link.sheet ? null : new Promise((done) => link.addEventListener("load", done, { once: true })),
        ),
    );
}

async function draw(element) {
    const component = element.dataset.oracle;
    const lang = element.dataset.lang || document.documentElement.lang || "en";
    const set = element.dataset.set || component;
    stylesheet("base.css");
    stylesheet(`${component}.css`);
    const [module, data] = await Promise.all([
        import(new URL(`${component}.js`, here).href),
        fetch(new URL(`data/${set}.${lang}.json`, here)).then((answer) => {
            if (!answer.ok) throw new Error(`${set}.${lang}.json: HTTP ${answer.status}`);
            return answer.json();
        }),
    ]);
    await stylesheetsReady();
    await Promise.all([
        document.fonts.load(`500 16px "Alegreya SC"`),
        document.fonts.load(`700 16px "Philosopher"`),
    ]);
    element.classList.add("oui");
    element.lang = lang;
    if (element.dataset.scale) element.style.setProperty("--oui-scale", element.dataset.scale);
    if (element.dataset.alt) {
        element.setAttribute("role", "img");
        element.setAttribute("aria-label", element.dataset.alt);
    }
    element.replaceChildren(await module.render(data, lang));
    if ("fit" in element.dataset) fitWidth(element, Number(element.dataset.scale ?? 1));
    await Promise.all(
        [...element.querySelectorAll("img")].map((image) =>
            image.complete ? null : new Promise((done) => {
                image.addEventListener("load", done, { once: true });
                image.addEventListener("error", done, { once: true });
            }),
        ),
    );
}

/** Shrinks `element`'s drawing, drawn at `scale`, to the width its container gives it -- as a
 * screenshot would shrink -- and follows the container as it resizes. */
function fitWidth(element, scale) {
    const room = element.parentElement;
    const apply = () => {
        element.style.setProperty("--oui-scale", scale);
        // Its width unconstrained: a drawing that wraps (the XP lines) would shrink to the room.
        element.style.width = "max-content";
        const natural = element.firstElementChild?.offsetWidth ?? 0;
        element.style.width = "";
        const width = room.clientWidth;
        if (natural > width && width > 0) {
            element.style.setProperty("--oui-scale", (scale * width) / natural);
        }
    };
    apply();
    new ResizeObserver(apply).observe(room);
}

window.oracleUiReady = Promise.all(
    [...document.querySelectorAll("[data-oracle]")].map((element) =>
        draw(element).catch((error) => {
            console.error("oracle-ui:", element.dataset.oracle, error);
        }),
    ),
);
