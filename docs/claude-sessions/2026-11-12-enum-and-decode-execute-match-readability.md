# Enum and decode/execute match readability

*Claude Code session, 2026-11-12. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

Refactor the order of my enum, and matchs for decode and execute to be more lisible

> 🔧 **Bash**
> 🔧 **Read** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Read** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Bash**

### 🤖 Claude

The reorder is done and builds. Now I'm checking whether that test failure was already there before my change.

> 🔧 **Bash**

### 🤖 Claude

I reordered the `Instruction` enum, the `decode` match and the `execute` match in `src/chip8.rs` so all three follow the opcode table order:

`00E0 → 00EE → 1NNN → 2NNN → 3XNN → 4XNN → 5XY0 → 6XNN → 7XNN → 9XY0 → ANNN → DXYN`

Now a given instruction sits in the same spot in all three places, and you can check them against the spec table from top to bottom. I didn't rename or change anything else. `cargo fmt --check` passes, and the change is 34 lines moved, none added or removed.

**Tests:** 32 pass and 1 fails, `decode_accepts_exactly_the_supported_opcodes`. It failed the same way before my change. The test still only allows `00E0` and the `1/6/7/A/D` families, so the opcodes you added (`00EE`, `2`, `3`, `4`, `5XY0`, `9XY0`) are flagged as "should be unknown". Its `supported` list needs updating.

While reading the code I found some bugs. I didn't fix them, since this is your code to work through:

1. **3XNN / 4XNN / 5XY0 / 9XY0 are inverted.** `SkipConditionAND` skips when `a != b`, but 3XNN and 5XY0 should skip when the values are *equal*. `SkipConditionXOR` has the same problem the other way round. Naming the variants by what they do (e.g. `SkipIfEqual` / `SkipIfNotEqual`) would make this kind of mistake easier to see. AND and XOR aren't what the instructions do.
2. **2NNN pushes the wrong address.** `CallSubroutine` pushes `nnn`, the subroutine's own address. It should push the return address, which is the current `program_counter`: fetch has already moved it past the call. As written, `00EE` jumps back to the start of the subroutine and loops forever.
3. **7XNN can panic.** `add_register` uses `+=`, which panics on overflow in debug builds. The spec says 7XNN wraps around, and the right tool for that is `u8::wrapping_add`.

Want me to save these bugs as a note in the vault so you can track them?

### 🧑 David

I fix them

> 🔧 **Bash**
> 🔧 **Bash**
> 🔧 **Bash**

### 🤖 Claude

Commit `c01b145` fixes the skip bug: 3XNN and 5XY0 now skip when the values are equal, and 4XNN and 9XY0 when they aren't.

Three things are still open in `src/chip8.rs`:

- **2NNN** (`chip8.rs:345`) still pushes `nnn`. It should push `self.program_counter`, which is the return address.
- **7XNN** (`chip8.rs:226`) still uses `+=`, so it panics on overflow in debug builds. It needs to wrap.
- **The opcode-coverage test** (`chip8.rs:740`) still fails. Its `supported` list is missing `00EE`, `2`, `3`, `4`, `5XY0` and `9XY0`.

You don't have tests for the skip, call and return instructions yet. A few `execute` tests would have caught both the skip inversion and the call bug.

### 🧑 David

Is there a way to convert usize to u8

### 🤖 Claude

Yes. There's no `From<usize> for u8`, because a `usize` can hold values a `u8` can't, so you have to pick how out-of-range values are handled:

```rust
let big: usize = 300;

let a = big as u8;                 // 44: keeps the low 8 bits, never fails
let b = u8::try_from(big);         // Err(TryFromIntError): checked, gives a Result
let c = u8::try_from(big).unwrap_or(u8::MAX); // 255: checked, falls back to a value you choose
```

