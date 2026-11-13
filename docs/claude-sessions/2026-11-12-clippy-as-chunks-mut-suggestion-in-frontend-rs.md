# Clippy as_chunks_mut suggestion in frontend.rs

*Claude Code session, 2026-11-12. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

@src/frontend.rs#L33-45 Cargo clippy tell me to use as_chunks_mut for better type safety. How to add bits of new_buffer ?

### 🤖 Claude

The problem is not `new_buffer`. It's what `as_chunks_mut` returns.

## What `as_chunks_mut` returns

```rust
pub fn as_chunks_mut<const N: usize>(&mut self) -> (&mut [[T; N]], &mut [T])
```

It returns a **tuple**:
- **First element:** a slice of full chunks. Each chunk is a `[T; N]`, so here each one is a row of `WIDTH` pixels.
- **Second element:** the leftover elements that don't fill a whole chunk.

A tuple isn't an iterator, so `.zip(...)` can't be called on it. You need to take out the chunk slice first and iterate over that. There are two common ways:

- **Destructure it:** `let (rows, rest) = self.buffer.as_chunks_mut::<WIDTH>();` Then you can add `debug_assert!(rest.is_empty())` to record that the buffer size is a multiple of `WIDTH`.
- **Index the tuple:** take `.0` and call `.iter_mut()` on it.

Then `rows.iter_mut().zip(new_buffer)` works. `new_buffer` is a `&[u64; 32]`, which implements `IntoIterator<Item = &u64>`, so `bits` is a `&u64`. `bits >> n` still compiles because the shift operators are implemented for references to integers too.

## Why clippy suggests it

Each `row` is a `&mut [u32; WIDTH]`, not a `&mut [u32]`, so its length is part of the type and known at compile time. When you index or iterate it with `0..WIDTH`, the compiler can prove every access is in bounds and **remove the bounds checks**. Your buffer is a fixed-size array, so the remainder is also known to be empty at compile time and costs nothing.

## Other things in this function

1. **`zip(0..WIDTH)` → `.enumerate()`.** It gives the same `(index, item)` pairs and states your intent more clearly. The generated code is identical.
2. **`color` is a misleading name.** It's the column index (x), not a color.
3. **`63 - color` silently assumes `WIDTH == 64`.** That's fine for CHIP-8, but tie it to the constant, or add a `const` assertion, so a later SUPER-CHIP 128-wide mode doesn't break it without warning.
4. **The `if bit == 1` branch is fine.** In release builds LLVM turns it into a branchless select (`cmov`), so you won't get branch mispredictions on a random pixel pattern. You could write it as `*pixel = if … { ON } else { OFF };`, which is more idiomatic but compiles to the same code.
5. **Another layout to consider:** store `buffer` as `[[u32; WIDTH]; HEIGHT]` from the start. Then `iter_mut()` gives you rows directly, with no chunking and no remainder. When minifb needs the flat `&[u32]`, call `self.buffer.as_flattened()`, which costs nothing because the memory layout is identical. The structure would then be part of the type itself instead of being added back each frame.

### 🧑 David

How to use as_flattened

### 🤖 Claude

`as_flattened` turns a slice of arrays into one flat slice:

```rust
impl<T, const N: usize> [[T; N]] {
    pub const fn as_flattened(&self) -> &[T];
    pub fn as_flattened_mut(&mut self) -> &mut [T];
}
```

It's been stable since Rust 1.80, so edition 2024 has it.

## How it applies to your buffer

1. **Declare the buffer as a 2D array:** `buffer: [[u32; WIDTH]; HEIGHT]`. The initializer is `[[OFF_COLOR; WIDTH]; HEIGHT]`. It's still one contiguous block of 2048 `u32`s on the stack or in the struct, with no `Vec` and no heap allocation.
2. **Calling it on an array works directly.** Method lookup coerces `[[u32; 64]; 32]` to `[[u32; 64]]`, so you write `self.buffer.as_flattened()` and get a `&[u32]` of length `WIDTH * HEIGHT`.
3. **Use it only where a flat slice is needed,** which is minifb's `update_with_buffer`. It takes a `&[u32]` plus the width and height, so you pass `self.buffer.as_flattened()` as the first argument.
4. **Elsewhere, use the nested structure directly.** In `update_buffer`, `self.buffer.iter_mut()` gives `&mut [u32; WIDTH]` rows. You can zip those with `new_buffer` without `as_chunks_mut` or a remainder. You can also index a pixel as `buffer[y][x]`.

