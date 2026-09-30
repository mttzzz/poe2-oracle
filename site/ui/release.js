// The app's latest release, for the landing pages to name wherever they show PoE2 Oracle's version:
// under the download buttons (download.js), in the comparison table (bench.js) and in the settings
// window's drawing (settings.js). One request per page, whichever module asks first: GET
// /api/v1/releases/latest, what the updater asks, which counts only the app's own checks, so a page
// asking counts nothing.

/** The latest version, "0.1.4", or null when the service names none: no release yet, or no answer. */
export const latestVersion = fetch("/api/v1/releases/latest")
    .then((answer) => (answer.ok ? answer.json() : null))
    .then((release) => {
        const version = String(release?.tag_name ?? "").replace(/^v/, "");
        return /^\d+\.\d+\.\d+/.test(version) ? version : null;
    })
    .catch(() => null);