**Trade-offs:**
- **`as`** costs nothing: the CPU just reads the low byte of the register. But if the value is too big, it silently gives you a wrong number. Clippy pedantic flags it (`cast_possible_truncation`), and you then add `#[allow(...)]` with a comment explaining why the value always fits.
- **`try_from`** adds one compare and branch. The CPU almost always predicts it right, so in practice it's nearly free, and you get an error instead of corrupted state. It fits naturally into your `Result<_, ExecutionError>` functions with `?` / `map_err`. Use it when you can't prove the value fits.
- **Convert less often.** Lots of casts usually mean a field has the wrong type. Pick the type that matches the hardware, so the conversions happen once, at the edges.

If this is for the 2NNN fix, note that the stack holds `u16` and `program_counter` is a `usize`, so the conversion you need is `usize → u16`, not `u8`. The same tools apply (`u16::try_from`, or `as u16`). The PC can never go past `0xFFF` because `pc_jump_to` wraps it with `% MEMORY_SIZE`, so `as u16` is safe there, as long as you write a comment saying why. Another option is to store the PC as a `u16` (12-bit addresses, like the real machine) and convert to `usize` only when indexing memory. That direction never loses data, so plain `usize::from` works.

### 🧑 David

:226

### 🧑 David

Make the project not panic on overflow

> 🔧 **Bash**
> 🔧 **Bash**
> 🔧 **Bash**

### 🤖 Claude

Integer overflow now wraps instead of panicking in every build. I added this to `Cargo.toml`:

```toml
[profile.dev]
overflow-checks = false
```

Release builds already wrap by default, and `cargo test` uses the dev settings, so dev, test and release all behave the same now. It builds, and the tests are unchanged: 32 pass and the one failure is still the opcode-coverage test.

**The trade-off:** this switches off overflow checks for *all* integer math in the project, not just 7XNN. Those debug panics are how Rust catches arithmetic bugs. For example, if something like `self.i + n` or `row + i` ever overflowed by mistake, it would now silently wrap. Usually you'd then hit an array-index panic further on, or worse, read the wrong memory without any error.

**The more idiomatic fix** is to keep the checks on and say where wrapping is intended:

```rust
self.register[index] = self.register[index].wrapping_add(value);
```

`wrapping_add` costs nothing in either profile: it compiles to a plain `add`, the same as `+` does in release. It also tells the reader that wrapping here is what the CHIP-8 spec wants. Later, 8XY4 (add with carry) and 8XY5/8XY7 (subtract with borrow) need `overflowing_add` / `overflowing_sub`, which return the result plus a bool that maps straight onto VF.

I'd go with `wrapping_add` in `add_register` and remove the profile setting, so debug builds keep catching real bugs. If you want that, I'll revert the `Cargo.toml` change.

### 🧑 David

How to write an int in bit in rust

### 🤖 Claude

Two things, depending on what you mean:

**1. Writing a number in binary in your code (a literal):** use the `0b` prefix. You can put `_` anywhere as a separator.

```rust
let a: u8 = 0b1111_0000;      // 240
let b = 0b1010u8;             // the type suffix also works
let mask: u64 = 1 << 63;      // or build it with shifts
```

**2. Printing a number in binary:** use the `{:b}` formatter.

```rust
let x: u8 = 0xF0;
println!("{x:b}");        // 11110000
println!("{x:08b}");      // pad with zeros to 8 digits: 00000101 for 5
println!("{x:#010b}");    // 0b11110000  (# adds "0b", and the width counts it)
```

Hex (`{:x}`, `{:X}`, `{:#06X}`) and octal (`{:o}`) work the same way.

**For your emulator:** a display row is a `u64`, so this prints one row as a line of pixels:

```rust
for row in &chip8.display {
    println!("{row:064b}");
}
```

That's handy in tests or for debugging `draw`: you can assert a row against a literal like `0b1111_0000 << 56`, so the sprite's shape is visible in the test source. Sprite bytes from ROMs are easier to read the same way: `0b1111_0000, 0b1001_0000, ...` makes it obvious that they draw a "0".

### 🧑 David

make tests for all my function that has not. Add doc strings to

> 🔧 **Bash**
> 🔧 **Bash**
> 🔧 **Bash**

### 🤖 Claude

I'll add the doc comments first, then the tests.

> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs

### 🤖 Claude

