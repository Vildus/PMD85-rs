//! Emulation core for the Tesla PMD 85 family of 8-bit computers.
//!
//! This crate is pure emulation logic (CPU, memory, peripheral chips,
//! keyboard matrix) and carries no windowing or GPU dependencies, so it can
//! be exercised headlessly in tests.

pub mod audio;
pub mod bus;
pub mod chips;
pub mod cpu;
pub mod keyboard;
pub mod machine;
pub mod model;
pub mod vram;

pub use machine::Machine;
pub use model::Model;
