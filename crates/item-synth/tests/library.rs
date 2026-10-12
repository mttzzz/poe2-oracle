//! Every item in the library, both languages: written from its listing exactly as the library
//! keeps it, and read back by the app's parser as the listing says (with the library's catalogs,
//! `library/catalog`). A change to the writer shows here as a text that moved; a change to the
//! parser that loses something of a real item, as the round trip's complaint.

use item_parser::IndexedCatalog;
use item_synth::check::round_trip;
use item_synth::library::{self, LANGUAGES};
use item_synth::{Options, render};

#[test]
fn every_library_item_is_written_as_kept_and_read_back_as_listed() {
    let root = library::root();
    let entries = library::entries(&root).expect("the library reads");
    assert!(!entries.is_empty(), "the library is empty");
    let mut failures = Vec::new();
    for language in LANGUAGES {
        let stats = library::load_catalog(&root, language).expect("the library's catalog reads");
        let catalog = IndexedCatalog::new(stats.clone());
        for entry in &entries {
            let code = library::code(language);
            let item = entry.items.get(language);
            let class = entry.class(language).expect("the entry's class");
            let options = Options {
                with_note: entry.meta.note,
            };
            let synthesized = match render(item, class, language, options) {
                Ok(synthesized) => synthesized,
                Err(err) => {
                    failures.push(format!("{} {code}: can't be written: {err:#}", entry.slug));
                    continue;
                }
            };
            if &synthesized.text != entry.texts.get(language) {
                failures.push(format!(
                    "{} {code}: written differently than the library has it:\n{}",
                    entry.slug, synthesized.text
                ));
            }
            if &synthesized.guesses != entry.meta.guesses.get(language) {
                failures.push(format!(
                    "{} {code}: makes up other things now: {:?}",
                    entry.slug, synthesized.guesses
                ));
            }
            for problem in round_trip(item, entry.texts.get(language), language, &catalog, &stats) {
                failures.push(format!("{} {code}: {problem}", entry.slug));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
