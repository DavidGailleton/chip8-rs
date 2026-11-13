# Unit tests for all instructions

*Claude Code session, 2026-11-13. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

Normally, i set all available instructions. Create unit tests for each one of them and add comment.

> 🔧 **Bash** — Inspect project files and emulator core
> 🔧 **Bash** — List functions and existing tests

### 🤖 Claude

Many tests already exist. Let me read the implementation and the full test module to find gaps.

> 🔧 **Read** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Bash** — List remaining tests, frontend, and run tests
> 🔧 **Read** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Bash** — Read quirks wiki page and relevant spec lines
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs

### 🤖 Claude

Now fix the stale tests (font is now loaded at `0x050`, and the decoder supports more opcodes).

> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs

### 🤖 Claude

Now the new tests, appended at the end of the test module.

> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Bash** — Run the test suite and summarize failures
> 🔧 **Bash** — Inspect decode and FX33 failures, run clippy
> 🔧 **Bash** — Run clippy and check formatting
> 🔧 **Bash** — Format code and rerun tests

### 🤖 Claude

I added tests for all 13 instructions you implemented since the IBM-logo subset, and documented them. The suite now runs **141 tests: 130 pass, 11 fail**. All 11 failures point at bugs in your code. I didn't touch any implementation code, so you can fix them yourself.

## What changed in `src/chip8.rs`
- **Comments:** doc comments on every `Instruction` variant and field that lacked one (`BNNN` through `FX65`), on `key_pressed`, `delay_timer` and `sound_timer`, and on `overflowing_add` and `overflowing_sub`.
- **New tests**, each with a comment saying what it checks:
  - `BNNN`: uses V0 not VX (COSMAC behaviour), wraps past `0xFFF`.
  - `CXNN`: the mask is respected; the values actually vary.
  - `EX9E` / `EXA1`: matching key, other key, no key, and key `0`, which must not be read as "no key".
  - `FX07`, `FX15`, `FX18`: plus a round trip through the delay timer.
  - `FX1E`: the add, and VF set when I passes `0xFFF` (Amiga behaviour).
  - `FX0A`: blocks with no key, then resumes when one arrives.
  - `FX29`: all 16 digits.
  - `FX33`: 156, leading zeros, 0 and 255; only memory changes.
  - `FX55` / `FX65`: X is inclusive, `F0`/`FF` edge cases, I left unchanged (modern behaviour), and a store-then-load round trip.
- **Out-of-date tests updated:** three tests assumed memory starts all zero, but the font now sits at `0x050`. The test that tries all 65,536 opcodes now accepts the new ones. I also added a test that the font is loaded.

## Bugs the failing tests found
1. **`FX29` uses the register number instead of the value in it.** `FONT_START + x * 5` should use the value stored in VX. This fails 2 tests.
2. **`FX33` writes to registers instead of memory.** `put_hex_to_pseudo_dec_in_register` calls `set_register(self.i + i, …)`, so with `I = 0x300` it returns `IndexToHigh { index: 768 }`. It also writes nothing for 0 and no leading zeros, but the spec always writes exactly three digits to `I`, `I+1` and `I+2`. This fails 5 tests.
   - A faster approach: three fixed operations (`/100`, `/10 % 10`, `% 10`). Division by a constant compiles to a multiply and a shift, with no recursion or stack frames and a known length the compiler can optimise.
3. **`IndexToHigh` reports `max: VF` (15).** Its own doc says valid indexes are `0..max`, so it should be `REGISTER_SIZE` (16). This fails 3 tests.
4. **`decode` accepts `8XY8`–`8XYD` and `8XYF`.** The wildcard `(0x8, _, _, _)` lets them through, and they are only rejected inside `logic_instruction`. That function then has to rebuild the opcode just for the error. If `decode` rejects them, every `Instruction` is valid by construction. This fails 1 test.

## Not covered by tests, worth knowing
- The timers are never decremented. You need a 60 Hz tick, separate from `step`.
- **Out-of-range I:** `FX1E` lets I go past `0xFFF` and never clears VF. If I is near `0xFFF`, a later `DXYN`, `FX55` or `FX65` will panic on the slice index.
- Skip instructions use `program_counter += 2` instead of `pc_jump_to`, so they don't wrap at the end of memory.
- `x >= MEMORY_SIZE` in `FX55` and `FX65` is dead code, because X is a 4-bit value (at most 15).

`cargo fmt --check` and `cargo clippy --all-targets` are both clean.

### 🧑 David

@src/chip8.rs#L588-590 Is it possible to set a range of invalid ?

### 🤖 Claude

Yes. Rust patterns accept **inclusive ranges** (`a..=b`), and you can combine them with `|` (or-patterns), including inside a tuple:

```rust
(0x8, _, _, 0x8..=0xD | 0xF) => Err(ExecutionError::UnknownOpcode { opcode }),
```

Your three lines only reject `8`, `D` and `F`. `9`, `A`, `B` and `C` still reach `(0x8, _, _, _)`. The range fixes that.

