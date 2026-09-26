// Notes round a picture, the way a manual's figure points out its parts: each note a label in a
// band above or below the picture, with a gold leader from it to the part it explains and an
// arrowhead on that part. Where the labels don't fit the width the picture leaves them, each part
// gets a numbered marker instead, and the notes a numbered legend under the picture.
//
// The labels are the page's text -- its body face at the page's size, whatever scale the picture
// is drawn at -- in one list a screen reader reads after the picture: the picture is marked
// [data-picture], which oracle-ui.js gives the page's alt, and the list stays outside it. Leaders
// and markers are drawing only. All of it follows the picture: laid out again whenever the picture
// changes size (data-fit's scale, the window) and once the page's fonts have loaded.

import { h } from "./dom.js";

// Between labels side by side, and between a band's labels and the picture, px.
const GAP = 12;
const CLEAR = 18;
// A label is as wide as its words up to MAX rem, the widest cut down alike until their band fits,
// but to no less than MIN rem, and into no more than LINES lines: past that, the notes turn into
// the legend.
const MIN = 8;
const MAX = 15;
const LINES = 3;
// How far in from the end of a label's line its leader may leave it, px.
const INSET = 6;
// The arrowhead's length and width, px.
const ARROW = [6, 5];
// A marker's radius, and the room between two markers, px.
const MARKER = 8;
const MARKER_GAP = 2;
// Sides are tried for at most this many notes (2^n layouts); any more stay above.
const MAX_CHOICES = 10;

const measure = document.createElement("canvas").getContext("2d");

/**
 * `picture` with `notes` round it, to draw in its place. A note is `{ text, target, side? }`: its
 * label; the element in `picture` it points at -- the ink of its text, or its shapes or box when
 * it has none; and the band its label takes, "above" or "below" the picture -- by default the one
 * that gives it the shorter, straighter leader.
 */
export function callouts(picture, notes) {
    picture.dataset.picture = "";
    const labels = notes.map((note) => h("li", "oui-callout", term(note.target), note.text));
    const lines = svg("svg", { class: "oui-callouts-lines", "aria-hidden": "true" });
    const root = h("div", "oui-callouts", picture, lines, h("ol", "oui-callouts-notes", labels));
    // A label under the pointer lights its leader or marker.
    labels.forEach((label, index) => {
        const light = (on) => {
            label.classList.toggle("oui-callout--lit", on);
            lines.querySelector(`[data-note="${index}"]`)?.classList.toggle("oui-callout--lit", on);
        };
        label.addEventListener("pointerenter", () => light(true));
        label.addEventListener("pointerleave", () => light(false));
    });
    // Laid out a frame after the picture changes size, not in the observer's own callback: the
    // bands' depth changes the size of the figure, which data-fit observes too.
    let pending = 0;
    const redraw = () => {
        cancelAnimationFrame(pending);
        pending = requestAnimationFrame(() => {
            if (!root.isConnected) return;
            layout(root, picture, lines, labels, notes);
            root.classList.add("oui-callouts--laid");
        });
    };
    new ResizeObserver(redraw).observe(picture);
    document.fonts.ready.then(redraw);
    return root;
}

// What a label explains, for a screen reader, which doesn't see the picture: the words it points
// at.
function term(target) {
    const words = target.textContent.trim();
    return words ? h("span", "oui-callout-term", `${words}: `) : null;
}

function layout(root, picture, lines, labels, notes) {
    const frame = picture.getBoundingClientRect();
    if (!frame.width) return;
    const targets = notes.map((note) => {
        const box = ink(note.target);
        return {
            x: (box.left + box.right) / 2 - frame.left,
            top: box.top - frame.top,
            bottom: box.bottom - frame.top,
        };
    });
    root.classList.remove("oui-callouts--legend");
    for (const label of labels) label.style.cssText = "width: max-content";
    const naturals = labels.map((label) => label.getBoundingClientRect().width);
    const rem = parseFloat(getComputedStyle(document.documentElement).fontSize);
    const plan = arrange(targets, naturals, notes, frame, rem);
    lines.replaceChildren();
    if (!plan || !bands(root, picture, lines, labels, targets, plan)) legend(root, picture, lines, labels, targets);
}

