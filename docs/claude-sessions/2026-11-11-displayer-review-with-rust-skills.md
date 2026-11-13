# Displayer review with rust-skills

*Claude Code session, 2026-11-11. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

`/rust-skills` Review my displayer

> 🔧 **Bash** — List and print source files
> 🔧 **Bash** — Run pedantic clippy on frontend

### 🤖 Claude

I reviewed `src/frontend.rs`. Two bugs in `update_buffer` are why the window stays black. Your `[u64; 32]` display in the core is a good choice: one row is one register, and drawing a sprite is a shift plus an XOR. The problems are only in turning those bits into pixels.

## Bugs (these explain the black screen)

**1. Each row is cut to the wrong length (`frontend.rs:34`)**
`chunks_exact_mut(HEIGHT)` splits the buffer into rows of 32 pixels, but a row is 64 pixels wide, so it should split by `WIDTH`. As written you get 64 chunks of 32. The `zip` with 32 display rows stops after 32 of them, so only the top half of the buffer is written, and each display row lands in a half-width strip. Using the size constants by name prevents this kind of mix-up.

**2. The shift never uses the column (`frontend.rs:36`)**
`(bits >> 1) & 1` reads bit 1 for every pixel in the row. That bit is screen column 62, and the IBM logo never draws there, so every pixel is `OFF_COLOR`. The compiler pointed at this already with the warning that `color` is unused: that variable is your column index, and it's never read.

Work out the mapping from your `draw`: `(sprite << 56) >> col` puts screen column 0 at bit 63, so column `c` is at bit `63 - c`. The shift amount has to depend on the column.

Two related fixes:
- `color` is a misleading name for a column index.
- `.zip(0..WIDTH)` is a hand-written `.enumerate()`.

A tidier way to read the bits: start from the MSB (most significant bit) and shift the row left by one per pixel. Then the shift amount is always 1 and you don't need the column index at all. A fixed shift costs less than a variable one, though here the difference is tiny.

## Design and performance

**3. Use `[u32; WIDTH * HEIGHT]` instead of `Vec<u32>` for the buffer.** The size is fixed when you compile. A plain array avoids a heap allocation and an extra pointer to follow, and the length is part of the type. When the compiler knows the length, it can remove the bounds checks in the loop and may unroll or vectorize it (`opt-bounds-check`, `mem-boxed-slice`). The array is 8 KiB, which is fine to keep inside the struct. `update_with_buffer` takes `&[u32]`, and an array turns into that slice automatically.

Clippy suggests `as_chunks_mut::<N>()`, which returns `&mut [[u32; N]]`. Then each row has a known size too, so the inner loop also loses its bounds checks. That's the clippy suggestion with `WIDTH` in place of `HEIGHT`.

**4. Keep the display size in one place.** `update_buffer(&[u64; 32])` repeats 32, and the core repeats `32` and `64` in `draw` and `new`. Define `DISPLAY_WIDTH`/`DISPLAY_HEIGHT` once in `chip8.rs` and have the frontend use those. Then bug #1 couldn't happen, because both sides would read the same constant.

**5. The `if bit == 1 { ON } else { OFF }` branch is fine.** LLVM compiles a two-value select like this into a `cmov`, so there's no branch to mispredict. Keep the readable version and don't write bit tricks by hand (`anti-premature-optimize`).

**6. Only redraw when the screen changed.** Right now `main` rebuilds the buffer after every instruction. Only `00E0` and `DXYN` change the display. A `display_changed: bool` flag in `Chip8`, set by those two instructions and cleared by the frontend, skips the 2048-pixel conversion on most steps. This doesn't matter much today, but it will once you run about 700 instructions per second.

## Encapsulation

**7. The struct fields are `pub`.** `pub window` exists only so `main` can call `is_open()`. Add an `is_open(&self) -> bool` method and make the field private. Do the same for `Chip8::display`: expose `fn display(&self) -> &[u64; 32]` so the frontend can't change the machine's state.

## Not the display, but you'll hit these next

- **Timing (`main.rs:19`).** With `set_target_fps(60)`, `draw()` blocks until the next frame, so you run one instruction per frame, which is 60 instructions per second. The CPU should run at about 700 per second. That's fine for the IBM logo, which is about 20 instructions, but most ROMs will run very slowly. The usual approach is about 11–12 `step()` calls per frame, then one timer tick, then one draw.
- **Overflow panic (`chip8.rs:206`).** `self.register[index] += value` panics on overflow in debug builds. 7XNN has to wrap, so use `wrapping_add` (`num-overflow-explicit`).
- **Out-of-bounds read (`chip8.rs:216`).** `&self.memory[self.i..self.i + n]` panics if `I + N` goes past 4096.

## Lint

- `0x00FF_FFFF`: clippy pedantic asks for digit separators (`unreadable_literal`).
- `new` and `draw` are public and return `Result`, so they need a `# Errors` doc section (`missing_errors_doc`).

Fixing #1 and #2 should be enough to get the IBM logo on screen. Do those first, then #3 and #4.
