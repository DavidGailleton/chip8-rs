# Keyboard scancode with minifb

*Claude Code session, 2026-11-13. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

How to use keyboard scancode with minifb

> 🔧 **Bash** — Check minifb version, frontend code, and crate source
> 🔧 **Bash** — Inspect how minifb maps keys per backend
> 🔧 **Bash** — Read key-mapping code in X11, Wayland, Windows backends

### 🤖 Claude

minifb has no scancode API. `Key` is the only key type it exposes, and on Linux it is tied to your keyboard layout. I checked the minifb 0.29 source in `~/.cargo/registry`:

| Backend | How it picks a `Key` | Follows the layout? |
|---|---|---|
| X11 (`os/posix/x11.rs:1136`) | `XLookupKeysym(ev, 0)`, the symbol the key types without Shift | yes |
| Wayland (`os/posix/wayland.rs:1318`) | xkb symbol at level 0 of the active layout | yes |
| Windows (`os/windows/mod.rs:242`) | real scancode (`lparam >> 16`) | no |

So the same physical key gives a different `Key` on AZERTY than on QWERTY. On AZERTY, the top row types `& é " '` without Shift, so `Key::Key1` to `Key::Key4` **never fire at all** on X11 or Wayland.

## Your options

1. **Stay on minifb and map by layout.** Translate the CHIP-8 pad positions into what your layout types. On AZERTY the left block types `& é " '` / `A Z E R` / `Q S D F` / `W X C V`. minifb has variants for some of these symbols (`Key::Apostrophe` and so on) but not for `é`, so a full AZERTY mapping isn't possible. You could make the key map a config table, but you'd still be working against the library.
2. **Switch the window crate to get physical keys.** This is the proper fix if you want to use scancodes:
   - **winit**: `KeyEvent::physical_key` is `PhysicalKey::Code(KeyCode::KeyQ)`, which means "the key in the Q position on a US keyboard", whatever your layout. You'd pair it with `softbuffer` or `pixels` to draw your `u32` buffer. It's more setup than minifb because it uses an event-loop/callback style.
   - **sdl2**: `Event::KeyDown { scancode: Some(Scancode::Q), .. }`, or the keyboard state polled by scancode. Polling fits an emulator loop well, but it needs the SDL C library (another devShell entry on NixOS).

For a CHIP-8 learning project I'd pick option 2 with winit if layout independence matters to you. Otherwise use option 1 with your own layout's keys.

## Problems in your current `get_key_pressed` (`src/frontend.rs`)

- **It panics when no key is held.** `get_keys()` returns an empty `Vec` then, so `[0]` is out of bounds. Use `.first()` and get an `Option` back.
- **It only reports one key.** CHIP-8 needs the state of all 16 keys. `EX9E`/`EXA1` ask whether a *specific* key is down, and several can be down at once. `FX0A` waits for a key press *and release*. See `CHIP-8-Quirks.md` for that one.
- **It allocates on every call.** `get_keys()` builds a new `Vec` each time. If you call it per instruction (~700/s), that's a heap allocation per instruction for nothing.

**The efficient design:** read the keyboard once per frame (60 Hz, right after `update_with_buffer`, which is when minifb processes events). Pack the 16 keys into a `u16` bitmask and give that to the CPU.
- `EX9E` then becomes `(keys >> vx) & 1`: one shift and one AND, no allocation, no search.
- A `u16` is `Copy` and fits in a register, so passing it costs nothing. A `[bool; 16]` takes 16 bytes and a `Vec<Key>` lives on the heap.
- `FX0A`'s "pressed then released" becomes a bit operation: `prev & !current` gives the keys released this frame.
- With minifb you'd build the mask from `is_key_down(key)` over a fixed `[Key; 16]` table indexed by CHIP-8 key. That's 16 lookups per frame, with no `Vec`.

The CPU only ever sees a `u16`, so this design stays the same whichever crate you pick. Switching to winit or SDL later only changes how the frontend builds the mask.

### 🧑 David

I will choose option 1 for now. set all key layout for qwerty with some.

### 🧑 David

I will choose option 1 for now. set all key layout for qwerty with some. In get_key_pressed function.

> 🔧 **Edit** — ~/Documents/chip8-rs/src/frontend.rs
> 🔧 **Bash** — Build to check the edit compiles