// The bands for the labels -- each note's side, and the most its label may be wide -- that fit and
// give the shortest, straightest leaders, or null when none does.
function arrange(targets, naturals, notes, frame, rem) {
    const free = notes.flatMap((note, index) => (note.side ? [] : [index])).slice(0, MAX_CHOICES);
    let best = null;
    for (let choice = 0; choice < 2 ** free.length; choice += 1) {
        const sides = notes.map((note) => note.side ?? "above");
        free.forEach((index, bit) => {
            if (choice & (1 << bit)) sides[index] = "below";
        });
        const plan = { sides, widths: [], cost: 0 };
        const fits = ["above", "below"].every((side) => {
            const members = band(sides, side, targets);
            if (!members.length) return true;
            const widths = share(members.map((index) => naturals[index]), frame.width, MIN * rem, MAX * rem);
            // Words don't break evenly: a tenth over the cut is about what wrapping wastes.
            if (!widths || members.some((index, at) => Math.ceil((naturals[index] * 1.1) / widths[at]) > LINES)) {
                return false;
            }
            // A label cut down wraps into lines about alike, narrower than the cut.
            const tight = members.map((index, at) => naturals[index] / Math.ceil(naturals[index] / widths[at]));
            const lefts = row(members.map((index, at) => ({ centre: targets[index].x, width: tight[at] })), frame.width, GAP);
            members.forEach((index, at) => {
                const target = targets[index];
                plan.widths[index] = widths[at];
                const inset = Math.min(INSET, tight[at] / 2);
                const across = Math.abs(clamp(target.x, lefts[at] + inset, lefts[at] + tight[at] - inset) - target.x);
                const down = CLEAR + (side === "above" ? target.top : frame.height - target.bottom);
                plan.cost += Math.hypot(across, down) + 2 * across;
            });
            return true;
        });
        if (fits && (!best || plan.cost < best.cost)) best = plan;
    }
    return best;
}

// The notes on `side`, left to right by their parts.
function band(sides, side, targets) {
    return sides.flatMap((each, index) => (each === side ? [index] : [])).sort((a, b) => targets[a].x - targets[b].x);
}

// Widths for labels whose words are `naturals` wide, side by side across `room`: each its words'
// own width, the widest cut down alike (never below `min`) until the row fits -- or null when it
// doesn't even then.
function share(naturals, room, min, max) {
    const space = room - GAP * (naturals.length - 1);
    const cut = (cap) => naturals.map((natural) => Math.min(natural, cap));
    const total = (cap) => cut(cap).reduce((sum, width) => sum + width, 0);
    if (total(min) > space) return null;
    if (total(max) <= space) return cut(max);
    let [low, high] = [min, max];
    while (high - low > 0.5) {
        const middle = (low + high) / 2;
        if (total(middle) <= space) low = middle;
        else high = middle;
    }
    return cut(low);
}

// Lefts for `boxes` (`{ centre, width }`, in order) in a row across `room`, `gap` apart, each as
// near the left that centres it on its `centre` as the others let it be: boxes that would overlap
// move as one, centred on their centres on average, and within the room.
function row(boxes, room, gap) {
    const clusters = [];
    const left = (cluster) => clamp(cluster.sum / cluster.count, 0, room - cluster.width);
    for (const box of boxes) {
        let cluster = { boxes: [box], width: box.width, sum: box.centre - box.width / 2, count: 1 };
        while (clusters.length && left(clusters.at(-1)) + clusters.at(-1).width + gap > left(cluster)) {
            const before = clusters.pop();
            cluster = {
                boxes: [...before.boxes, ...cluster.boxes],
                width: before.width + gap + cluster.width,
                sum: before.sum + cluster.sum - cluster.count * (before.width + gap),
                count: before.count + cluster.count,
            };
        }
        clusters.push(cluster);
    }
    return clusters.flatMap((cluster) => {
        let x = left(cluster);
        return cluster.boxes.map((box) => {
            const at = x;
            x += box.width + gap;
            return at;
        });
    });
}

