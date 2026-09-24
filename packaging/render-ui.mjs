#!/usr/bin/env node
// Saves the drawn interface (site/ui) as the images the README and the guide show: serves the
// repository, opens site/ui/shots.html in a headless Chrome over the DevTools protocol, waits
// for the drawings, and writes every `[data-shot]` element there as
// docs/guide/src/images/<lang>/<shot>.<webp|jpg> at twice its CSS size.
//
//   node packaging/render-ui.mjs [chrome flags]      (Node 22 or newer: WebSocket is built in)
//
// Chrome or Chromium: $CHROME, else the first of the usual names on PATH. No npm packages. Flags
// after the script go to Chrome -- `--no-sandbox` where it has no usable sandbox (Ubuntu 23.10+
// restricts the user namespaces it needs); it only ever opens this repository's pages.
//
// The app's text face is Segoe UI, which comes with Windows and may not be shared: render on
// Windows or with it installed, else the pictures' text comes out in a fallback face (it warns).

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile, mkdir } from "node:fs/promises";
import http from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.join(root, "docs/guide/src/images");
const types = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript",
    ".mjs": "text/javascript",
    ".css": "text/css",
    ".json": "application/json",
    ".webp": "image/webp",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".svg": "image/svg+xml",
    ".ttf": "font/ttf",
};

function serve() {
    const server = http.createServer(async (request, response) => {
        const file = path.join(root, decodeURIComponent(new URL(request.url, "http://x").pathname));
        if (!file.startsWith(root + path.sep)) {
            response.writeHead(403).end();
            return;
        }
        try {
            const body = await readFile(file);
            response.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
            response.end(body);
        } catch {
            response.writeHead(404).end();
        }
    });
    return new Promise((done) => server.listen(0, "127.0.0.1", () => done(server)));
}

function chromePath() {
    if (process.env.CHROME) return process.env.CHROME;
    const names = ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "chrome", "msedge"];
    for (const dir of (process.env.PATH ?? "").split(path.delimiter)) {
        for (const name of names) {
            for (const candidate of [path.join(dir, name), path.join(dir, `${name}.exe`)]) {
                if (existsSync(candidate)) return candidate;
            }
        }
    }
    throw new Error("no Chrome found: set CHROME to its path");
}

async function launch(profile) {
    const chrome = spawn(
        chromePath(),
        [
            "--headless=new",
            "--remote-debugging-port=0",
            `--user-data-dir=${profile}`,
            "--no-first-run",
            "--no-default-browser-check",
            "--hide-scrollbars",
            "--force-color-profile=srgb",
            // A real 2x screen: an emulated one snaps boxes to whole CSS px, moving the half-px
            // edges GPUI draws by a device px.
            "--force-device-scale-factor=2",
            "--window-size=1400,1000",
            // Grayscale text, as GPUI draws it, not LCD colour fringes.
            "--disable-lcd-text",
            ...process.argv.slice(2),
            "about:blank",
        ],
        { stdio: ["ignore", "ignore", "pipe"] },
    );
    const url = await new Promise((done, fail) => {
        let text = "";
        chrome.stderr.on("data", (chunk) => {
            text += chunk;
            const found = text.match(/DevTools listening on (ws:\/\/\S+)/);
            if (found) done(found[1]);
        });
        chrome.on("exit", (code) => fail(new Error(`Chrome exited (${code}): ${text}`)));
    });
    return { chrome, port: new URL(url).port };
}

/** A DevTools protocol session with one page. */
async function connect(port) {
    const target = await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" }).then((r) => r.json());
    const socket = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((done) => socket.addEventListener("open", done, { once: true }));
    let next = 0;
    const pending = new Map();
    const waiters = [];
    socket.addEventListener("message", ({ data }) => {
        const message = JSON.parse(data);
        if (message.id && pending.has(message.id)) {
            const { done, fail } = pending.get(message.id);
            pending.delete(message.id);
            message.error ? fail(new Error(message.error.message)) : done(message.result);
        } else if (message.method) {
            for (const waiter of waiters.splice(0)) waiter(message);
        }
    });
    const send = (method, params = {}) =>
        new Promise((done, fail) => {
            const id = ++next;
            pending.set(id, { done, fail });
            socket.send(JSON.stringify({ id, method, params }));
        });
    const event = (name) =>
        new Promise((done) => {
            const wait = (message) => (message.method === name ? done(message.params) : waiters.push(wait));
            waiters.push(wait);
        });
    return { send, event, close: () => socket.close() };
}

const server = await serve();
const profile = await mkdtemp(path.join(tmpdir(), "render-ui-"));
const { chrome, port } = await launch(profile);
try {
    const page = await connect(port);
    await page.send("Page.enable");
    await page.send("Emulation.setDeviceMetricsOverride", {
        width: 1400,
        height: 1000,
        deviceScaleFactor: 2,
        mobile: false,
    });
    const loaded = page.event("Page.loadEventFired");
    await page.send("Page.navigate", {
        url: `http://127.0.0.1:${server.address().port}/site/ui/shots.html`,
    });
    await loaded;
    const evaluate = async (expression) => {
        const { result, exceptionDetails } = await page.send("Runtime.evaluate", {
            expression,
            awaitPromise: true,
            returnByValue: true,
        });
        if (exceptionDetails) throw new Error(exceptionDetails.text);
        return result.value;
    };
    await evaluate("window.oracleUiReady.then(() => document.fonts.ready).then(() => true)");
    const segoe = await evaluate(`(() => {
        const context = document.createElement("canvas").getContext("2d");
        const width = (font) => ((context.font = font), context.measureText("Price check 1,234 Проверка").width);
        return ["monospace", "serif"].every((fallback) => width('16px "Segoe UI", ' + fallback) !== width("16px " + fallback));
    })()`);
    if (!segoe) console.warn("warning: Segoe UI isn't installed, so the text is drawn in a fallback face, not the app's");
    const shots = await evaluate(`[...document.querySelectorAll("[data-shot]")].map((element) => {
        const box = element.getBoundingClientRect();
        return {
            name: element.dataset.shot,
            lang: element.dataset.lang,
            format: element.dataset.format ?? "webp",
            scale: Number(element.dataset.shotScale ?? 1),
            x: box.left + scrollX,
            y: box.top + scrollY,
            width: box.width,
            height: box.height,
        };
    })`);
    for (const shot of shots) {
        const { data } = await page.send("Page.captureScreenshot", {
            format: shot.format === "jpg" ? "jpeg" : shot.format,
            quality: 90,
            captureBeyondViewport: true,
            clip: { x: shot.x, y: shot.y, width: shot.width, height: shot.height, scale: shot.scale },
        });
        const file = path.join(output, shot.lang, `${shot.name}.${shot.format}`);
        await mkdir(path.dirname(file), { recursive: true });
        await writeFile(file, Buffer.from(data, "base64"));
        console.log(path.relative(root, file), `${Math.round(shot.width)}x${Math.round(shot.height)} css px`);
    }
    page.close();
} finally {
    chrome.kill();
    server.close();
    await rm(profile, { recursive: true, force: true });
}
