//! GPUI presentation layer for the Price Check overlay. `panel` implements `gpui::Render` for
//! `crate::price_check::PriceCheckApp`, consuming its state -- see that module's own doc comment
//! for the orchestration/presentation split. `theme` is the palette every window draws with;
//! `text_field` the one-line text input the settings window edits strings with; `hint` the text
//! tooltip chips and buttons explain themselves with; `item_card` a listed item drawn as the
//! game's own tooltip; `style` the game-styled frames, ornaments, motion and controls, and
//! `ornament` those ornaments' exact device pixels, `part_height` how tall a part of the price
//! panel drawn from its last frame stands; `tour` the onboarding tour's spotlight over the other
//! windows; `welcome` the dialog over the settings window after an install; and `toast` the plate
//! over the game after an update.
//!
//! Only `theme`, `ornament` and `part_height` build on every target -- the palette, the
//! ornaments' pixels, which the native test pass checks and `examples/ornaments.rs` draws, and
//! the parts' heights; the rest is Windows-only, like the platform layer it runs on.

#[cfg(target_os = "windows")]
pub mod fonts;
#[cfg(target_os = "windows")]
pub mod hint;
#[cfg(target_os = "windows")]
pub mod item_card;
pub mod ornament;
#[cfg(target_os = "windows")]
pub mod panel;
pub mod part_height;
#[cfg(target_os = "windows")]
pub mod report_view;
#[cfg(target_os = "windows")]
pub mod settings_view;
#[cfg(target_os = "windows")]
pub mod style;
#[cfg(target_os = "windows")]
pub mod text_area;
#[cfg(target_os = "windows")]
pub mod text_field;
pub mod theme;
#[cfg(target_os = "windows")]
pub mod toast;
#[cfg(target_os = "windows")]
pub mod tour;
#[cfg(target_os = "windows")]
pub mod welcome;
#[cfg(target_os = "windows")]
pub mod xp_overlay;
