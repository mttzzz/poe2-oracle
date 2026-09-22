//! PoE2 trade API client (leagues -> search -> fetch). Endpoints and request/response shapes are
//! migrated from the real, independently-verified POC in
//! `crates/poe2-oracle/examples/trade_api.rs` into a real library API here -- a move-and-refactor
//! of already-proven logic, not a rewrite.
//!
//! Depends only on `poe2-domain` for request/response shapes.
