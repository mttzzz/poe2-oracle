// The pages' language links, the header's EN/RU switch and the footer's "По-русски" / "In English",
// remember the choice in the `lang` cookie: oracle-web sends / and /guide/ to the language in it,
// and without it goes by the browser's languages. The guide's own switch sets the same cookie
// (docs/guide/theme/guide.js).
for (const link of document.querySelectorAll("a[hreflang]")) {
    link.addEventListener("click", () => {
        const lang = link.hreflang;
        if (lang !== "en" && lang !== "ru") {
            return;
        }
        const secure = location.protocol === "https:" ? "; Secure" : "";
        document.cookie = `lang=${lang}; Path=/; Max-Age=31536000; SameSite=Lax${secure}`;
    });
}
