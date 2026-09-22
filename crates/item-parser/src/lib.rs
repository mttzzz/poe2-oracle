//! Parses PoE2's Ctrl+C clipboard item-text format (the plain-text block the game writes to the
//! clipboard when a player copies an item) into `poe2_domain::Item`. Depends only on
//! `poe2-domain` for the output shape.
//!
//! Scaffolded here as part of the architecture foundation; the real parser is out of scope for
//! this plan and lands in the follow-up Price Check feature plan.