The file changed on disk while I was editing, so I'm checking what else is in it now.

> 🔧 **Bash**
> 🔧 **Bash**

### 🤖 Claude

Only my doc comments differ from HEAD, and it builds. Now the tests.

> 🔧 **Bash**
> 🔧 **Bash**
> 🔧 **Write** — <scratchpad file>
> 🔧 **Bash**

### 🤖 Claude

Many failures. I'm checking that each one is a real bug in the code rather than a mistake in my tests.

> 🔧 **Bash**

### 🤖 Claude

Each failure traces back to a bug in `src/` (details below), not to the tests. I still need to add docs to `frontend.rs`:

> 🔧 **Bash**
> 🔧 **Bash**

### 🤖 Claude

I added 64 tests and doc comments on every item that lacked one. **75 tests pass and 22 fail.** Each failure points to a bug in `src/`, not in the tests. I haven't fixed any of them and haven't committed anything.

**Tests added in `src/chip8.rs`:**
- **Registers:** `set_register`, `get_register` and `add_register`, including wrap-around and rejecting register number 16.
- **`draw`:** origin, column and row offsets, XOR and collision, VF reset, start-position wrap, right and bottom clipping, `n = 0`.
- **`logic_instruction`:** every `8XYN`, the VF flag values, `8FY4` (flag written last), `8XY8` must be unknown. Before each test VF is set to `0xAA`, so a flag that never gets written shows up.
- **`decode`:** `00EE`, `2NNN`, `3XNN`, `4XNN`, `5XY0`, `9XY0`, `8XYN`, `5XY1`/`9XY1` must be unknown.
- **`step`:** each instruction run from a small ROM, including call/return, nested calls, return on an empty stack, and every skip.
- **Other:** `Default`, and the `ExecutionError` messages. I also updated the existing 65 536-opcode test to the current instruction set.

**Doc comments:** `ExecutionError` variants, `REGISTER_SIZE`, the undocumented `Instruction` variants, `display` and `stack`, all the helper functions, `execute`, `step` (with an example that runs as a test), and all of `frontend.rs`. `cargo doc` gives no warnings and `cargo fmt` is clean.

**What the 22 failures point to:**

| Failing tests | Cause |
|---|---|
| All 8 `draw_*` tests and `step_dxyn_*` | In `draw`, `WIDTH - size_of::<u8>()` equals `64 - 1`. `size_of` gives the size in **bytes**, not bits, so only the lowest bit of each sprite byte ends up on screen. You want `u8::BITS`. |
| `8XY4`, `8XY5`, `8XY7` | VF is never written (no carry or borrow flag). |
| `8XY6` | It shifts left and keeps bit 7. It should shift right and keep bit 0. |
| `8XY6`, `8XYE` | The flag goes to `self.set_register(self.i, …)`, which is register `V[I]`, not `VF` (`0xF`). |
| `8XY8`, plus the 65 536-opcode test | `decode` accepts every `8XYN`, and `logic_instruction` returns `Ok(())` for an unknown `N` (`_ => Ok(())`). Both should give `UnknownOpcode`. |

The shift tests set VX equal to VY, so they pass whichever shift behavior you choose. Your code uses the original COSMAC VIP version (VX = VY first). Your quirks notes recommend the modern version as the default, with a toggle.

**Two more things I noticed:**
- `ExecutionError` is `pub` but `lib.rs` doesn't re-export it, so code using the library can't name the error type that `step` returns. I unlinked it in the `step` docs to silence the doc warning. Adding it to the `pub use` in `lib.rs` is the real fix.
- I didn't test `frontend.rs`. `update_buffer` only works on an `MfbWindow`, and making one opens a real window. If you pulled the conversion out into a free function, you could test it.

### 🧑 David

Review now

> 🔧 **Skill** — rust-skills
> 🔧 **Bash**
> 🔧 **Bash**

### 🤖 Claude

Your changes fixed 2 of the 22 failing tests: 77 now pass and 20 fail. The changes you made in `draw` and `logic_instruction` are still uncommitted, together with my docs and tests. Here's what's still wrong, most important first.

