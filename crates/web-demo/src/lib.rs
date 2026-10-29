//! Browser demo core: the reference matcher plus unsigned settlement drafts.
//!
//! Everything here is plain safe Rust and runs natively in tests. On `wasm32` the small
//! `ffi` module exposes it to JavaScript without any generated glue code.
#![deny(unsafe_code)]

mod session;

#[cfg(target_arch = "wasm32")]
mod ffi;

pub use session::{conformance_report, Session, DEADLINE, VECTORS};
