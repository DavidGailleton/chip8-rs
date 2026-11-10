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
cargo run -- <rom>          # run (ROM path argument once implemented)
cargo test                  # all tests
cargo test <name>           # single test (substring match on test name)
cargo clippy -- -W clippy::pedantic   # lint
cargo fmt                   # format
```

Rust edition 2024, no dependencies yet.

## State

Freshly `cargo init`ed: `src/main.rs` is still Hello World. Planned first milestone: the IBM-logo instruction subset (`00E0 1NNN 6XNN 7XNN ANNN DXYN`), then the full instruction set validated with test ROMs.

## Reference material

- CHIP-8 spec, main loop and opcode table: `~/obsidian-vault/Wiki/Learning/CHIP-8-Emulation.md`
- Ambiguous instructions (COSMAC VIP vs CHIP-48/SUPER-CHIP): `~/obsidian-vault/Wiki/Learning/CHIP-8-Quirks.md`
- Test ROMs and tools: `~/obsidian-vault/Wiki/Reference/CHIP-8-Resources.md`
- Rust guidelines skill: `.agents/skills/rust-skills/` (invoke with `/rust-skills` when reviewing code)

Key spec points to keep in mind: 4 KB RAM, programs load at `0x200`, font at `0x050`–`0x09F`, 64×32 XOR display, VF doubles as the flag register, PC advances by 2 at fetch time, timers tick at 60 Hz independently of the ~700 instr/s CPU rate.