## Bugs

**1. `draw` is off by one bit (`chip8.rs:307`):** `<< (WIDTH - 7)` shifts by 57, but a byte needs to shift by **56** to put its top bit (bit 7) at bit 63. With 57, the leftmost pixel of every sprite falls off the end and the rest land one column too far left. The test output shows it: `0xF0 << 57` gives `0xE000…`, not `0xF000…`.

Writing the number as a hand-computed constant is how this happened. Have the compiler work it out instead:
```rust
const SHIFT: u32 = u64::BITS - u8::BITS; // 56, computed at compile time, no casts
```
It costs nothing at runtime and can't drift if the types change.

**2. The flag is written to `V[I]`, not `VF`:** all four `self.set_register(self.i, …)` calls in `8XY4`/`5`/`6`/`7`/`E` use the index register `I` as a register number. You want `0xF`. This one is worse than a wrong value. In a real ROM, `I` usually holds something like `0x2xx`, so `set_register` returns `IndexToHigh` and **the emulator stops the first time it runs `8XY4`**. Accounts for most of the `logic_*` failures.

**3. `8XY6` and `8XYE` have their `>> 7` swapped:**
- `8XY6` keeps bit 0 (`& 1`), then shifts it right by 7, so the flag is always 0.
- `8XYE` keeps bit 7 (`& 0x80`) but never shifts it down, so the flag is `0x80` instead of 1.

**4. An unknown `8XYN` is silently skipped (`_ => Ok(())`):** a bad opcode like `8128` does nothing and gives no error. Reject it in `decode` with `(0x8, _, _, 0x0..=0x7 | 0xE)`. That's also the only place that still has the `opcode` value for `UnknownOpcode { opcode }`. Fixes `logic_8xy8_*` and the 65 536-opcode test.

## Better approaches (with the reasons)

**5. Arithmetic with flags: use `overflowing_*`:**
```rust
let (sum, carry) = vx.overflowing_add(vy);
```
This replaces both the `u16` widening and the second addition. It compiles down to one `add`, and the carry comes straight from the CPU's carry flag. Your version adds twice, once in `u8` and once in `u16`. For subtraction, `let (diff, borrow) = vx.overflowing_sub(vy);` gives the flag as `VF = u8::from(!borrow)`. The `u8::from(bool)` form also clears the 3 clippy `bool_to_int` warnings: it doesn't branch, while `if … {1} else {0}` asks the compiler to remove the branch for you.

**6. The register bounds checks can never fail:** `x` and `y` are nibbles (`hi & 0x0F`, `lo >> 4`), so they're always below 16. `IndexToHigh` is unreachable, yet every register access pays for the check and the `Result` handling with `?`, which is what makes `logic_instruction` so long. Two cleaner options:
- Index the array directly with `self.register[usize::from(x & 0xF)]`. The mask lets the compiler prove the index is below 16 and drop the bounds check entirely. No `Result` is needed.
- Make a `Register(u8)` newtype that only `decode` can construct. Then "x is a valid register" is guaranteed by the type system.

Either way, `set_register`/`get_register`/`add_register` stop returning `Result`, and each `8XYN` arm shrinks to about two lines.

**7. `7XNN` still uses `+=` (`chip8.rs:267`):** it only works because `overflow-checks = false` is set in `Cargo.toml`. Using `wrapping_add` says that wrapping is intended right where it happens, and costs the same.

**8. `CallSubroutine` falls back to `unwrap_or(255)`:** if the conversion could fail, pushing 255 would be a silent wrong answer. It can't fail, because `pc_jump_to` keeps the PC at or below `0xFFF`, so the fallback is dead code that looks like error handling. Either use `as u16` with a comment saying why it's safe, or store the PC as a `u16`, as discussed before.

**9. `draw` can panic:** `&self.memory[self.i..self.i + n]` goes out of bounds if `I + n > 4096` (for example `I = 0xFFF`, `n = 2`). Also, `.take(n)` does nothing because the slice already has length `n`. Wrap the address like `fetch` does, or return an error.

