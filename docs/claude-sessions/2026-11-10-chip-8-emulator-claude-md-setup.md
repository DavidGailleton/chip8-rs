# Chip-8 emulator CLAUDE.md setup

*Claude Code session, 2026-11-10. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

`/init` I will create a chip-8 emulator. The purpose of this project is to learn how to write rust and basic low levele programming. You will answer my question but not writing code except if i explictly tell you. I would like to make my project optimized, you will explain me why it's better to do this way and not an other.

> 🔧 **Bash** — Inspect project files and source
> 🔧 **Bash** — List .claude contents and read wiki spec page
> 🔧 **Write** — ~/Documents/chip8-rs/CLAUDE.md
> 🔧 **Bash** — Save teaching-mode note to vault and commit

### 🤖 Claude

I created `CLAUDE.md` in the repo. The project is a fresh `cargo init`: Rust edition 2024, no dependencies, and `main.rs` still prints "Hello, world!". So the file covers mostly how to work with you, not existing architecture:

- **Your rules, at the top:** Claude won't write or edit code in `src/` unless you explicitly ask. It answers questions, reviews your code and points you to the spec. When something can be optimized, it explains why one approach beats another (memory layout, memory allocation, bounds checks, branching) and names the trade-off.
- **Commands:** build, release build, run, test, run a single test, clippy and fmt.
- **Current state and first milestone:** get the IBM logo on screen with six instructions (`00E0 1NNN 6XNN 7XNN ANNN DXYN`).
- **References:** links to your vault pages on the CHIP-8 spec, quirks and test ROMs, plus the `rust-skills` skill. It also lists the key spec points: 4 KB of memory, programs load at `0x200`, 64×32 display drawn with XOR, VF is also the flag register, and the timers tick at 60 Hz separately from the CPU.

I also saved these rules as a note in your vault (`raw/memory/2026-11-10-chip8-rs-teaching-mode.md`) and committed it. I haven't run `/llm-wiki ingest` on it yet, so it isn't in the wiki pages.

Nothing in the chip8-rs repo is committed yet.

### 🧑 David

`/model`
