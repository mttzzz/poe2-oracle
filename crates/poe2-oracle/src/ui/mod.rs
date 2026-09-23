//! GPUI presentation layer for the Price Check overlay. `panel` implements `gpui::Render` for
//! `crate::price_check::PriceCheckApp`, consuming its state -- see that module's own doc comment
//! for the orchestration/presentation split. `theme` is the palette every window draws with;
//! `text_field` the one-line text input the settings window edits strings with; `hint` the text
//! tooltip chips and buttons explain themselves with; `item_card` a listed item drawn as the
//! game's own tooltip.

pub mod fonts;
pub mod hint;
pub mod item_card;
pub mod panel;
pub mod settings_view;
pub mod text_field;
pub mod theme;
pub mod trade_overlay;
pub mod xp_overlay;