// The labels in their bands over and under the picture, each leader leaving its label's line
// nearest the picture as near straight over (or under) its part as the line reaches -- or false,
// laying out nothing, when a label takes more than LINES lines after all.
function bands(root, picture, lines, labels, targets, plan) {
    // Each label wraps at its width, then is only as wide as its longest line: the line nearest
    // the picture then spans about all of it, for its leader to leave from over the part.
    labels.forEach((label, index) => {
        label.style.width = `${plan.widths[index]}px`;
    });
    const wrapped = labels.map(lineRects);
    if (wrapped.some((rects) => rects.length > LINES)) return false;
    const widths = wrapped.map((rects) => Math.ceil(Math.max(...rects.map((rect) => rect.width))) + 1);
    const frame = picture.getBoundingClientRect();
    for (const side of ["above", "below"]) {
        const members = band(plan.sides, side, targets);
        const lefts = row(members.map((index) => ({ centre: targets[index].x, width: widths[index] })), frame.width, GAP);
        members.forEach((index, at) => {
            const label = labels[index];
            label.style.width = `${widths[index]}px`;
            label.style.left = `${lefts[at]}px`;
            // Its lines flush to the side its part is on, when the others pushed it aside.
            const aside = targets[index].x - (lefts[at] + widths[index] / 2);
            label.style.textAlign = Math.abs(aside) <= widths[index] / 4 ? "" : aside < 0 ? "left" : "right";
        });
    }
    const heights = labels.map((label) => label.getBoundingClientRect().height);
    // A band is as deep as its deepest label, and the room between it and the picture.
    const depth = (side) => {
        const own = heights.filter((_, index) => plan.sides[index] === side);
        return own.length ? Math.max(...own) + CLEAR : 0;
    };
    const top = depth("above");
    root.style.paddingTop = `${top}px`;
    root.style.paddingBottom = `${depth("below")}px`;
    labels.forEach((label, index) => {
        const y = plan.sides[index] === "above" ? top - CLEAR - heights[index] : top + frame.height + CLEAR;
        label.style.top = `${y}px`;
    });
    const origin = root.getBoundingClientRect();
    const box = picture.getBoundingClientRect();
    const [dx, dy] = [box.left - origin.left, box.top - origin.top];
    labels.forEach((label, index) => {
        const target = targets[index];
        const above = plan.sides[index] === "above";
        const rects = lineRects(label);
        const line = above ? rects.at(-1) : rects[0];
        const [left, right] = [line.left - origin.left, line.right - origin.left];
        const inset = Math.min(INSET, (right - left) / 2);
        const tip = [dx + target.x, dy + (above ? target.top - 1 : target.bottom + 1)];
        let x = clamp(tip[0], left + inset, right - inset);
        // Straight down (or up), through a pixel's middle, is a crisp hairline.
        if (x === tip[0]) x = tip[0] = Math.floor(origin.left + x) + 0.5 - origin.left;
        const from = [x, above ? line.bottom - origin.top + 3 : line.top - origin.top - 3];
        lines.append(lead(index, from, tip));
    });
    return true;
}

// The rects of a label's lines, top to bottom -- its words, not the screen reader's term.
function lineRects(label) {
    const range = document.createRange();
    range.selectNodeContents(label.lastChild);
    return [...range.getClientRects()].filter((rect) => rect.width > 0);
}

