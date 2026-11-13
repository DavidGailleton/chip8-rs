# Display setup in rust-skills

*Claude Code session, 2026-11-11. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

`/clear`

### 🧑 David

`/rust-skills` How to setup the display ?

> 🔧 **Bash** — Show Cargo.toml and all source files

### 🤖 Claude

Your `Chip8` docstring already gives the right split: **the core holds the pixels, the binary shows them**. So the display has two parts.

## 1. The framebuffer, in `chip8.rs` (no I/O)

You need a 64×32 grid of on/off pixels plus a way to read it from outside. There are two reasonable layouts:

| Layout | Size | DXYN cost per sprite row | Notes |
|---|---|---|---|
| `[bool; 64 * 32]` (or `[[bool; 64]; 32]`) | 2048 B | 8 reads, 8 XORs, 8 branches | Simplest. One byte per pixel, index is `y * 64 + x`. |
| `[u64; 32]`, one `u64` per row | 256 B | **1 XOR, 1 AND** | The screen is exactly 64 wide, so a row fits in one register. |

**I recommend `[u64; 32]`.** CHIP-8 sprites are 8 pixels wide, one byte per row. If you put that byte at the top of a `u64` (bit 63 = column 0) and shift it right by `x`:
- **XOR-drawing** the whole row is a single `^=`.
- **Collision** (VF) is `(row & sprite_mask) != 0`: did any bit that was on get turned off? It's one AND per row, with no branch per pixel.
- **Clipping at the right edge comes free.** Bits shifted past bit 0 just fall off. That's the modern quirk (see `CHIP-8-Quirks.md`): the start coordinate wraps, pixels past the edge are clipped. If you ever want wrapping instead, `rotate_right` gives you that.

The whole screen also fits in 4 cache lines, compared with 32 for the bool version. At 700 instructions per second that won't show up in a benchmark. The real gain is that the code matches what the hardware does: the COSMAC VIP drew in bytes, not pixels. The cost is that reading one pixel takes a shift and a mask instead of a plain index. That only happens in the renderer, 2048 times per frame, which costs nothing.

**DXYN algorithm (spec details that are easy to get wrong):**
1. Read `VX` and `VY` **before** touching VF, because X or Y can be `F`.
2. Wrap the start: `x = VX % 64`, `y = VY % 32`.
3. Set `VF = 0`.
4. For each of the `n` rows: stop once `y + row >= 32` (clip at the bottom). Read the sprite byte at `memory[I + row]`, build the mask, set VF to 1 on collision, then XOR.

**API for the front-end:** expose something read-only, like `pub fn display(&self) -> &[u64; 32]` or `pub fn pixel(&self, x, y) -> bool`. Returning a reference means nothing gets copied. Also consider a "screen changed" flag that `00E0`/`DXYN` set and the front-end clears. Most frames don't draw anything, so you can skip the re-upload.

## 2. The window, in `main.rs`

The crate I'd pick for a learner is **`minifb`**:
- You hand it a `&[u32]` of pixels (`0x00RRGGBB`) plus a width and height. It scales the image up for you (`Scale::X16` turns 64×32 into a 1024×512 window).
- You keep control of the loop: `while window.is_open() { … window.update_with_buffer(&buf, 64, 32) }`. This is the big advantage over `winit`/`pixels`, where you hand the loop to an event-loop callback, which is awkward for an emulator you're just learning to write.
- It has `is_key_down` for the keypad later, and `set_target_fps(60)` gives you 60 Hz pacing.

Alternatives: `pixels` + `winit` renders on the GPU but needs much more boilerplate. `sdl2` gives you audio for the buzzer, but it depends on a C library.

**Gotcha on NixOS/Hyprland:** `minifb` loads `wayland`, `libxkbcommon` and `libX11` at runtime. Put them in a devShell (`buildInputs` plus `LD_LIBRARY_PATH`), or the window won't open.

**Converting framebuffer to pixels:** allocate one buffer of `64 * 32` `u32`s **once**, before the loop, and overwrite it each frame. Don't build a new one per frame (`mem-reuse-collections`). Walk each row's bits and write `ON_COLOR` or `OFF_COLOR`.

