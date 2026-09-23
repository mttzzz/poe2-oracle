//! A plain text tooltip: what a chip, marker or button means, shown on hover -- a
//! [`style::game_hint`] with one line and no heading.

use gpui::{AnyView, App, SharedString, Window};

use crate::ui::fonts;
use crate::ui::style;
use crate::ui::theme::TEXT;

/// The builder `tooltip` takes, for a tooltip saying `text`.
pub fn hint(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    // The face only sets a heading, which a one-line hint doesn't have.
    style::game_hint(fonts::interface_font(), None, vec![(text.into(), TEXT)])
}