**10. `execute` takes `Instruction` by value (clippy `needless_pass_by_value`):** the enum is only a few bytes and holds plain numbers. Add `#[derive(Clone, Copy)]` and passing it by value becomes the idiomatic choice, as cheap as passing a reference.

## Minor

- `Ok(self.set_register(…)?)` is the same as `self.set_register(…)`. The `?` unwraps the `Result` and `Ok` immediately wraps it again.
- The skips use `program_counter += 2` while everything else goes through `pc_jump_to`. Using `pc_jump_to` everywhere keeps the wrap at the end of memory in one place.
- `SkipConditionAND`/`XOR` should be renamed to `SkipIfEqual`/`SkipIfNotEqual`, since they don't do AND or XOR. `IndexToHigh` should be `IndexTooHigh`.
- `ExecutionError` still isn't re-exported in `lib.rs`.
- The other clippy pedantic warnings are cosmetic: `0x00FF_FFFF` separators, inline format args in `LoadError`, and `is_empty` in the tests.

Fixing #1–#4 should make all 20 failing tests pass. I'd do #5 and #6 at the same time, because they make the fixes for #2 and #3 shorter.

### 🧑 David

What is vf

### 🤖 Claude

**VF is register 15**, the last of the 16 registers V0 to VF. The "F" is the hex digit for 15. In your code it's `self.register[0xF]`.

VF has two jobs:

1. **It's a normal 8-bit register.** A program can store values in it like any other.
2. **It's the flag register.** Some instructions write a status value (0 or 1) into it after running, overwriting whatever was there:

| Instruction | What VF holds afterwards |
|---|---|
| `8XY4` VX += VY | **1** if the addition overflowed past 255 (carry), else 0 |
| `8XY5` VX −= VY | **1** if VX ≥ VY (no borrow), else 0 |
| `8XY7` VX = VY − VX | **1** if VY ≥ VX (no borrow), else 0 |
| `8XY6` shift right | the bit that fell off the right (old bit 0) |
| `8XYE` shift left | the bit that fell off the left (old bit 7) |
| `DXYN` draw | **1** if any lit pixel was turned off (a collision), else 0 |

CHIP-8 has no separate status register like the carry or zero flags on a real CPU, so it reuses VF for that. This is why games avoid keeping data in VF: the next add or draw can overwrite it.

A program then reads VF like any other register. For example, collision detection in a game:
```
DXYN        ; draw the player
3F01        ; skip the next instruction if VF == 1 (a collision happened)
```

Back to your code: bug #2 in the review was writing the flag with `set_register(self.i, …)`. That uses the *index register I* as a register number. It should be `0xF`, meaning VF. Your `draw` already gets this right with `self.register[0xF] = u8::from(collision)`.

Two details to watch for:
- **When X is F** (e.g. `8F14`), the instruction writes its result to VF and then the flag to VF too. The usual convention is to write the flag last, so VF ends up holding the flag. My test `logic_8fy4_with_vf_as_destination_keeps_the_flag` checks for that.
- **Read VX and VY before you write anything.** If Y is F, writing the flag first would overwrite an operand you still need. Your `8XY4`/`5`/`7` already do this by loading `vx` and `vy` into locals first.

### 🧑 David

review now

> 🔧 **Bash**

### 🤖 Claude

95 tests pass and 2 fail. Your fixes for bugs #1, #2 and #3 work: every `draw_*` and every flag test now passes.

## Still wrong: rebuilding the opcode for an unknown `8XYN`

```rust
opcode: (0x80_u16 + u16::from(x)) << (2 + (u16::from(y) << 1) + u16::from(n)),
```
This doesn't rebuild `8XYN`. For `0x8128` it computes `0x81 << 14`, which is a garbage number. Most shift amounts here are ≥ 16, and shifting a `u16` by 16 or more is an overflow. It only avoids a panic because `overflow-checks = false`. You put each nibble in place with a shift by its position, then combine them with `|`:
```
0x8000 | x << 8 | y << 4 | n      // nibble positions: 12, 8, 4, 0
```