**A better option is to list the valid values instead of the invalid ones.** Put the condition on the arm that accepts the instruction:

```rust
(0x8, _, _, 0x0..=0x7 | 0xE) => Ok(Instruction::Logical { x, y, n }),
```

Then delete the reject arms. Anything not listed falls through to your final `_ => Err(UnknownOpcode { opcode })`. This is better for three reasons:

- **It's safe by default.** If you forget a value, the opcode is rejected rather than accepted by mistake. That's exactly the bug the reject list had.
- **It's one arm instead of two or more**, and arm order no longer matters. With a reject list, those arms must come *before* the general `(0x8, _, _, _)` arm. `match` tries arms top to bottom, so if you swap them, the reject arms can never match. The compiler does warn about unreachable patterns, but only if you look at the warnings.
- **Every `Instruction::Logical` is valid by construction.** The `_ =>` arm in `logic_instruction` can then become `unreachable!()`. That also removes the code that rebuilds the opcode for the error message.

**Speed is the same.** The compiler turns a `match` on small integer nibbles into a jump table or a short series of comparisons, whether you list ranges or single values. Range patterns cost nothing at runtime, so choose based on what's clearest and safest.

The same idea fits your `5XY0`/`9XY0` arms, which already match the valid value: `(0x5, _, _, 0x0)`.

### 🧑 David

I fixed it, review my changes

> 🔧 **Bash** — Show changed functions, run tests and clippy
> 🔧 **Bash** — Inspect remaining FX29 failure and LoadMemory

### 🤖 Claude

# Review of your fixes

Tests: **140 pass, 1 fail**, and the failure is my mistake, not yours (see the end).

## `IndexToHigh { max: REGISTER_SIZE }` ✅
This is correct and now matches the doc ("valid numbers are `0..max`").

## `FX33` (`put_hex_to_pseudo_dec_in_memory`) ✅ Good rewrite
- It's correct: three digits, leading zeros included, written to memory.
- **Why it's faster than the recursive version:**
  - Dividing by a constant (`/100`, `/10`, `%10`) compiles to a multiply and a shift, with no `div` instruction.
  - There's no recursion, so no stack frames.
  - It can't fail, so the `Result` and `?` overhead is gone.
- One gap: if `I` is above `0xFFD`, `self.i + 2` indexes past the end of memory and panics. `FX55` and `FX65` now check for this, so `FX33` should do the same, for consistency.
- The name is out of date now: it's ordinary BCD, so something like `store_bcd` describes it better.

## `FX29` ✅ Logic correct
- Reading VX instead of using `x` is the right fix.
- One edge case: if VX is above `0xF`, `FONT_START + vx * 5` points past the font, into program memory. Real interpreters only use the low nibble: `vx & 0x0F`. That's one AND instruction, and it keeps I inside the font.

## `FX55` / `FX65` bounds check ✅ Works, ⚠️ wrong error variant
- The condition `i + x >= MEMORY_SIZE` is exactly right: the last address you touch is `i + x`, so it must be at most `0xFFF`.
- But `IndexToHigh` is documented as "a register number is outside `V0`–`VF`". You're now also using it for a **memory address**, so the error message will mislead whoever reads it.
- A separate variant would be clearer, for example `AddressOutOfRange { address, max }`. Matching on an enum costs nothing at runtime, so it's purely a gain in clarity.

## `8XYN` decode ✅ Works
- `(0x8, _, _, 0x8..=0xD | 0xF)` comes before `(0x8, _, _, _)`, so the order is right.
- You chose to list the invalid values. The alternative was one arm listing the valid ones, `0x0..=0x7 | 0xE`. Both produce the same code, so it's a style choice.
- **Follow-up:** the `_ =>` arm in `logic_instruction` can no longer be reached. You can replace it with `unreachable!("filtered by decode")` and drop the code that rebuilds the opcode.

## The remaining failure is my test's fault
`exec_fx29_points_index_at_digit_in_vx` expects `[F0 90 F0 90 F0]` for digit `A`, but those are the bytes for `8`. The real `A` sprite is `F0 90 F0 90 90`, and your font has it right. The fix is to compare against `FONT[0xA * 5..0xA * 5 + 5]` instead of hard-coded bytes, which is what `exec_fx29_covers_all_digits` already does. Your CLAUDE.md says I only edit `src/` when you ask: do you want me to fix it, or would you rather do it yourself?

### 🧑 David

yes fix the test

> 🔧 **Read** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Bash** — Format and run the test suite

### 🤖 Claude

I fixed the test, and all 141 unit tests now pass, plus the 3 doc tests.

`exec_fx29_points_index_at_digit_in_vx` used to compare against hard-coded bytes, which were actually the sprite for `8`, not `A`. It now compares the memory at `I` against `FONT[0xA * 5..0xA * 5 + 5]`, so it checks against your real font table. I also ran `cargo fmt`, and it only changed this file.
