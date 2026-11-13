# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Purpose and working rules

This is a **learning project**: David is building a CHIP-8 emulator to learn Rust and basic low-level programming.

- **Do not write or edit code in `src/` unless David explicitly asks for it.** Answer questions, explain concepts, review his code, and point at the relevant spec or Rust feature instead. Short illustrative snippets in an explanation are fine only when they clarify a concept; never hand over a ready-to-paste implementation.
- **Optimization matters.** When discussing a design or reviewing code, prefer the efficient approach and **explain why it beats the alternatives** (memory layout, allocations, copies, bounds checks, branch cost, cache behavior, what the compiler can optimize). Name the trade-off, don't just state the rule.
- Teach idiomatic Rust along the way (ownership, fixed-size arrays vs `Vec`, integer types and wrapping/overflow, `match` on opcode nibbles, etc.).

## Commands

```sh
cargo build                 # debug build
cargo build --release       # optimized build (use for performance discussions)
cargo run --release -- <rom> # run a ROM
cargo test                  # all tests
cargo test <name>           # single test (substring match on test name)
cargo clippy -- -W clippy::pedantic   # lint
cargo fmt                   # format
```

Rust edition 2024. Dependencies: `minifb` (window and keyboard) and `rand` (`CXNN`). On NixOS, `nix develop` or `direnv allow` provides the toolchain and the libraries `minifb` loads at runtime.

## State

All 35 standard CHIP-8 instructions are implemented, with a unit test for each one.

| File | Role |
|---|---|
| `src/chip8.rs` | The core: memory, registers, stack, timers, `decode` → `Instruction` → `execute`, and the tests |
| `src/frontend.rs` | The `minifb` window: converts the display to pixels, maps the keyboard |
| `src/main.rs` | The CLI and the 60 Hz main loop (CPU batch, timers, redraw, frame pacing) |
| `src/lib.rs` | The library's public API |

Quirks chosen: COSMAC VIP shifts (`8XY6`/`8XYE` copy `VY` first) and `BNNN` (jump to `NNN + V0`); modern behavior for `FX55`/`FX65` (`I` unchanged).

## Reference material

- [Tobias V. Langhoff's guide to making a CHIP-8 emulator](https://tobiasvl.github.io/blog/write-a-chip-8-emulator/): main loop, instructions and the ambiguous ones (COSMAC VIP vs CHIP-48/SUPER-CHIP)
- [Cowgod's CHIP-8 technical reference](http://devernay.free.fr/hacks/chip8/C8TECH10.HTM): opcode table
- [Timendus chip8-test-suite](https://github.com/Timendus/chip8-test-suite): test ROMs
- Rust guidelines skill: `/rust-skills`, used when reviewing code (installed locally, not part of the repo)

Key spec points to keep in mind: 4 KB RAM, programs load at `0x200`, font at `0x050`–`0x09F`, 64×32 XOR display, VF doubles as the flag register, PC advances by 2 at fetch time, timers tick at 60 Hz independently of the ~700 instr/s CPU rate.