**Main loop shape, once per 60 Hz frame:**
1. Run about 11 or 12 `step()` calls (700 / 60).
2. Decrement the timers once.
3. If the screen changed, convert and `update_with_buffer`. Otherwise call `window.update()` so input keeps being polled.

For that to work, `step` (or a `run_frame`) has to become `pub`.

## Problems I found in your current code

- **`execute` won't compile.** The `match` has no arms for `ClearScreen` or `Draw`. Rust requires matches to be exhaustive, so the display work is what fills those two arms in.
- **`add_register` panics in debug builds.** `+=` panics on `u8` overflow there, and 7XNN must wrap. Use `wrapping_add`, which is free in release builds (no extra check).
- **The index checks in `set_register`/`add_register` can never fail.** `x` is a nibble, so it's always ≤ 0xF. You could drop `IndexToHigh` and index directly. The compiler often can't remove the bounds check because it doesn't know `x < 16`, but `& 0xF` or a `usize::from(x)` into a `[u8; 16]` makes that clear to it. (Small spelling note: it's "TooHigh".)

Once you've written the framebuffer and DXYN, send it over and I'll review it.

### 🧑 David

Should I put the Window management of minifb in an other file ?

### 🤖 Claude

Yes. Put it in its own module **in the binary crate**, not in the library.

```
src/
├── lib.rs        // library crate root: mod chip8;
├── chip8.rs      // core: memory, CPU, framebuffer. No minifb.
├── main.rs       // binary crate root: mod frontend; plus the main loop
└── frontend.rs   // minifb: Window, pixel buffer, keys later
```

The important detail is **where you write the `mod` line**. `main.rs` and `lib.rs` are the roots of two separate crates that share one package. A `mod frontend;` in `main.rs` makes `frontend.rs` part of the binary only. If you put it in `lib.rs`, the library would depend on minifb.

**Why this split is right:**
- **It follows dependencies, not file length.** Keep `chip8.rs` free of any windowing code. Then `cargo test` builds and runs the core without a display, which also works headless in CI. You could later add a second front-end (a terminal renderer, WASM, or a test harness that compares framebuffers) without touching the core.
- **`main.rs` stays short.** It parses arguments, loads the ROM, and runs the frame loop. `frontend.rs` handles how pixels get onto the screen.
- **Later, minifb can become optional.** Because only the binary uses it, you can make it an optional Cargo feature. People who only use the library then don't download or compile it.

**What goes in `frontend.rs`:** a struct that owns the `Window` and the `Vec<u32>` pixel buffer, allocated once in `new` and reused every frame. Give it a few methods: `new()`, `is_open()`, `render(&[u64; 32])`, and later something like `pressed_keys() -> u16`. minifb handles both the window and the keyboard, so keep both in this one file. Splitting it into `display.rs` + `input.rs` would be over-splitting for a project this size (`proj-flat-small`).

**Naming gotcha:** don't call the module `minifb.rs`. `use minifb::Window;` would then refer to your module instead of the crate, unless you write `::minifb::…`. Name the module after its role (`frontend`, `screen`), not after the library it wraps.

**Performance cost: none.** A module only organizes code; inside one crate the compiler can still inline across files freely. The crate boundary between `lib` and `main` is a slightly different case. In release builds, small non-generic functions are still inlined automatically across crates. Marking the hot accessor `#[inline]`, or turning on `lto = "fat"`, removes any remaining doubt. And at 60 frames per second, a single `render` call per frame is not a hot path anyway.

### 🧑 David

How to convert u64 in 64 size array.

> 🔧 **Bash** — Show frontend module and framebuffer references

### 🤖 Claude

I read `src/frontend.rs`. Before the conversion itself, three things there will bite you:

1. **`buffer` must be `Vec<u32>`, not `Vec<u8>`.** `update_with_buffer` takes `&[u32]`, one `0x00RRGGBB` value per pixel.
2. **Take `&[u64; 32]`, not `[u64; 32]`.** Passing by value copies 256 bytes on every call. That's cheap, but you only need to read the data, so borrow it. It also pairs with the `pub fn display(&self) -> &[u64; 32]` accessor on `Chip8`.
3. **`WindowOptions::default()` uses `Scale::X1`, so the window is 64×32 real pixels, about the size of a stamp.** Use `WindowOptions { scale: Scale::X16, ..WindowOptions::default() }`.