## Why it's free

- **Same memory, new view.** `[[T; N]; M]` is laid out exactly like `[T; N * M]`. Rust guarantees there's no padding between array elements. So `as_flattened` only builds a new fat pointer (address plus length `N * M`). It copies nothing and allocates nothing. In release builds it compiles to almost nothing. The length is a multiplication of constants, which gets folded at compile time.
- **No runtime checks.** `as_chunks` has to compute a remainder. Going the other way, flattening, can't fail, so it needs no assert and returns no tuple.
- **Fewer bounds checks.** With `buffer[y][x]`, each index is checked against a constant length. That's cheaper than computing `y * WIDTH + x` on a flat slice, and the compiler can often remove those checks entirely when `x` and `y` come from bounded loops.

The trade-off: if the window size ever had to change at runtime, you'd need a `Vec<u32>` and manual `y * width + x` indexing. CHIP-8 has a fixed 64×32 display, so the fixed 2D array is the better fit.

### 🧑 David

review my update_buffer now

> 🔧 **Bash** — Show frontend file and display usages in chip8
> 🔧 **Bash** — Show the sprite drawing code
> 🔧 **Bash** — Show the function above draw

### 🤖 Claude

The function works, and the indexing (`WIDTH - 1 - x`, the select) is correct. The main problem is a leftover round-trip.

## 1. You flatten the buffer and then re-chunk it

```rust
self.buffer.as_flattened_mut().as_chunks_mut::<WIDTH>().0.iter_mut()
```

`self.buffer` is already `[[u32; WIDTH]; HEIGHT]`. Flattening it to `&mut [u32]` and then cutting it back into `[u32; WIDTH]` chunks gets you back to where you started. You also bring back the `.0` remainder tuple that the 2D layout was supposed to remove.

`self.buffer.iter_mut()` already gives `&mut [u32; WIDTH]` rows. The optimizer probably compiles both versions to the same code, so this isn't a performance bug. It's a readability one: a reader has to work out that the round-trip does nothing. Keep `as_flattened` for the one place that needs a flat slice, which is `draw`, and you already do that correctly.

## 2. `&[u64; 32]` uses a literal

Write `&[u64; HEIGHT]` so the signature uses the same constant as the buffer. If the two ever disagree, `zip` would silently stop at the shorter one, while a type mismatch fails at compile time. The same goes for `display: [u64; 32]` and the `% 64` / `% 32` / `>= 32` in `chip8.rs`. Those constants should probably live in one place shared by the core and the frontend.

## 3. Make the "one row = one u64" assumption checked

The code is only correct if `WIDTH == 64`. A compile-time assertion such as `const _: () = assert!(WIDTH == u64::BITS as usize);` makes that assumption checked instead of implied. It costs nothing at runtime.

## 4. Minor points

- `let bit = ...; if bit == 1` could be `bits & (1 << (WIDTH - 1 - x)) != 0`. That's one fewer named temporary and generates the same code. It's a matter of taste.
- `bits` is a `&u64` here, and the shift works through the reference impls. That's fine, and you don't need to dereference it.

## Separate bug in `chip8.rs:206` (not part of the review, but it will bite you)

`self.register[index] += value;` is presumably your `7XNN`. On CHIP-8, `7XNN` **wraps** on overflow (for example `0xFF + 2 = 0x01`) and does **not** touch VF. Rust's `+=` on `u8` **panics** on overflow in debug builds and wraps silently in release builds. So test ROMs will crash under `cargo run` but pass under `--release`. Look at `u8::wrapping_add`; it's the same instruction in both build modes, so it doesn't cost anything.
