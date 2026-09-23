// The book is one mdBook with an English and a Russian part, and mdBook sets a single
// <html lang> for all of it. Pages of the Russian part (src/ru/) declare Russian, so browsers
// pick the right hyphenation and screen readers the right voice.
if (/\/ru\/[^/]*$/.test(window.location.pathname)) {
    document.documentElement.lang = "ru";
}