## Reading bit `x` of a row

With bit 63 as column 0 (the layout from before), column `x` is at bit `63 - x`:

```rust
let on = (row >> (63 - x)) & 1 == 1;
```

To shift right, move the wanted bit down to position 0, then `& 1` keeps only that bit. Another way to write it is `row & (1 << (63 - x)) != 0`, which compiles to the same code.

## Walking the whole screen without manual indexing

Don't keep a separate counter like your `buf_line`. Let iterators pair each `u64` row with its 64-pixel slice of the buffer:

- `self.buffer.chunks_exact_mut(WIDTH)` gives 32 mutable slices of 64 `u32`, one per screen row.
- `.zip(rows)` pairs each slice with its `u64`.
- Inside, `slice.iter_mut().enumerate()` gives you `(x, pixel)` for that row's pixels.

This is better than `buffer[y * 64 + x]` for three reasons:
- **No bounds checks.** With manual indexing, the compiler can't always prove `y * 64 + x < 2048`, so it keeps a check and a panic branch on every access. With `chunks_exact_mut`, the chunk length is known to be exactly 64, so the inner loop has no checks and can be vectorized (`opt-bounds-check`, `perf-iter-over-index`).
- **No running index to get wrong.** Your `buf_line` counter is exactly the kind of state that drifts out of sync.
- **`chunks_exact` beats `chunks`.** It promises every chunk is full length, and puts any leftover in a separate `remainder()`. That promise is what lets the compiler drop the checks.

## Choosing the color

Use `*pixel = if on { ON_COLOR } else { OFF_COLOR };

### 🧑 David

Explain me more how insert the new_buffer in buffer

### 🤖 Claude

## Why you can't just copy it

The two buffers hold the same picture in different formats:

| | `new_buffer: &[u64; 32]` | `self.buffer: Vec<u32>` |
|---|---|---|
| Number of elements | 32, one per **row** | 2048, one per **pixel** |
| One pixel is | 1 **bit** | 1 whole `u32` (a color) |
| Size | 256 bytes | 8 KiB |

A `copy_from_slice` or `self.buffer = …` won't work, because the element types and counts don't match. You have to **expand** the data: each of the 2048 bits becomes one `u32` color in the buffer.

## Where each pixel goes

`self.buffer` is **row-major**: row 0's 64 pixels come first, then row 1's, and so on.

```
buffer index:  0 .. 63 | 64 .. 127 | 128 .. 191 | ... | 1984 .. 2047
screen row:      y=0   |    y=1    |    y=2     | ... |     y=31
```

So the pixel at column `x`, row `y` is stored at `buffer[y * 64 + x]`, and its source bit is bit `63 - x` of `new_buffer[y]`.

**Worked example:** `new_buffer[1] = 0x8000_0000_0000_0001`. Only bit 63 and bit 0 are set.
- Bit 63 is column `x = 0`, so `buffer[1*64 + 0]`, which is `buffer[64]`, gets `ON_COLOR`.
- Bit 0 is column `x = 63`, so `buffer[1*64 + 63]`, which is `buffer[127]`, gets `ON_COLOR`.
- The other 62 pixels, `buffer[65..=126]`, get `OFF_COLOR`.

## The algorithm

```
for each row y in 0..32:
    let row = new_buffer[y]
    for each column x in 0..64:
        bit   = (row >> (63 - x)) & 1
        color = ON_COLOR if bit is 1, else OFF_COLOR
        write color into buffer at y * 64 + x
