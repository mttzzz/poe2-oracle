// Where a visitor came from, carried on to the download. A link the maintainer publishes is tagged
// `?from=<tag>` (`reddit`, `forum`, ...): oracle-web counts the visit by it, but the download comes
// pages later. So this keeps the tag in the tab's session storage, which the browser drops when the
// tab closes (no cookie is set), and adds it to every link to the installer, /download/latest.
// oracle-web hands it on to the file's address and counts the download by it, when it is one of its
// own list of tags. The guide's script does the same (docs/guide/theme/guide.js). Where the browser
// gives no storage, nothing is kept and no link changes.
const KEY = "oracle-from";
const TAG = /^[a-z0-9_-]{1,32}$/;

/** The tag this tab came by, or null: the page's own, kept for the pages after it. */
function landingTag() {
    try {
        const own = new URLSearchParams(location.search).get("from");
        if (own !== null && TAG.test(own)) {
            sessionStorage.setItem(KEY, own);
        }
        const kept = sessionStorage.getItem(KEY);
        return kept !== null && TAG.test(kept) ? kept : null;
    } catch {
        return null;
    }
}

const tag = landingTag();
if (tag !== null) {
    for (const link of document.querySelectorAll("a[href]")) {
        if (link.origin === location.origin && link.pathname === "/download/latest") {
            const query = new URLSearchParams(link.search);
            query.set("from", tag);
            link.search = query.toString();
        }
    }
}