### 🤖 Claude

The file has changed since I last read it. I'll look at it again.

> 🔧 **Bash** — Show current function and build errors

### 🤖 Claude

You've already started switching to `.first()`. I'll finish the function in that direction.

> 🔧 **Edit** — ~/Documents/chip8-rs/src/frontend.rs
> 🔧 **Bash** — Rebuild and list remaining errors

### 🤖 Claude

I filled in `get_key_pressed` in `src/frontend.rs` with all 16 keys, using the usual QWERTY layout:

```
CHIP-8 keypad      QWERTY keys
1 2 3 C            1 2 3 4
4 5 6 D            Q W E R
7 8 9 E            A S D F
A 0 B F            Z X C V
```

You had already switched to `.first()`, and that change didn't compile yet. Two Rust points explain the fix:

- **`.first()?`** gives an `Option<&Key>`. The `?` returns `None` from the function right away when no key is held. It replaces the `[0]` that could panic, and you don't need to write `Some(...)` around every pattern.
- **Matching `&Key` against `Key::Q`** works because of match ergonomics (default binding modes). The compiler automatically matches through the reference, so you don't need `&Key::Q` or `*`.

`frontend.rs` now compiles, but the crate still doesn't build because of your unfinished work in `chip8.rs`. I didn't touch those parts:

- `src/chip8.rs:505`: the `match instruction` doesn't handle `SkipIfPressed` / `SkipIfNotPressed` yet.
- `src/chip8.rs:8`: `use minifb::Key::I;` is unused. It was probably added by mistake through IDE auto-import.

The earlier limitations still apply: this function reports only one key at a time, and `get_keys()` creates a new `Vec` on every call. So call it once per frame, not once per instruction.

### 🧑 David

@src/chip8.rs#L558-563 What is the best option to extract value of an option and continue if None ?

> 🔧 **Bash** — Find key_pressed and register field types
> 🔧 **Bash** — Show the variant definitions and their decoding
> 🔧 **Bash** — Show how skip instructions are decoded

### 🤖 Claude

The best option here is to not extract the value at all. Compare against an `Option` directly:

```rust
self.key_pressed == Some(value)   // EX9E: skip if pressed
self.key_pressed != Some(value)   // EXA1: skip if not pressed
```

`Option<u8>` implements `PartialEq`, so `None == Some(5)` is just `false`. There's nothing to unwrap and no `None` case to handle separately.

### Why it beats the other options

| Tool | When to use it | Problem here |
|---|---|---|
| `opt == Some(v)` | comparing with a known value | none: it's the best fit |
| `let Some(k) = opt else { return Ok(()); };` (let-else) | you need `k` for later work, and `None` means stop early | **wrong for `SkipIfNotPressed`**: no key held means "not pressed", so you *must* skip, but let-else would return without skipping |
| `opt.is_some_and(\|k\| k == v)` | the test is more than equality (ranges and so on) | just a longer way to write `== Some(v)` |
| `opt.unwrap_or(0xFF) == v` | a fallback value that has real meaning | `0xFF` is a magic "no key" value; it works only because keys are `0..=0xF`, and it hides intent |
| `?` | the function returns `Option` | `execute` returns `Result`, so `?` on an `Option` doesn't compile without `.ok_or(...)`, and it would mean "error" instead of "no key" |

On cost: `Option<u8>` is 2 bytes, a tag plus the value. `== Some(v)` compiles to two byte compares, or one 16-bit compare. That's as cheap as the `unwrap_or` version and safer.

For "extract the value, or continue if `None`" in general, **let-else** is the idiomatic answer. Use it when `None` really means there's nothing more to do.

### A bug to fix before this works

At `src/chip8.rs:559`, `x` in `EX9E`/`EXA1` is a **register number**, not a key. The spec says "skip if the key whose value is in **VX** is pressed". So you need to compare against the value stored in register `x`, not `x` itself.

Your other skip instructions take a different approach (see the doc comment at line 106): they read the register values at decode time. If you do the same here, the variant would carry the key value instead of `x`, and `execute` wouldn't need the registers. Either works, but stay consistent.

Also, these two variants aren't decoded yet: there's no `0xE` arm in the decode `match` for `EX9E`/`EXA1`. Line 565 is still incomplete, and that's why the crate doesn't compile right now.
