//! Aegis core: privacy engine, storage, search and an HTTP client that never
//! loads a byte it has decided to block.
//!
//! Nothing in this crate talks to a server owned by the Aegis project.

pub mod cookies;
pub mod dns;
pub mod filter;
pub mod http;
pub mod models;
pub mod search;
pub mod store;
pub mod urlclean;
pub mod util;
pub mod vault;

pub use crate::filter::{Category, Decision, FilterEngine};
pub use crate::models::*;