The cleaner fix is still to **reject it in `decode`**, which already has the original `opcode`, so there's nothing to rebuild:
```rust
(0x8, _, _, 0x0..=0x7 | 0xE) => Ok(Instruction::Logical { x, y, n }),
```
An unknown `8XY8` then falls through to `_ => UnknownOpcode { opcode }`, and the `_` arm in `logic_instruction` can never be reached. That fixes both failing tests: `logic_8xy8_is_unknown_opcode`, and the 65 536-opcode test, which checks that *decode* rejects it.

## Using `REGISTER_SIZE - 1` for VF
It works, but it's the wrong name for the job: it describes "the last register", not "the flag register". If the register count ever changed, the flag would silently move too. Add a constant that says what it is:
```rust
const VF: usize = 0xF;
```
Then `draw` and `logic_instruction` can use the same name. Right now `draw` uses `0xF` and the 8XYN arms use `REGISTER_SIZE - 1`.

## Not changed since the last review
- **#5:** `vx + vy` / `vx - vy` plus a second `u16` addition for the carry. `overflowing_add` / `overflowing_sub` give you both the result and the flag with one instruction. `u8::from(bool)` would also clear the 3 clippy `bool_to_int` warnings.
- **#6:** the register bounds checks can never fail (x and y are always nibbles). They're why every arm needs `?` and `Result`.
- **#7:** `add_register` still uses `+=` and only works because of the `Cargo.toml` setting.
- **#8:** `unwrap_or(255)` in `CallSubroutine`.
- **#9:** `draw` slices `memory[i..i + n]` without wrapping, so `I = 0xFFF` panics. The `.take(n)` there does nothing.
- **#10:** `Instruction` should derive `Clone, Copy`.
- `WIDTH - 8` is correct now. `u64::BITS - u8::BITS` would explain *why* it's 56 without a magic number, but that's optional.

Fix the decode pattern and all tests should pass.

### 🧑 David

review now

> 🔧 **Bash**
> 🔧 **Bash**

### 🤖 Claude

93 tests pass and 4 fail. Two things are left to fix: a typo-style slip and the decode pattern.

## Failing tests

**1. `max: VF` in the register errors (3 tests):** `set_register`, `get_register` and `add_register` now report `IndexToHigh { index, max: VF }`, so `max` is 15. Before, `max` was 16, and the variant's doc and the tests define it as "number of registers; valid numbers are `0..max`". This looks like a find-and-replace of `REGISTER_SIZE` that went one step too far. Put `REGISTER_SIZE` back. If you want `max` to mean "the highest valid index" (15), that's fine too, but then change the doc and the 3 tests to match.

**2. `decode_accepts_exactly_the_supported_opcodes`:** your opcode rebuild is correct now:
```rust
(u16::from(0x80 + x) << 8) + u16::from((y << 4) + n)
```
Each part fits its type: `0x80 + x` is at most `0x8F` and `(y << 4) + n` is at most `0xFF`. But `decode` still accepts every `8XYN`, so the test still sees `8XY8` decode successfully. The one-line fix is still to make the pattern `(0x8, _, _, 0x0..=0x7 | 0xE)`. That also makes the `_` arm and the rebuild code unnecessary.

## Your new `overflowing_add` / `overflowing_sub` helpers
They work, but they don't use the standard methods the review pointed to, and they hide a few problems:
- **They reimplement std under the same name.** `u8::overflowing_add` already exists and returns `(result, overflowed)`. Your version computes the sum twice (`a + b` in `u8`, then again in `u16`), and `a + b` / `a - b` still only avoid a panic because of `overflow-checks = false`.
- **The flag name is backwards for subtraction.** `overflowing_sub` returns `is_overflow = 1` when there is *no* borrow. Anyone reading `is_overflow` will assume the opposite.
- **There's a hidden side effect.** A function called `overflowing_add` also writes a register. You can't tell that from the call site.

With the standard method, each arm is two writes and nothing else:
```rust
let (sum, carry) = vx.overflowing_add(vy);   // one `add`; carry comes from the CPU flag
// write sum to VX, then u8::from(carry) to VF
let (diff, borrow) = vx.overflowing_sub(vy);
// VF = u8::from(!borrow)
```

