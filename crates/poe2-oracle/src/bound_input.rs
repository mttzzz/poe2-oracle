//! A filter row's min/max box, edited a keystroke at a time: GPUI ships no text input, and a
//! search bound needs no cursor or selection -- digits, a decimal point, a leading minus,
//! backspace. Pure, so the native test pass covers it; `price_check` holds the boxes' text and
//! `ui::panel::filters` draws them.

/// Applies `key` (GPUI's key name) to a box's `text`: digits type, `.` or `,` types the decimal
/// point -- a Russian layout gives `,` for its Shift+`.` and its numpad's decimal key --, a
/// leading `-` types and `backspace` deletes. Right after a click into the box (`fresh`) the
/// first of them replaces the whole value, as EE2 selects it on focus. Returns whether `key` was
/// one of them; any other leaves the box as it was.
pub fn type_key(text: &mut String, fresh: &mut bool, key: &str) -> bool {
    let is_digit = key.len() == 1 && key.as_bytes()[0].is_ascii_digit();
    if !(is_digit || matches!(key, "." | "," | "-" | "backspace")) {
        return false;
    }
    if std::mem::take(fresh) {
        text.clear();
    }
    match key {
        "backspace" => {
            text.pop();
        }
        "." | "," => {
            if !text.contains('.') {
                text.push('.');
            }
        }
        "-" => {
            if text.is_empty() {
                text.push('-');
            }
        }
        digit => text.push_str(digit),
    }
    true
}

/// A bound as its box shows it: up to two decimals with trailing zeros dropped (`8.19`, `8.5`),
/// a whole number bare. An integer stat's roll is whole (`stat_filters` floors and ceils it),
/// so its fraction can only be one the player typed, and stays as typed -- the search sends it.
pub fn format(value: f64, dp: bool) -> String {
    if !dp && value.fract() == 0.0 {
        return format!("{}", value as i64);
    }
    let mut text = format!("{value:.2}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(start: &str, fresh: bool, keys: &[&str]) -> String {
        let mut text = start.to_owned();
        let mut fresh = fresh;
        for key in keys {
            type_key(&mut text, &mut fresh, key);
        }
        text
    }

    #[test]
    fn a_click_replaces_the_value_and_later_keys_edit_it() {
        assert_eq!(typed("96", true, &["2", "0", "0"]), "200");
        assert_eq!(typed("96", true, &["backspace"]), "");
        assert_eq!(typed("96", false, &["backspace", "5"]), "95");
        // A key the box doesn't take leaves it selected for the next one.
        let (mut text, mut fresh) = ("96".to_owned(), true);
        assert!(!type_key(&mut text, &mut fresh, "tab"));
        assert_eq!((text.as_str(), fresh), ("96", true));
    }

    #[test]
    fn a_comma_types_the_decimal_point_and_both_only_once() {
        assert_eq!(typed("", false, &["8", ",", "1", "9"]), "8.19");
        assert_eq!(typed("", false, &["8", ".", "1", ",", "."]), "8.1");
        assert_eq!(typed("", false, &["-", "3", "-"]), "-3");
    }

    #[test]
    fn a_typed_fraction_shows_as_the_search_sends_it() {
        assert_eq!(format(8.19, true), "8.19");
        assert_eq!(format(8.0, true), "8");
        assert_eq!(format(-3.0, false), "-3");
        assert_eq!(format(8.5, false), "8.5");
    }
}