```

That's all there is to it: two nested loops, a bit test, and a write. I'd write it this way first so you can see it work.

## Then make it idiomatic

When you write `buffer[y * 64 + x]`, Rust adds a bounds check, because it can't prove the index is under 2048. The iterator version removes that check and the index arithmetic:

1. **Split the buffer into rows.** `self.buffer.chunks_exact_mut(64)` yields 32 `&mut [u32]` slices, each one screen row, so you never compute `y * 64` yourself.
2. **Pair each row slice with its `u64`.** `.zip(new_buffer)` (or `new_buffer.iter()`) gives you `(row_pixels, &row_bits)` together, so you no longer need `y` at all.
3. **Walk the 64 pixels of that row.** `row_pixels.iter_mut().enumerate()` gives `(x, pixel)`, where `pixel` is a `&mut u32` you write through with `*pixel = color;`.

This needs no index variables, no `y * 64 + x`, and no bounds checks, and the compiler can vectorize the inner loop. It's the `perf-iter-over-index` rule in practice.

## Another option for the inner loop: shift instead of index

Instead of computing `63 - x` for each pixel, copy the row into a `mut` local and, for each pixel, test the **top** bit (`bits >> 63`), then do `bits <<= 1`. The next pixel's bit moves into position, so the loop doesn't need `x` at all and `enumerate()` goes away. Both versions are equally fast; pick whichever reads better to you.

## Last step

After filling the buffer, call `self.window.update_with_buffer(&self.buffer, WIDTH, HEIGHT)` with the same `self.buffer` every frame, so nothing gets allocated.

Write the nested-loop version first, check that the IBM logo shows up, then refactor it to the iterator version. I'm happy to review both.

### 🧑 David

Explain me how to implement the draw function in chip8

> 🔧 **Bash** — Show current Chip8 struct and execute function

### 🤖 Claude

## First, a design problem: `Chip8` now owns the window

`Chip8` has a `pub window: MfbWindow` field, and `ClearScreen` calls `self.window.update_buffer(...)`. That puts minifb back inside the core, which is the split we wanted to avoid. It also breaks DXYN specifically:

- **DXYN has to read the current pixels**, both for the XOR and to detect collisions. Your window buffer holds `u32` colors, so you would have to read colors back and compare them to `ON_COLOR`. The screen state would end up stored as colors.
- **`cargo test` would need a window** just to test one instruction.
- **You would redraw on every instruction**, possibly 12 times per frame, instead of once per frame.

**The fix:** `Chip8` owns the framebuffer, and the window only displays it.

```
Chip8                                   main.rs, once per frame
  display: [u64; 32]   <-- DXYN/00E0     window.update_buffer(chip8.display())
  pub fn display(&self) -> &[u64; 32]