## Smaller points
- **`CallSubroutine` now uses `.unwrap()`.** That's better than a made-up fallback value, but it panics without saying why it can't fail. Use `.expect("PC is always below 0x1000")` to state the reason (`err-expect-bugs-only`), or store the PC as a `u16` so no conversion is needed.
- **`draw` still writes `self.register[0xF]`.** Use the `VF` constant there too. `u64::BITS - u8::BITS` is good.
- **Still open from earlier reviews:** `add_register`'s `+=` (#7), the bounds checks that can never fail (#6), the `Ok(self.set_register(…)?)` wrapping in `8XY0`–`8XY3`, the possible panic when slicing in `draw` (#9), and `#[derive(Clone, Copy)]` on `Instruction` (#10).

The clippy `bool_to_int` warnings are gone. The rest are cosmetic.

Fix #1 and #2 and all tests should pass.

### 🧑 David

Where should i handle inputs ? In frontend ?

### 🤖 Claude

Yes, split it in two:

- **The frontend reads the physical keyboard.** It's the only part that knows about `minifb`, `Key::Q` and so on. It turns the keyboard into CHIP-8 keypad state.
- **The core (`Chip8`) only stores which of the 16 keypad keys are down.** Its instructions read that state. It never calls `minifb`.

This is the same split you already have for the display. The core holds `display: [u64; HEIGHT]` and the frontend turns it into pixels. Input is the same thing in the other direction. It also keeps what `chip8.rs`'s module doc promises ("this module does no I/O"), and it means you can test key instructions by setting the state in a test, with no window.

## How the state moves

```
minifb keyboard ──(frontend: map keys)──> keypad state ──(main loop)──> chip8.set_keys(...)
                                                                         │
                                         EX9E / EXA1 / FX0A read it  <───┘
```

Each frame in `main`:
1. `window.draw()` (it calls `update_with_buffer`) also makes `minifb` poll the keyboard.
2. The frontend builds the keypad state from `window.is_key_down(...)` for the 16 mapped keys.
3. Hand that state to the core, then run the CPU steps for that frame.

## How to store it: a `u16` bitmask

One bit per key (bit `k` = key `k` is down) fits all 16 keys in exactly one `u16`:
- **Checking a key** (`EX9E`/`EXA1`) is one shift and one AND: `keys & (1 << vx) != 0`.
- **It's 2 bytes,** copied by value from the frontend for free. A `[bool; 16]` is 16 bytes and needs a loop or a 16-byte compare for anything that looks at all the keys.
- **`FX0A` (wait for a key) gets easy.** On the original VIP a key only counts once it has been pressed *and released* (see your quirks notes). If the core keeps the previous frame's mask too, then `prev & !now` gives every key released since last frame, in one operation. `trailing_zeros()` then tells you which key it was.

A `[bool; 16]` would also work and is a bit easier to read. The bitmask wins on size, copying, and the `FX0A` edge detection.

## Key mapping

This lives entirely in the frontend. The usual layout uses the left block of a QWERTY keyboard:
```
CHIP-8 keypad      Keyboard
1 2 3 C            1 2 3 4
4 5 6 D            Q W E R
7 8 9 E            A S D F
A 0 B F            Z X C V
```
A `const` array of 16 `(minifb::Key, u8)` pairs is enough. Loop over it and set the matching bits. On an AZERTY keyboard, you might want to map by physical key position rather than by letter.

## A problem in your loop to fix at the same time
`main` runs **one `step()` per frame**, and `set_target_fps(60)` limits it to 60 frames per second. That's only 60 instructions per second, while CHIP-8 programs expect about 700. The usual structure:
```
each frame (60 Hz):
    read keys -> chip8
    run ~11 steps   (700 / 60)
    tick the delay/sound timers once
    draw
```
This also gives you the 60 Hz timer tick for free. It's the right place for the timers you haven't added yet (`FX07`/`FX15`/`FX18`).
