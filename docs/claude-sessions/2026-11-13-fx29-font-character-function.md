# FX29 font character function

*Claude Code session, 2026-11-13. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

Explain me the FX29 font character function

> 🔧 **Bash** — Look for font handling in code and wiki notes
> 🔧 **Bash** — Read font-related constants in chip8.rs

### 🤖 Claude

## FX29: point I at the font glyph for VX

### What it does
Set **I** to the memory address of the built-in sprite for the hex digit stored in **VX**. That's all it does. It draws nothing. The game normally runs `DXYN` right after it, with N = 5, to put the digit on screen:

```
F029    ; I = address of glyph for V0's value
D125    ; draw 5 rows from I at (V1, V2)
```

This is how games show scores. `FX33` (BCD) splits a number into decimal digits, `FX65` loads them into registers, then FX29 + DXYN draws each digit.

### Where the glyphs live
The interpreter (your code) has to put 16 glyphs, 0 to F, into RAM before the ROM runs. Each glyph is 4×5 pixels stored as **5 bytes**, one byte per row. Only the high 4 bits of each byte are used, because sprites are 8 pixels wide. For example, "0":

```
F0  1111....
90  1..1....
90  1..1....
90  1..1....
F0  1111....
```

16 glyphs × 5 bytes = 80 bytes = `0x50`. With the usual base address `0x050`, they fill `0x050..0x0A0`, which matches your doc comment at `src/chip8.rs:189`.

### The address formula
The glyphs are packed back to back, so the address is plain arithmetic:

```
I = FONT_START + (VX & 0x0F) * 5
```

- **`* 5`** because each glyph is 5 bytes. Digit 3 is at `0x050 + 15 = 0x05F`.
- **`& 0x0F`** because VX is a full byte but only 16 glyphs exist. The original COSMAC VIP used only the low nibble. If VX = `0x1A` and you skip the mask, I lands at `0x050 + 130`, which is past the font and inside whatever comes next. Masking matches original hardware and keeps I inside the font table, so you never need a bounds check or error path here.

### Optimization notes
- **No lookup table, no branch.** A `match` on 16 digits, or an array of 16 addresses, would compute something the hardware already gives you as one multiply-add. `x * 5` compiles to `lea` (`x + x*4`), which takes a single cycle. Since the glyphs are contiguous, the index is the offset.
- **Font as a `const` array.** Make the font data a `const [u8; 80]` and copy it into `memory[0x050..0x0A0]` once in `new()` with `copy_from_slice`. The size is known at compile time, so the copy becomes a single `memcpy` with no allocation and no per-element bounds checks. A `Vec` would allocate on the heap for data that never changes. Putting the font *inside* the emulated RAM, rather than in a separate Rust array, matters because DXYN reads sprites through I from memory. FX29 then just needs an address, and DXYN doesn't need a special case for font sprites.
- **Types.** VX is `u8` and I is `u16`. Widen *before* multiplying: `u16::from(vx & 0xF) * 5`. Multiplying as `u8` works here (15 × 5 = 75 still fits), but widening first is the habit that prevents silent overflow elsewhere. `u16::from` is also lossless and passes `clippy::pedantic`, unlike `as`.

### One thing I noticed
From a quick grep, your `chip8.rs` only *mentions* the font in a comment. I couldn't find the glyph data or the code that loads it. If that's right, FX29 would point I at zeros, and DXYN would draw nothing. So the first step is to load the font in `new()`, then implement FX29. A test ROM that shows digits, such as a score counter or corax89's test, will catch either bug.

The 80 bytes of font data are listed in the spec. The CHIP-8 wiki page has the first glyph, and the full table is in any reference linked from `CHIP-8-Resources.md`.

### 🧑 David

How to fill the font in new function on the init

> 🔧 **Bash** — Read the struct and new() function

### 🤖 Claude

There are three pieces: a constant holding the glyph data, a constant for the base address, and one copy into `memory` inside `new()`.

### 1. The data: a `const` array
```rust
const FONT_START: usize = 0x050;
const FONT: [u8; 80] = [
    0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
    // ... 15 more rows of 5 bytes, 1 to F
];
```
**Why `const` and not `static` or `Vec`:** the type `[u8; 80]` puts the length in the type, so the compiler checks you typed exactly 80 bytes. A `const` is inlined where it's used. Nothing is allocated and nothing exists at runtime beyond the bytes in the binary. A `Vec` would allocate on the heap on every `new()` for data that never changes. Use `usize` for `FONT_START` because it's only used to index `memory`, and your `i` is `usize` too.

### 2. The problem: `new()` builds `Self { ... }` in one expression
Each field in a struct literal is a single expression, so you can't write "zero this, then copy into it" inside it. You need a statement before or after the literal. There are two idiomatic ways:

- **Before:** build the array in a `let mut` local, copy the font into it, then pass it in with field-init shorthand (`Self { memory, program_counter: ..., }`).
- **After:** build `let mut chip8 = Self { ... };`, mutate `chip8.memory`, then return `chip8`.

Both compile to the same thing in release mode. I'd pick **before**, because the struct literal stays declarative and the value is never seen half-initialized.

### 3. The copy: a slice range plus `copy_from_slice`
Here's the general pattern, using different names so you can work it out for your code:
```rust
buf[START..START + DATA.len()].copy_from_slice(&DATA);
```
- `buf[a..b]` borrows a sub-slice of the array as `&mut [u8]`. Nothing is copied yet.
- `copy_from_slice` panics if the two lengths differ. Here both lengths are compile-time constants, so the optimizer proves they match. The check and the bounds check both disappear, and you get a single `memcpy` of 80 bytes.
- **Use `DATA.len()`, not a hardcoded end like `0x0A0`.** The range then depends on the data itself, so there's no magic number to keep in sync.

A manual `for` loop that indexes each byte also works. But each `buf[i]` keeps a bounds check unless the optimizer can remove it. `copy_from_slice` makes the intent explicit and reliably becomes a `memcpy`.

### On optimization here
This runs **once** per machine, so it's 80 bytes copied once. The cost is nothing either way, and you shouldn't make it more complicated. You *could* build the whole 4 KB initial memory as a `const` at compile time, but that puts 4 KB in the binary to save an 80-byte copy that already costs nothing. Save effort for the hot path: fetch, decode and `DXYN` run hundreds of times a second.

### Check it with a test
Write a test: `Chip8::new()`, then assert that `memory[FONT_START..FONT_START + 5]` equals the bytes for "0" (`F0 90 90 90 F0`). Also check that `memory[FONT_START + 75..FONT_START + 80]` is the "F" glyph. That catches a wrong offset or a missing row. Your tests are inside the module, so they can read the private `memory` field.

Once that passes, FX29 is the one-line address calculation from before, and `DXYN` draws the glyph with no special case.