```

Then `00E0` is just `self.display = [0; 32];`, which compiles to a single 256-byte memset, and `Chip8::new()` no longer takes a window. Remove the `window` field and the `frontend` import from `chip8.rs`/`lib.rs`. `main.rs` creates the window and calls `update_buffer` after running each frame's instructions.

## DXYN step by step

`Draw { x, y, n }` holds **register numbers**, not coordinates. The coordinates are the values in VX and VY.

**1. Read the coordinates first.**
Read `self.register[x]` and `self.register[y]` before anything writes to VF. If X or Y is `F`, writing VF first would change the coordinate you're about to use.

**2. Wrap the starting position.**
Use `x0 = vx % 64` and `y0 = vy % 32`. Since 64 and 32 are powers of two, `vx & 63` and `vy & 31` do the same thing. The compiler emits that AND for `%` anyway, because the values are unsigned.

**3. Track collision in a local `bool`, starting at `false`.**
Write it into VF once, at the end, with `u8::from(collided)`. That's one register write instead of one per row, and the loop doesn't depend on VF.

**4. Loop over the `n` sprite rows, `row in 0..n`:**
- Screen row: `y0 + row`. If it's `>= 32`, **stop**. That's clipping at the bottom; don't wrap.
- Sprite byte: `self.memory[(usize::from(self.i) + row) % MEMORY_SIZE]`. Use `usize` for the addition so `I + row` can't overflow a `u16`.
- Build the mask: put the byte in the top 8 bits of a `u64`, then shift it right to column `x0`:
  ```rust
  let mask = (u64::from(byte) << 56) >> x0;
  ```
- Collision: `if self.display[screen_y] & mask != 0`, a lit pixel is about to be turned off, so set `collided = true`. You can also write it without a branch: `collided |= self.display[screen_y] & mask != 0;`.
- Draw: `self.display[screen_y] ^= mask;`

**5. Set VF** from `collided`, and set a "screen changed" flag if you added one.

## How the mask works

```
byte        = 0b1111_0000
<< 56       = 1111_0000 0000…0000      columns 0..7 hold the sprite row
>> x0 (=2)  = 0011_1100 0000…0000      columns 2..9
>> x0 (=60) = 0000…0000 1111           columns 60..63; the rest of the byte is gone
```

When `x0 = 60`, the low four bits of the sprite are shifted past bit 0 and are dropped. **That's right-edge clipping with no extra code.** One `u64` operation draws a whole sprite row, compared with 8 bit tests on a `[bool; 2048]`.

## Common mistakes

| Mistake | Result |
|---|---|
| `byte << 56` while `byte` is still a `u8` | Compile error or overflow. Widen with `u64::from` first. `From` is lossless, so it's the right choice for widening instead of `as`. |
| Wrapping `y` on each row (`% 32`) | Sprites wrap to the top of the screen. Only the starting coordinate wraps. |
| Writing VF before reading VX/VY | Wrong position when X or Y is `F`. |
| Setting VF to 1 when a pixel turns **on** | VF must be 1 only when a lit pixel turns **off**. |
| `self.i + row as u16` | Can overflow. Do the addition in `usize`, then `% MEMORY_SIZE`. |

## Tests to write (they need no window once the display is in the core)

- Draw one sprite at (0, 0): `display[0]` has the byte in its top 8 bits, and VF is 0.
- Draw the same sprite twice: the screen is all zeros again, and VF is 1.
- Draw at x = 60: only the high 4 bits of the byte appear, in columns 60–63.
- Draw at y = 30 with n = 5: only rows 30–31 change.
- VX = 70: the sprite starts at column 6.
- Use VF as the X register: the position comes from VF's value **before** the draw.

After that, the IBM logo ROM should display. Send me your draw code once it's written and I'll review it.

### 🧑 David

I still don't understand

### 🤖 Claude

Let me start over with one concrete example and pictures.

## 1. What DXYN does, in plain words

**It copies a small picture from memory onto the screen.**

- The **picture** (the "sprite") is stored in memory starting at address `I`.
- **Where** to draw it is stored in two registers: VX for the column, VY for the row.
- **N** is how many rows tall the picture is.

## 2. What a sprite looks like in memory

Each byte is **one row of 8 pixels**: a 1 bit is a lit pixel, a 0 bit is dark.

Say memory holds these 3 bytes at `I = 0x300`:

```
address  byte   binary       on screen
0x300    0xF0   1111 0000    ████....
0x301    0x90   1001 0000    █..█....
0x302    0xF0   1111 0000    ████....
```

That's a 3-row picture of a small box.

## 3. What the screen looks like in your struct

`display: [u64; 32]` has 32 rows, and each row is one `u64` holding 64 pixels.
The **leftmost bit (bit 63) is column 0**, and the rightmost bit (bit 0) is column 63.

```
display[0] = row 0  -> 64 bits -> 64 pixels
display[1] = row 1
...
display[31] = row 31
```

## 4. The example instruction: `D013`

Split into nibbles: `D`, X = `0`, Y = `1`, N = `3`.

Assume V0 = 2 and V1 = 5. That means:
- "Draw the 3-row sprite stored at I"
- "at column V0 = **2**, row V1 = **5**"

So:
- sprite row 0 (`0xF0`) goes onto screen row **5**
- sprite row 1 (`0x90`) goes onto screen row **6**
- sprite row 2 (`0xF0`) goes onto screen row **7**

That's the loop: **one pass per sprite row**, `N` passes in total.

## 5. One pass: putting a byte at column 2 of a `u64`

Take sprite row 0, the byte `1111 0000`. It has to land in columns 2 to 9 of `display[5]`.

(Only the first 16 columns are shown; the other 48 are all 0.)

```
column:                  0123456789...

