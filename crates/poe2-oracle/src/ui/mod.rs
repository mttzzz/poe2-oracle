//! GPUI presentation layer for the Price Check overlay. `panel` implements `gpui::Render` for
//! `crate::price_check::PriceCheckApp`, consuming its state -- see that module's own doc comment
//! for the orchestration/presentation split. `theme` is the palette every window draws with;
//! `text_field` the one-line text input the settings window edits strings with; `hint` the text
//! tooltip chips and buttons explain themselves with; `item_card` a listed item drawn as the
//! game's own tooltip; `style` the game-styled frames, ornaments, motion and controls, and
//! `mockup` the dev-only preview of them (`POE2_ORACLE_MOCKUP=1`).

pub mod fonts;
pub mod hint;
pub mod item_card;
pub mod mockup;
pub mod panel;
pub mod settings_view;
pub mod style;
pub mod text_field;
pub mod theme;
pub mod trade_overlay;
pub mod xp_overlay;
