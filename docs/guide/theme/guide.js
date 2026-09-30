// The guide is a book per language (docs/guide/build.sh). This joins them in each book's header:
// the site's language switch, which leads to the same page in the other book, and the title as a
// link back to the site in the book's language. Its second block carries the landing tag on to the
// download link, as the site's pages do (site/from.js).
{
    const lang = document.documentElement.lang === "ru" ? "ru" : "en";
    const other = lang === "ru" ? "en" : "ru";

    // The book's root: the page's own folder, as the books have no subfolders, or on the 404 page,
    // which the server sends for any missing path in the book, its <base href> (site-url).
    const root = new URL("./", document.baseURI);
    const otherRoot = new URL(`../${other}/`, root).pathname;
    // Where the page is in the book: install.html, print.html, index.html, or "" for the root. The
    // other book has the same page there. A deeper path can only be the 404 page's, and its
    // counterpart is the other book's root. The hash stays behind: heading anchors differ between
    // the languages.
    const page = location.pathname.startsWith(root.pathname)
        ? location.pathname.slice(root.pathname.length)
        : null;
    const href = page === null || page.includes("/") ? otherRoot : otherRoot + page;

    // The same cookie the site's language links set (site/lang.js): oracle-web sends / and /guide/
    // to the language in it.
    const remember = (code) => {
        const secure = location.protocol === "https:" ? "; Secure" : "";
        document.cookie = `lang=${code}; Path=/; Max-Age=31536000; SameSite=Lax${secure}`;
    };

    const languages = document.createElement("div");
    languages.className = "lang-switch";
    languages.setAttribute("role", "group");
    languages.setAttribute("aria-label", lang === "ru" ? "Язык" : "Language");
    for (const code of ["en", "ru"]) {
        let item;
        if (code === lang) {
            item = document.createElement("span");
            item.setAttribute("aria-current", "page");
        } else {
            item = document.createElement("a");
            item.href = href;
            item.lang = code;
            item.hreflang = code;
            item.title = code === "ru" ? "Русский" : "English";
            item.addEventListener("click", () => remember(code));
        }
        item.textContent = code.toUpperCase();
        languages.append(item);
    }
    document.querySelector("#mdbook-menu-bar .right-buttons")?.prepend(languages);

    const title = document.querySelector("#mdbook-menu-bar .menu-title");
    if (title) {
        const home = document.createElement("a");
        home.href = lang === "ru" ? "/ru/" : "/";
        home.append(...title.childNodes);
        title.append(home);
    }

    // mdBook titles the 404 page "Page not found" in either book; its heading (src/<lang>/404.md)
    // is in the book's language.
    const notFound = "Page not found - ";
    const heading = document.querySelector("#mdbook-content main h1");
    if (document.title.startsWith(notFound) && heading) {
        document.title = `${heading.textContent} - ${document.title.slice(notFound.length)}`;
    }
}

// The landing tag, the same logic as site/from.js, which the site's pages load as a module: a
// link the maintainer publishes is tagged `?from=<tag>`, and the download comes pages later. So the
// tag is kept in the tab's session storage, which the browser drops when the tab closes (no cookie
// is set), and added to every link to the installer, /download/latest, such as the introduction's.
// oracle-web hands it on to the file's address and counts the download by it, when it is one of its
// own list of tags. Where the browser gives no storage, nothing is kept and no link changes.
{
    const KEY = "oracle-from";
    const TAG = /^[a-z0-9_-]{1,32}$/;

    /** The tag this tab came by, or null: the page's own, kept for the pages after it. */
    const landingTag = () => {
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
    };

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
}