// A leader from `from` to the arrowhead's tip at `tip`, over a dark halo that keeps it clear of
// the picture.
function lead(index, [x0, y0], [x1, y1]) {
    const length = Math.hypot(x1 - x0, y1 - y0) || 1;
    const [ux, uy] = [(x1 - x0) / length, (y1 - y0) / length];
    const [long, wide] = ARROW;
    const [bx, by] = [x1 - ux * long, y1 - uy * long];
    const [nx, ny] = [(-uy * wide) / 2, (ux * wide) / 2];
    const head = `${x1},${y1} ${bx + nx},${by + ny} ${bx - nx},${by - ny}`;
    const line = { x1: x0, y1: y0, x2: bx, y2: by };
    return svg(
        "g",
        { class: "oui-callout-lead", "data-note": index },
        svg("line", { class: "oui-callout-halo", ...line }),
        svg("polygon", { class: "oui-callout-halo", points: head }),
        svg("line", line),
        svg("polygon", { points: head }),
    );
}

// Numbered markers just over the parts, side by side where parts are close, and the notes as a
// numbered legend under the picture.
function legend(root, picture, lines, labels, targets) {
    root.classList.add("oui-callouts--legend");
    for (const label of labels) label.removeAttribute("style");
    root.style.paddingBottom = "";
    const width = picture.getBoundingClientRect().width;
    const order = targets.map((_, index) => index).sort((a, b) => targets[a].x - targets[b].x);
    const lefts = row(order.map((index) => ({ centre: targets[index].x, width: 2 * MARKER })), width, MARKER_GAP);
    const centres = [];
    order.forEach((index, at) => {
        centres[index] = lefts[at] + MARKER;
    });
    const heights = targets.map((target) => target.top - 3 - MARKER);
    const lift = Math.max(0, MARKER + 1 - Math.min(...heights));
    root.style.paddingTop = `${lift}px`;
    targets.forEach((target, index) => {
        const [x, y] = [centres[index], heights[index] + lift];
        lines.append(
            svg(
                "g",
                { class: "oui-callout-marker", "data-note": index },
                svg("line", { x1: x, y1: y + MARKER, x2: target.x, y2: target.top - 1 + lift }),
                svg("circle", { cx: x, cy: y, r: MARKER - 0.5 }),
                svg("text", { x, y, "text-anchor": "middle", "dominant-baseline": "central" }, String(index + 1)),
            ),
        );
    });
}

// The box of what `target` draws: its text's ink, from its font's measures, the bounds of its
// shapes, or its own box.
function ink(target) {
    if (target instanceof SVGElement) {
        const shapes = [...target.querySelectorAll("*")]
            .map((shape) => shape.getBoundingClientRect())
            .filter((box) => box.width || box.height);
        return shapes.length ? union(shapes) : target.getBoundingClientRect();
    }
    const words = target.textContent;
    if (!words.trim()) return target.getBoundingClientRect();
    const range = document.createRange();
    range.selectNodeContents(target);
    const box = range.getBoundingClientRect();
    const style = getComputedStyle(target);
    measure.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    const metrics = measure.measureText(words);
    const baseline = box.top + metrics.fontBoundingBoxAscent;
    return {
        left: box.left,
        right: box.right,
        top: baseline - metrics.actualBoundingBoxAscent,
        bottom: baseline + metrics.actualBoundingBoxDescent,
    };
}

function union(boxes) {
    return {
        left: Math.min(...boxes.map((box) => box.left)),
        right: Math.max(...boxes.map((box) => box.right)),
        top: Math.min(...boxes.map((box) => box.top)),
        bottom: Math.max(...boxes.map((box) => box.bottom)),
    };
}

function clamp(value, low, high) {
    return Math.min(Math.max(value, low), high);
}

function svg(tag, attrs, ...children) {
    const element = document.createElementNS("http://www.w3.org/2000/svg", tag);
    for (const [name, value] of Object.entries(attrs)) element.setAttribute(name, value);
    element.append(...children);
    return element;
}
