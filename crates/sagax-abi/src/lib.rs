//! Internal number codes of the C and WebAssembly layers, not Rust API.
//!
//! `sagax-ffi` (the C interface) and `sagax-wasm` (the WebAssembly
//! interface for JavaScript) pass numbers where Rust has enums. This crate
//! is the one home of those numbers ([`codes`], the `SX_*` values of
//! `sagax.h`) and of the checked conversions between numbers and Rust
//! values ([`convert`]), so both layers decode and encode the same way.
//! Nothing here is meant for Rust users of Sagax.

pub mod codes;
pub mod convert;
