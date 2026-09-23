//! The unit a price reads in: divine orbs, or exalted orbs for what is worth less than one
//! (`Market::in_display_unit`, poe2scout's prices).

use crate::cx::{DIVINE, EXALTED};

/// The currency a price is quoted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceUnit {
    Divine,
    Exalted,
}

impl PriceUnit {
    /// The trade API currency id -- the same key a listing's `price.currency` carries.
    pub fn trade_id(self) -> &'static str {
        match self {
            PriceUnit::Divine => DIVINE,
            PriceUnit::Exalted => EXALTED,
        }
    }
}