step A: u64::from(byte)  0000000000...0011110000   the byte sits at the RIGHT end (columns 56-63)
step B: << 56            1111000000...0000000000   moved to the LEFT end (columns 0-7)
step C: >> 2             0011110000...0000000000   moved right by 2, now at columns 2-9
```

- **Step A:** turn the `u8` into a `u64`. A `u64` has 64 bits, so the byte lands in the last 8.
- **Step B:** shift left by 56 (64 - 8) so it starts at column 0.
- **Step C:** shift right by the column you want (here 2).

The result is called the **mask**: a `u64` with 1s exactly where the sprite's lit pixels go.

## 6. Putting the mask on the screen: XOR

CHIP-8 doesn't just write pixels; it **flips** them. That's XOR (`^`):

```
screen pixel  sprite pixel  ->  result
     0             0              0    (unchanged)
     1             0              1    (unchanged)
     0             1              1    (turned ON)
     1             1              0    (turned OFF, a "collision")
```

`display[5] ^= mask;` does that for all 64 pixels of the row in **one operation**.

Why flip instead of write? Games erase a sprite by drawing it a second time in the same place. That's how things move on screen: draw, erase, then draw again at the new position.

## 7. Collision: setting VF

VF must be 1 if **any pixel went from ON to OFF**. Games use this to detect when two things touch.

A pixel turns off only where the screen **and** the mask both have a 1:

```
display[5] & mask   -> not 0?  then something was turned off -> collision
```

Check this **before** the XOR, because after the XOR those pixels are already 0.

## 8. The whole instruction, in words

```
column = V[X] % 64
row    = V[Y] % 32
collision = false

repeat for r = 0, 1, ..., N-1:
    if row + r >= 32: stop          (off the bottom of the screen)
    byte = memory[I + r]
    mask = (byte as u64 << 56) >> column
    if display[row + r] & mask != 0: collision = true
    display[row + r] ^= mask

V[F] = 1 if collision else 0
```

After the three passes in our example, the screen shows:

```
         0123456789
row 5    ..████....
row 6    ..█..█....
row 7    ..████....
```

---

Which step loses you: the sprite bytes (2), the shifting (5), XOR (6), or something else? Tell me and I'll go deeper on just that part.

### 🧑 David

What is v0 and v1 with x and y

### 🤖 Claude

## V0 to VF are your `register` array

CHIP-8 has 16 small storage slots, each holding one `u8`. They're named **V0, V1, … VF** (hex 0 to F). In your code they're this field:

```rust
register: [u8; REGISTER_SIZE],   // 16 slots
```

```
name   V0           V1           V2          ...  VF
code   register[0]  register[1]  register[2] ...  register[15]
```

So "V1" just means `self.register[1]`.

## X and Y are slot numbers, not coordinates

In `DXYN`, **X and Y say which slots to read**. The coordinates are the **values stored in those slots**.

Take `D013`:

```
D   0   1   3
    X   Y   N
    |   |
    |   +-- Y = 1 -> "the row is in slot V1"
    +------ X = 0 -> "the column is in slot V0"
```

So the CPU does:

```
column = register[0]   (the value in V0)
row    = register[1]   (the value in V1)
```

It's like an address on an envelope: X doesn't tell you *what* the column is, it tells you *where to look* for it.

## Where those values come from

Earlier instructions in the program put them there with `6XNN` ("set VX to NN"). For example:

```
6002   -> V0 = 0x02    register[0] = 2
6105   -> V1 = 0x05    register[1] = 5
A300   -> I  = 0x300   the sprite is at address 0x300
D013   -> draw 3 rows from I, at column V0 (= 2), row V1 (= 5)
```

So the sprite lands at column 2, row 5, because those are the **values** in V0 and V1.

## Why it works this way

Because the position is in a register, the program can change it. A game can run `7001` (V0 += 1) and then the same `D013` again, and the sprite moves one pixel to the right. If the coordinates were hard-coded in the instruction, nothing could move.

## In your code

Your decoded instruction is `Instruction::Draw { x, y, n }`:
- `x` and `y` are **slot numbers** (0 to 15), straight from the opcode.
- The coordinates are `self.register[x]` and `self.register[y]`.
- `n` really is a number: how many sprite rows to draw.

Mixing these up is the classic CHIP-8 bug. If you draw at `x` instead of `self.register[x]`, everything appears in the top-left corner of the screen.
