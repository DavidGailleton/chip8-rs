//! A CHIP-8 interpreter.
//!
//! The library holds the emulator core ([`Chip8`]); the `chip8-rs` binary
//! handles the command line, files, and (later) display and input.

mod chip8;
mod frontend;

pub use chip8::{Chip8, LoadError};
pub use frontend::{FPS, HEIGHT, MfbWindow, WIDTH};
