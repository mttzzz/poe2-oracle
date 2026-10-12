//! Items made from real trade listings: the text the game would copy for an item someone has
//! listed on the trade site, in English and in Russian, for developing and testing the app on any
//! item in any state -- an unidentified unique of a base with several, a fractured and desecrated
//! rare, a mirrored or twice corrupted one -- whether or not the developer has one.
//!
//! An item is always a real one: a listing the trade site returns, fetched from the English site
//! and the Russian one ([`listing`]). [`render`] writes the game's advanced copy of it, the one the
//! app asks the game for; [`check`] parses that text the way the app does and compares what it
//! reads with the listing; the `item-synth` tool also runs the app's own search for it, which
//! must find the very listing it came from. Each item made is kept in `library/` ([`library`]),
//! where the tests render and parse it again.

pub mod check;
pub mod classes;
pub mod library;
pub mod listing;
pub mod render;
mod texts;

pub use render::{Options, Synthesized, render};
