# Storing 4 kB memory in Rust

*Claude Code session, 2026-11-10. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

`/rust-skills` What is the best way to store the 4 kB memory

### 🤖 Claude

**Use a fixed-size array stored directly in your emulator struct: `[u8; 4096]`.**

```rust
const MEMORY_SIZE: usize = 4096;

struct Chip8 {
    memory: [u8; MEMORY_SIZE],
    // registers, pc, i, stack, display...
}
```

## Why this beats the alternatives

| Option | Where the bytes live | Cost |
|---|---|---|
| **`[u8; 4096]`** | Inside the struct | No allocation. The length is part of the type. No extra pointer to follow. |
| `Vec<u8>` | On the heap | Stores a pointer, length and capacity (24 bytes), but the size never changes. Every access first loads the pointer, and the compiler doesn't know the length, so it keeps every bounds check. |
| `Box<[u8]>` | On the heap | Like `Vec` without the capacity field. The length is still only known at runtime. |
| `Box<[u8; 4096]>` | On the heap | The size is known at compile time, but every access goes through a pointer. The only gain is that moving the struct copies 8 bytes, not 4 KB. |

- **Size is fixed by the spec.** CHIP-8 RAM is always 4096 bytes and never grows. Use `Vec` when the size changes or is only known at runtime, and neither is true here. With a fixed size, the compiler can check and optimize more.
- **The stack is not a problem.** 4 KB is small compared to the 8 MB main-thread stack on Linux. Your `Chip8` struct is built once and then used through `&mut self`, so it is almost never moved, and a move is the only time the 4 KB gets copied. If you later move it around a lot (for example, passing it by value between threads), switch to `Box<[u8; 4096]>`.
- **Cache.** All 4 KB fits easily in a 32–48 KB L1 data cache. Placing it next to the registers and PC also keeps the hot state close together.

## Bounds checks: the main optimization

Every `memory[addr]` checks `addr < len` and panics if it fails. With an array, the compiler knows the length is 4096 at compile time. CHIP-8 addresses are 12 bits, so you can mask them:

```rust
let byte = self.memory[usize::from(addr & 0x0FFF)];
```

`addr & 0x0FFF` is always below 4096, so LLVM can prove the index is in range and remove the check. With a `Vec`, it can't, because the length is only known at runtime. The mask is also correct behavior, not just a trick: real hardware wraps addresses at 12 bits. For example, `I + 1` at the top of memory goes back to 0 instead of crashing the emulator. Use `usize::from(...)` rather than `as usize`, because `From` only exists for conversions that can't lose data (rule `num-cast-try-from`).

## Gotchas

1. **`#[derive(Default)]` won't compile.** Arrays only implement `Default` up to length 32. Write a `new()` that sets `memory: [0; MEMORY_SIZE]` yourself.
2. **Copy blocks with slices, not byte-by-byte loops.** To load the font at `0x050` and the ROM at `0x200`, take a sub-slice like `memory[0x200..0x200 + rom.len()]` and call `copy_from_slice`. It compiles to a single `memcpy` with one bounds check.
3. **Check the ROM size.** The largest valid ROM is `4096 − 0x200 = 3584` bytes. If it's bigger, return an error instead of letting `copy_from_slice` panic, because a bad ROM file is user input, not a bug (rule `err-result-over-panic`).

Next design question: how you index the PC and `I` registers. Storing them as `u16` and converting to `usize` only when you access memory keeps the struct small and close to the hardware's 16-bit registers.

### 🧑 David

How to create a variable base on Chip8 struct ?

> 🔧 **Bash** — Show current source files

### 🤖 Claude

There are two ways to do it. The second is the idiomatic one for your emulator.

## 1. Struct literal

You list every field by name with its value:

```rust
let mut chip8 = Chip8 {
    memory: [0; MEMORY_SIZE],   // 4096 zeros
    program_counter: 0x200,     // programs load at 0x200
    i: 0,
    // ...every other field
};
```

- **Every field is required.** Rust has no "uninitialized" or default values here, so if you leave one out the code won't compile. This is on purpose: C would let you read garbage from a field you forgot to set.
- **`mut` is needed** because the emulator changes its own state: `memory`, `program_counter`, the timers and so on. Without `mut`, every write is a compile error.
- **`[0; MEMORY_SIZE]`** is the array repeat syntax: `MEMORY_SIZE` copies of `0`. The length must be a constant known at compile time, and your `const` is one.

## 2. A `new()` associated function (idiomatic)

Put the struct literal inside an `impl` block so that `main` doesn't have to know how to set up a CHIP-8:

```rust
impl Chip8 {
    fn new() -> Self {
        Self { /* struct literal, as above */ }
    }
}

// in main:
let mut chip8 = Chip8::new();
```

- `new` takes no `self`, so it's an **associated function** (like a static method in other languages). You call it with `Chip8::new()`, not `chip8.new()`.
- `Self` is another name for `Chip8` inside the `impl` block, so you don't have to change it everywhere if you rename the type.
- **`new` is not a special keyword**, just a naming convention. Rust has no constructors, so this is an ordinary function that returns the value.
- **Why it's better:** the startup rules live in one place. The PC must start at `0x200` and the font must be copied to `0x050`. Every caller then gets a valid machine. Later, `new` can take the ROM bytes and copy them in too.

**Performance:** returning a 4 KB struct by value looks like a copy, but it isn't. The compiler builds the value directly where the caller stores it (return value optimization), so returning `Self` costs nothing extra.

## About your current code

- **`program_counter` must start at `0x200`, not 0.** Address 0 is empty memory.
- **Your font isn't set up yet.** It goes in `new()`: copy the 80 font bytes into `memory[0x050..0x0A0]`.
- **Missing fields.** You'll need a call stack: 16 × `u16` plus a stack pointer, used by `2NNN` and `00EE`. You'll also need the 64×32 display buffer before you can do `00E0` and `DXYN`.
- **`variables` is usually named `v`** in emulators and specs (V0–VF). That makes `self.v[x]` match the opcode tables you'll be reading.

Try writing `Chip8::new()` yourself, then load `buffer` into memory at `0x200`. I can review it after.

### 🧑 David

How to fill my memory with my buffer vector

> 🔧 **Bash** — Show current main.rs

### 🤖 Claude

Use **slicing** together with **`copy_from_slice`**.

## The concept

`copy_from_slice` copies one slice into another slice **of exactly the same length**. Your memory is 4096 bytes and the ROM is much smaller, so first cut a sub-slice of memory that has the ROM's length:

```rust
dest[start..start + src.len()].copy_from_slice(&src);
```

- `memory[a..b]` doesn't copy anything. It's a *view* (`&mut [u8]`) into the array, stored as a pointer and a length.
- `&buffer` turns your `Vec<u8>` into a `&[u8]` (deref coercion). The `Vec` also converts to a slice automatically when you call a slice method on it.
- The start is `0x200` because programs load there. Index with `usize`, so a `const PROGRAM_START: usize = 0x200;` is cleaner than a magic number.

## Why this beats a `for` loop

A loop like `for (i, byte) in buffer.iter().enumerate() { memory[0x200 + i] = *byte; }` does a **bounds check on every byte** and relies on the optimizer to turn it into a block copy. `copy_from_slice` checks the range **once** and then does a single `memcpy`, which uses SIMD and copies 16 or 32 bytes at a time. It also says clearly what you mean.

## The trap: ROM too large

If `0x200 + buffer.len() > 4096`, the slicing **panics**. Only `4096 − 0x200 = 3584` bytes are available. A ROM file is user input, so check the length first and return an error instead of crashing. Your `main` returns `std::io::Result<()>`, so you can create an error with `std::io::Error::new(std::io::ErrorKind::InvalidData, "rom too large")` (or `io::Error::other(...)`) and return it with `return Err(...)`.

## Where to put it

Make it a method on `Chip8`, for example `fn load_rom(&mut self, rom: &[u8]) -> Result<...>`:

- **`&mut self`** because it writes into `self.memory`.
- **`rom: &[u8]`, not `&Vec<u8>`.** A slice accepts a `Vec`, an array or part of another buffer, and avoids a double indirection (rule `own-slice-over-vec`).

## Small simplification

`std::fs::read("path")?` returns a `Vec<u8>` in one call. It replaces `File::open`, `Vec::new` and `read_to_end`, and it also sizes the `Vec` from the file's metadata, so it allocates only once. Reading into a `Vec` first is fine here: it's a single small allocation at startup, nowhere near the hot loop.

When it's written, a quick check is to `println!("{:02X?}", &chip8.memory[0x200..0x210])` and compare the bytes with `xxd test_suite/1-chip8-logo.ch8 | head -1`.

### 🧑 David

How to handle Error correctly in my load_rom function

### 🤖 Claude

## First, two bugs in the current code

1. **Wrong start address.** `self.memory[0..rom.len()]` copies the ROM to address `0x000`, but your PC starts at `0x200`. The slice must start at `PROGRAM_START`. Your debug print shows the ROM bytes at the start of memory and zeros at `0x200`.
2. **The `Result` is ignored.** `chip8.load_rom(&buffer);` throws the result away, and the compiler warns about it (`unused_must_use`). If you don't handle the error, it disappears silently.

## Choosing the `Result` types

`Result<u8, u8>` doesn't mean anything: the `0` you return on success and a `u8` error code carry no information.

- **Success type:** loading has nothing to return, so use `()`. The return type becomes `Result<(), SomeError>` and the function ends with `Ok(())`.
- **Error type:** you have four options, from worst to best for this case.

| Error type | Verdict |
|---|---|
| `u8` / integer code | This is C-style. The caller has to know what `1` means, and nothing stops a typo. |
| `String` | Allocates memory, and the caller can't `match` on it. This is a "stringly-typed" error (rule `anti-stringly-typed`). |
| `std::io::Error` with `ErrorKind::InvalidData` | Works and fits your current `main`, but you're borrowing the I/O error type for something that isn't an I/O problem. |
| **Your own error enum** | **Best.** It's specific, carries data, can be matched, and costs nothing: no allocation, and its size is just its largest variant. |

## The idiomatic way: your own error enum

```rust
#[derive(Debug)]
enum LoadError {
    RomTooLarge { size: usize, max: usize },
}
```

To make it a real error type that works with `?` and prints nicely, implement two traits:

- **`std::fmt::Display`**: the human-readable message, e.g. `"rom is 5000 bytes, max is 3584"`. By convention it starts lowercase with no trailing period (rule `err-lowercase-msg`).
- **`std::error::Error`**: an empty `impl Error for LoadError {}` is enough. It only requires `Debug` and `Display`.

Using an enum, even with one variant, means you can add `RomEmpty` or other cases later, and callers who `match` will get a compile error until they handle them.

## The size check

Compare against the remaining space, which is calculated from constants:

```rust
let max = MEMORY_SIZE - PROGRAM_START; // 3584, computed at compile time
if rom.len() > max { return Err(LoadError::RomTooLarge { size: rom.len(), max }); }
```

Writing `PROGRAM_START + rom.len() > MEMORY_SIZE` adds an untrusted length to a constant, which could in theory overflow. Subtracting two constants can't overflow, so the check is safe by design.

**Alternative:** `self.memory.get_mut(range)` returns `Option<&mut [u8]>` instead of panicking. You can chain it with `.ok_or(LoadError::...)?`, which does the bounds check and the error in one step. The downside is that you lose the explicit check, and building the range still needs the addition.

## Connecting it to `main`

You already imported `std::error::Error`, so change `main` to return `Result<(), Box<dyn Error>>`. Then `?` works for both:

- `std::fs::read(...)?` returns an `io::Error`.
- `chip8.load_rom(&buffer)?` returns your `LoadError`.

**How `?` does this:** on `Err(e)` it returns `Err(From::from(e))`. The standard library implements `From<E> for Box<dyn Error>` for every `E: Error`, so both error types are boxed automatically.

The `Box` costs one heap allocation, but only when an error actually happens, and only in `main` where the program is about to exit anyway. That's why `Box<dyn Error>` (or `anyhow` later) is good for the application edge, while a concrete enum is better inside your emulator code, where callers might want to `match` on the error.

## Small notes

- `File` and `Read` are now unused imports, so remove them (`cargo clippy` will point them out).
- **`program_counter: usize`** saves a conversion at each fetch. But it's 8 bytes instead of 2, and it no longer matches the 12-bit hardware register. So it won't wrap the way CHIP-8 expects unless you mask it (`& 0x0FFF`), and jump targets (`NNN`, which is `u16`) will need converting the other way. Both choices are valid; just pick one deliberately. Keeping `u16` and converting only when you index memory is closer to the hardware.

### 🧑 David

How to implement Display for my LoadError enum

### 🤖 Claude

## The trait's shape

`Display` has a single method you must write. The signature has to match exactly:

```rust
use std::fmt;

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // write the message into `f`
    }
}
```

- **`&self`**: a borrow of the error you're printing.
- **`f: &mut fmt::Formatter<'_>`**: where the output goes, for example stdout, a `String` or a file. Your code doesn't know or care which. `'_` means "some lifetime the compiler works out", so you don't have to name it.
- **`fmt::Result`**: an alias for `Result<(), fmt::Error>`. It only reports that writing failed, never what your message says.

## The body: `match` and `write!`

1. **`match self`** with one arm per variant, so each error gets its own message. Use `Self::RomTooLarge { size, max } => ...` to destructure the variant and pull its fields out as variables. Because `self` is a reference, `size` and `max` are bound as `&usize`. This is "match ergonomics", and you don't need any `*` or `ref`. Formatting works on references directly.
2. **`write!(f, "...", ...)`**: the same syntax as `println!`, except the first argument is where to write. It returns a `fmt::Result`, so make it the arm's value (no semicolon) and the result is returned from `fmt`.
3. Since Rust 2021 you can put variables directly in the format string: `"rom is {size} bytes"`.

Don't add a catch-all `_ =>` arm. When you add a new variant, the compiler will then point to this `match` and force you to write a message for it (rule `pat-exhaustive-enum`).

## Why `write!` and not `format!`

`format!` builds a new `String` on the heap and returns it. `write!` streams the pieces straight into the formatter's output, so nothing is allocated. Building a `String` inside `fmt` and then writing it would allocate memory only to copy it again (rules `mem-write-over-format` and `anti-format-hot-path`).

## What you get for free

- **`.to_string()`**: the standard library implements `ToString` for every type that implements `Display`. Implement `Display`, never `ToString` directly.
- **`{}` in `println!`/`eprintln!`** uses your `Display`. **`{:?}`** uses the derived `Debug`. `Display` is for the person using the program and `Debug` is for you while developing (rule `type-display-vs-debug`).
- **`impl std::error::Error for LoadError {}`**: once you have `Display` and `Debug`, this empty impl compiles, and `?` can turn your error into `Box<dyn Error>` in `main`.

## How to check it

Pass a ROM that's too large (`head -c 5000 /dev/urandom > /tmp/big.ch8`) or temporarily lower the limit. Then look at what `main` prints. When `main` returns an `Err`, Rust prints it with **`Debug`**, not `Display`, so you'll see `Error: RomTooLarge { size: 5000, max: 3584 }`. To show your `Display` message instead, handle the error yourself in `main` and print it with `eprintln!("error: {e}")`.

### 🧑 David

How to catch the error to  print the message ?

> 🔧 **Bash** — Show current main.rs

### 🤖 Claude

## First: `load_rom` never returns an error

Your `load_rom` doesn't check the size yet, so `Err` is never produced. With a ROM that's too big, the slice `PROGRAM_START..PROGRAM_START + rom.len()` goes past 4096 and the program **panics** before any error could be caught. Add the size check at the top of `load_rom` (`if rom.len() > MEMORY_SIZE - PROGRAM_START { return Err(...) }`). Then there's something to catch.

## Ways to catch a `Result`

**`match`** handles both cases explicitly:
```rust
match chip8.load_rom(&buffer) {
    Ok(()) => { /* continue */ }
    Err(e) => eprintln!("error: {e}"),  // {e} uses your Display
}
```

**`if let`** works when you only care about the error:
```rust
if let Err(e) = chip8.load_rom(&buffer) { eprintln!("error: {e}"); }
```

**`let ... else`** is for when success gives you a value you want to keep. It doesn't fit here because `load_rom` returns `()`.

Use `eprintln!`, not `println!`. Errors go to **stderr**, so they stay visible when stdout is redirected or piped.

## Recommended structure: `run()` plus a small `main`

Catching errors one by one in `main` repeats the same code, and it doesn't cover the `fs::read` error. The usual pattern is:

1. Move your current `main` body into `fn run() -> Result<(), Box<dyn Error>>` and keep using `?` everywhere inside it.
2. `main` catches the single result in one place:

```rust
fn main() -> std::process::ExitCode {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
```

- `Box<dyn Error>` implements `Display` by forwarding to the error inside it. Your `LoadError` message and the `io::Error` message (for example "No such file or directory") both print correctly.
- **`ExitCode`** gives shells and scripts a non-zero exit status on failure. Returning `ExitCode` is better than calling `std::process::exit(1)`, because `exit` skips destructors (`Drop`), so buffered output or open resources might not be cleaned up.
- This keeps `run` testable and leaves `main` with one job: reporting.

## Your test has 3 problems

1. **`std::fs::read` returns a `Result`**, not a `Vec`. In a test, use `.expect("...")`, or make the test return `Result` and use `?`.
2. **`LoadError` alone isn't a value**, only the type's name. Also, `assert_eq!` needs `PartialEq` on both sides. Use `matches!`, which checks the shape of a pattern and needs no extra trait:
   ```rust
   assert!(matches!(chip8.load_rom(&rom), Err(LoadError::RomTooLarge { .. })));
   ```
   `{ .. }` means "any field values".
3. **You don't need a file.** `vec![0u8; 4000]` creates an oversized ROM in memory. The test then doesn't depend on a file existing or on the folder it's run from. Also test the exact limit: 3584 bytes must load, and 3585 must fail. Off-by-one errors happen right at that boundary.

Put tests in a `#[cfg(test)] mod tests { use super::*; ... }` block. Then they are only compiled for `cargo test` and never into your binary (rule `test-cfg-test-module`).

## Small fixes

- The message has typos: "to large" → "too large", "rome" → "rom", and a space is missing after "max size". Inline arguments also make it easier to read: `"rom too large: {size} bytes, max is {max}"`.
- Imports: `use core::fmt` works but `std::fmt` is the usual one in a `std` program. `fmt::write`, `File` and `Read` are unused. `cargo clippy` will list them all.

### 🧑 David

review my load_rom and main now

> 🔧 **Bash** — Show code, run clippy and tests
> 🔧 **Bash** — List test ROMs and run the binary

### 🤖 Claude

Your error handling works. Running it on `big.ch8` prints `error: rom to large: rome size: 5000; max size3584` instead of panicking, so the size check and the `run()`/`main` split do their job. `cargo test` finds 0 tests, though, and Clippy isn't installed, so it couldn't run.

## `load_rom`

**Good:**
- The check comes before the slicing, so a ROM that's too big returns `Err` instead of panicking.
- The limit is the difference of two constants, so it can't overflow.
- **Bonus:** once the code has checked that `rom.len()` is at most 3584, the compiler can prove that `PROGRAM_START + rom.len()` is at most 4096. In a release build it can remove the panic path of the slice. Your check gives safety and makes the bounds check on the copy free.

**To improve:**
1. **`MEMORY_SIZE - PROGRAM_START` appears twice.** Make it a constant: `const MAX_ROM_SIZE: usize = MEMORY_SIZE - PROGRAM_START;`. It's computed at compile time, so the compiled code is the same, but the limit is defined once and has a name.
2. **Optional: use an early return.** `if too_big { return Err(...); }` followed by the copy and `Ok(())` at the normal indentation level. The success path is then not nested inside an `else`. Most Rust code uses this style for checks at the top of a function.

## `run` / `main`

1. **`let _ = chip8.load_rom(&buffer)?;`: drop the `let _ =`.** After `?` the value is `()`, so there's nothing to throw away. Avoid the habit: `let _ =` is how you deliberately ignore a `#[must_use]` result. If you ever remove the `?`, it would silently swallow the error and the compiler would stay quiet. Write `chip8.load_rom(&buffer)?;`.
2. **Hard-coded ROM path.** The next step is reading it from the command line with `std::env::args().nth(1)`. That returns an `Option<String>`, so a missing argument becomes another error case. A `.ok_or("usage: chip8-rs <rom>")?` works with `Box<dyn Error>`, because `&str` converts into it.
3. **`fn main() -> std::process::ExitCode`:** you already import `ExitCode`, so write just `ExitCode`.
4. **`&chip8.memory[0x200..0x210]`** in your debug print: use `PROGRAM_START`, as you do everywhere else.

## Also

- **Your test was deleted, and `cargo test` runs 0 tests.** Add it back inside a `#[cfg(test)] mod tests`. Use in-memory ROMs (`vec![0u8; n]`) to check three things: 3584 bytes loads, 3585 bytes fails with `matches!(..., Err(LoadError::RomTooLarge { .. }))`, and the bytes really land at `0x200`. The third check would have caught the earlier "copied to address 0" bug.
- **The `Display` message still has the typos:** use `"rom too large: {size} bytes, max is {max}"`.
- **`use core::fmt`:** change it to `std::fmt`. Then use `fmt::Formatter` and `fmt::Result` in the signature, since you already import `fmt`.
- **The `dead_code` warnings** for `i`, the timers and `variables` are expected until you write the opcodes.
- **Clippy is missing** (`no such command: clippy`). On NixOS, add `clippy` (and `rustfmt` if needed) to your `home.nix` packages, or use a rust-overlay/fenix toolchain that includes it. Then `cargo clippy -- -W clippy::pedantic` will catch most of these points for you.

### 🧑 David

How to read the ROM path from command line args

### 🤖 Claude

## `std::env::args()`

`std::env::args()` returns an **iterator** over the command-line arguments as `String`s. The first item (index 0) is the program's own path, e.g. `target/debug/chip8-rs`. So after `cargo run -- rom.ch8`:

| index | value |
|---|---|
| 0 | `target/debug/chip8-rs` |
| 1 | `rom.ch8` |

To get the ROM path, take the item at index 1:

```rust
let path = std::env::args().nth(1);  // Option<String>
```

- **`nth(1)`** advances the iterator and returns the 2nd item.
- It returns an **`Option`** because the user might not have passed any argument. Rust makes you handle that case instead of crashing like C's `argv[1]` would.

## Why not `collect()` into a `Vec`?

You'll often see `let args: Vec<String> = env::args().collect(); let path = &args[1];`. It's worse on two counts:
- **Allocation:** it builds a heap `Vec` and a `String` for *every* argument, only to use one of them. The iterator is lazy, so `nth(1)` only creates the strings it walks past.
- **Panic:** `args[1]` panics when the argument is missing, so the error isn't handled at all.

## Turning the `Option` into an error

`run` returns `Result<_, Box<dyn Error>>`, so convert `None` into an `Err` and use `?`:

```rust
let path = std::env::args().nth(1).ok_or("usage: chip8-rs <rom>")?;
```

- **`ok_or(e)`** turns `Some(v)` into `Ok(v)` and `None` into `Err(e)`.
- **`?` with a `&str`** works because the standard library implements `From<&str> for Box<dyn Error>`. That conversion allocates, but only on the error path.
- **`ok_or` vs `ok_or_else`:** `ok_or` builds its argument every time, even on success. That's free for a string literal. If the error were costly to build, for example with `format!`, use `ok_or_else(|| ...)` so it's only built when needed.

## Paths that aren't valid UTF-8

`env::args()` **panics** if an argument isn't valid UTF-8. That's possible on Linux, because file names are just bytes. The robust version is **`std::env::args_os()`**, which returns `OsString`s with no check.

`std::fs::read` takes any `impl AsRef<Path>`, and `OsString` implements it. So you can pass the `OsString` straight to `fs::read` with no conversion and no possible panic. The cost: printing an `OsString` needs `.display()` or `{:?}`, because it may not be valid text.

For a learning project `args()` is fine, but `args_os()` is the correct choice for file paths.

## Later

Once you add options like `--speed 700` or `--quirks vip`, use the **`clap`** crate with `#[derive(Parser)]`. It generates parsing, `--help` and error messages for you. For a single positional argument, the standard library is enough and adds no dependency.

Test it with `cargo run -- test_suite/2-ibm-logo.ch8`. The `--` separates cargo's own arguments from your program's. Then try `cargo run` with no argument to check that the usage message appears.

### 🧑 David

review my main now

> 🔧 **Bash** — Show main, build, and run edge cases

### 🤖 Claude

Your `main` is correct. I built it and ran it on four cases: all behave as they should, and every failure exits with code 1.

| Case | Output | Exit |
|---|---|---|
| no argument | `error: usage: chip8-rs <rom_path>` | 1 |
| `2-ibm-logo.ch8` | `[00, E0, A2, 2A, 60, 0C, ...]` (starts with `00E0`, the clear-screen opcode) | 0 |
| file missing | `error: No such file or directory (os error 2)` | 1 |
| `big.ch8` | `error: rom to large: 5000 bytes, max is 3584` | 1 |

**What's good:**
- **`args_os()`** means a path that isn't valid UTF-8 can't panic, and the `OsString` goes straight into `fs::read` with no conversion.
- **`nth(1)` + `ok_or` + `?`** is lazy, allocates nothing on success, and doesn't panic.
- **`main` has one job.** The exit codes are right, and destructors still run because you don't call `process::exit`.
- `let _ =` and the full `std::process::ExitCode` path are gone.

## Remaining points

1. **The I/O error doesn't say which file failed.** `No such file or directory` is vague. `io::Error` doesn't store the path, so it's on you to add it. Without a crate, use `map_err` to wrap it in a message that includes the path:
   - Pass `&path` to `fs::read`, so `path` is borrowed and still available for the message.
   - Show it with `path.display()`, since `OsString` has no `Display`, and you'd get `"cannot read nope.ch8: No such file or directory"`.
   - Building that `String` costs an allocation, but only on the error path.
   - Later, `anyhow`'s `.with_context(|| ...)` does exactly this more neatly (rule `err-context-chain`).
2. **The typo is still there:** "rom **to** large" should be "too large".
3. **`0x200` in the debug `println!`** should be `PROGRAM_START`. That line is temporary anyway; it'll go once the emulator loop exists.
4. **Optional:** extra arguments (`chip8-rs a.ch8 b.ch8`) are silently ignored. That's fine for now; `clap` will handle it when you add options.

The `dead_code` warning is expected. Those fields get used once you write the fetch/decode/execute loop.

**Next step:** a `fetch` method. Read the two bytes at `program_counter`, combine them into a big-endian `u16` opcode, and advance the PC by 2. Combine with either `u16::from_be_bytes([hi, lo])` or a manual shift `(u16::from(hi) << 8) | u16::from(lo)`.

### 🧑 David

How to use map_err ?

### 🤖 Claude

`map_err` changes the **error** inside a `Result` and leaves a success untouched:

```rust
fn map_err<F, O: FnOnce(E) -> F>(self, op: O) -> Result<T, F>
```

- `Ok(v)` comes out as `Ok(v)` and **the closure never runs**.
- `Err(e)` comes out as `Err(op(e))`: your closure gets the old error and returns the new one.

It's the error-side counterpart of `map`, which transforms only the `Ok` value. For `Option`, the closest equivalent is the `ok_or` you already used, which turns `None` into an error.

## Closure syntax

`|e| expression` is an anonymous function that takes `e`. The compiler infers the types: here `e` is an `io::Error`, because that's the `E` of the `Result` returned by `fs::read`.

Generic example, not your code:
```rust
let n: i32 = text.parse().map_err(|e| format!("bad number {text:?}: {e}"))?;
```

What happens, in order:
1. `parse()` returns `Result<i32, ParseIntError>`.
2. `map_err` replaces `ParseIntError` with a `String` that includes the input, and the `{e}` keeps the original message.
3. `?` sees `Err(String)` and converts it to `Box<dyn Error>` using `From<String>`, then returns from the function.

## Applying it to your `fs::read`

Same shape:
- Call `fs::read(&path)` with a **reference**. The closure needs `path` for the message, and passing `path` by value would move it into `read`, so it would no longer exist.
- Inside the closure, use `path.display()`, since `OsString` has no `Display`, and `{e}` for the OS's message.
- Put `?` at the end.

**Ownership detail:** the closure only *reads* `path`, so it captures it by **reference** automatically. No copy, no `clone()`. You'd only need `move` if the closure had to outlive `path`, which it doesn't here.

**Cost:** `format!` allocates a `String`, but it's inside the closure, so it only runs on failure. That's why `map_err` takes a closure and not a value: it's lazy, like `ok_or_else`. The success path stays free.

## Trade-off: text vs. a typed error

Formatting the error into a `String` **flattens** it. The original `io::Error` becomes text, so a caller can no longer check `e.kind() == NotFound`, and `Error::source()` returns `None`. For printing in `main` that's fine.

The more structured alternative, for code that may need to react to the error type:

- Add a variant like `Io { path: PathBuf, source: io::Error }` to an error enum.
- Implement `Error::source()` to return the inner error. The chain is kept and the caller can still inspect it (rule `err-source-chain`).
- `map_err` builds that variant: `.map_err(|source| MyError::Io { path: ..., source })`. Note the field shorthand: the closure parameter has the same name as the field.

A related trick: a **tuple variant's name works as a function**. For example, `.map_err(LoadError::Io)` works if `Io(io::Error)` is a variant, with no closure needed.

Start with the `format!` version in `run()`. It's the right level for an application's top level. Use the typed version when errors cross module boundaries, or later, `anyhow::Context` does the same in one call.

### 🧑 David

review my main now

> 🔧 **Bash** — Show code, build, run edge cases

### 🤖 Claude

Your `main` works and the `map_err` is used correctly. I built it and ran the four cases again; every message is now clear and the exit codes are right.

| Case | Output | Exit |
|---|---|---|
| no argument | `error: usage: chip8-rs <rom_path>` | 1 |
| IBM ROM | `[00, E0, A2, 2A, ...]` | 0 |
| missing file | `error: cannot read nope.ch8: No such file or directory (os error 2)` | 1 |
| big ROM | `error: rom too large: 5000 bytes, max is 3584` | 1 |

**What's good:**
- `fs::read(&path)` borrows the path, so the closure can still use it.
- `path.display()` prints the `OsString` correctly.
- `{e}` keeps the OS message.
- The `format!` runs only when reading fails.
- `load_rom` now has the early return and `MAX_ROM_SIZE`, and the typo is fixed.

## Small style nits

These don't change behavior:

1. **`fmt::Display` signature (line 14):** you import `std::fmt`, so write `fmt::Formatter<'_>` and `fmt::Result` instead of the full `std::fmt::` paths.
2. **`write!` on line 17:** use inline arguments, `"rom too large: {size} bytes, max is {max}"`, as you already do in `map_err`. Clippy pedantic flags this as `uninlined_format_args`.
3. **Imports:** merge lines 1–2 into one: `use std::{error::Error, fmt, process::ExitCode};`.
4. **Line 71:** `0x200` should be `PROGRAM_START`. Or delete the line once `fetch` exists.

## Still missing

There are **no tests yet**. Add a `#[cfg(test)] mod tests` with three `load_rom` cases, using in-memory ROMs (`vec![0u8; n]`):
- 3584 bytes loads with `Ok`.
- 3585 bytes fails with `RomTooLarge`.
- The bytes are actually placed at `0x200..`.

These are cheap to write now and protect the loader when you refactor.

Next: the `fetch` method, which reads two bytes as a big-endian `u16` opcode and advances `program_counter` by 2.

### 🧑 David

Before making the tests. Should i move some code to other files to make my project more maintenable

### 🤖 Claude

**Yes, but only a little.** You have about 80 lines, so splitting into many files would add `mod`/`pub`/`use` boilerplate and not help much (rule `proj-flat-small`). One split is worth making now, before you write the tests: separate the **emulator core** from the **program around it**.

## Recommended layout

```
src/
├── main.rs    // command line: read args, read file, run(), exit code
├── lib.rs     // the library crate root: `pub mod chip8;`
└── chip8.rs   // Chip8 struct, new, load_rom, LoadError (later fetch/execute)
```

What goes where:
- **`main.rs`** keeps `run()` and `main()`. That's everything that touches the OS: arguments, files, stderr, exit codes.
- **`chip8.rs`** contains pure emulation code: no files, no printing, no `std::env`. It gets bytes and changes state.
- **`LoadError`** stays in `chip8.rs` for now. Give errors their own `error.rs` only once there are several error types.

## Why this split is the useful one

1. **Testability.** The core takes `&[u8]` and doesn't read files, so tests can give it any bytes with no disk access. You already started this when `load_rom` took a slice instead of a path.
2. **The front-end will change and the core won't.** Soon you'll add a window (minifb/SDL/pixels), a keyboard, sound and maybe a debugger. These belong in `main.rs` or their own modules, never inside `Chip8`. The same core could later run in a terminal UI or in WASM without any changes.
3. **`lib.rs` + `main.rs` = two crates in one package.** Integration tests in `tests/` can only use a *library* crate, so this split makes them possible. `main.rs` imports the library by the package name, with `-` changed to `_`: `use chip8_rs::chip8::Chip8;` (rule `proj-lib-main-split`).

## Rust module rules you'll run into

- **One file = one module**, but a file is only compiled if something declares it: `pub mod chip8;` in `lib.rs` loads `src/chip8.rs`. Without that line, the file is ignored.
- **Everything is private by default.** That includes the struct, its fields, `new`, `load_rom` and the error enum. Anything `main.rs` uses needs `pub`.
- **Keep the fields private** and only make the methods public. `main` should call `chip8.load_rom(...)`, not reach into `chip8.memory`. This keeps the CPU's rules, like PC wrapping or memory masking, impossible to bypass. Your debug `println!` of `memory[0x200..]` will then fail to compile, which is fine: delete it, or add a small read-only method for debugging.
- **Put unit tests inside `chip8.rs`** (`#[cfg(test)] mod tests { use super::*; }`). A child module can see its parent's private items, so tests can check `memory` directly without making it public. Integration tests in `tests/` only see the public API. Use them later for whole-ROM runs.

## Performance

There's no runtime cost. Modules are only a way to organize names; they don't exist at runtime. The library and the binary are separate crates, which used to block inlining across them. Since Rust 1.75, small functions are inlined across crates automatically, and the hot fetch/decode/execute loop will live entirely inside the library anyway. Only the single `run`/`step` call crosses the boundary.

## Later splits

Split only when a piece has its own job, not one file per type:
- **`opcode.rs`**: decoding `u16` into an `enum Instruction`, when the `match` grows large.
- **`display.rs`**: the 64×32 framebuffer and the XOR sprite drawing (`DXYN`).
- **`font.rs`**: the 80-byte `const FONT: [u8; 80]`.

Avoid `mod.rs` folders until a module has child modules of its own.

Do the `lib.rs` / `chip8.rs` split now, then write the tests inside `chip8.rs`.

### 🧑 David

review my project structure now

> 🔧 **Bash** — Show project files, build, test, run

### 🤖 Claude

Your split is done right: everything builds, the four command-line cases behave the same as before (with the expected exit codes), and the only warning is the expected `dead_code` one. `cargo test` still runs 0 tests.

## What's right

- **`main.rs` only handles the outside world:** arguments, the file, stderr and the exit code. `chip8.rs` doesn't do any I/O.
- **The fields are private** and only `new` and `load_rom` are `pub`, so `main` can't reach into `memory`. You also removed the debug `println!` instead of making `memory` public to keep it.
- **`impl Default` that forwards to `new()`** is the idiomatic way. Clippy's `new_without_default` lint asks for it on any public type with a no-argument `new`. You couldn't write `#[derive(Default)]` because `[u8; 4096]` doesn't implement `Default`, so implementing it by hand was the right call.
- `MAX_ROM_SIZE` and the constants stay private to the module, because nobody outside needs them.

## Improvements

1. **Repeated name in the path: `chip8_rs::chip8::Chip8`.** Re-export the main types from `lib.rs` so users write `use chip8_rs::Chip8;`:
   - `lib.rs` becomes `mod chip8;`, now private, followed by `pub use chip8::{Chip8, LoadError};`.
   - The file layout stays as it is, but the public API is flat (rule `proj-pub-use-reexport`). This also lets you move code between files later without breaking `main.rs`.
2. **Doc comments on the public API.** It's now a library, so add `///` on `Chip8`, `new`, `load_rom` and `LoadError`. Add a `//!` line at the top of `lib.rs` describing the crate. For `load_rom`, include an `# Errors` section saying it returns `RomTooLarge` above 3584 bytes (rules `doc-all-public`, `err-doc-errors`). `cargo doc --open` shows the result.
3. **`#[must_use]` on `new()`.** `Chip8::new();` on its own line builds 4 KB and throws it away, which is always a mistake. Clippy pedantic suggests this with `must_use_candidate`.
4. **Leftovers from earlier reviews in `chip8.rs`:**
   - Line 13: use `fmt::Formatter<'_>` and `fmt::Result`, since you import `fmt`.
   - Line 16: use inline arguments, `"{size} bytes, max is {max}"`.
5. **`buffer` in `main.rs`:** call it `rom`. It matches the `load_rom(rom: &[u8])` parameter and says what it contains.

## Not worth doing yet

- **Splitting `LoadError` into `error.rs`:** wait until you have several error types.
- **`#[derive(Debug)]` on `Chip8`:** it would print all 4096 bytes. If you want debug output later, write a custom `Debug` that only shows the registers and PC.
- **A `[profile.release]` in `Cargo.toml`:** wait until you measure speed.
- **`test_suite/error_roms/big.ch8`:** once the tests build oversized ROMs in memory, this file isn't needed. Keep it only as a manual command-line check.

Next step: the tests in a `#[cfg(test)] mod tests` at the bottom of `chip8.rs`. Write three cases: 3584 bytes loads, 3585 bytes is rejected, and the bytes land at `0x200`.

### 🧑 David

Add doc comments yourself.

> 🔧 **Bash** — Show current source files
> 🔧 **Bash** — Add doc comments, run doctests and cargo doc
> 🔧 **Bash** — Fix private doc links, rebuild docs, show result

### 🤖 Claude

I added doc comments to all three files and changed nothing else in the code. `cargo doc` builds with no warnings, and the two code examples I put in the docs run and pass under `cargo test`. You still have 0 regular tests. I ran `cargo fmt` afterwards, but it didn't reformat any of your code.

## What I added

**`lib.rs`** got a `//!` crate description: the library holds the emulator core and the binary handles the command line, files, and later the display and input.

**`chip8.rs`:**
- A `//!` module description saying the module does no I/O.
- `///` on each constant. `PROGRAM_START` also explains why the first 512 bytes are reserved.
- `LoadError`, its variant and both fields (`size`, `max`) are documented.
- Each `Chip8` field is documented, including the note that `VF` doubles as the flag register.
- `new()` has an `# Examples` section.
- `load_rom()` has an `# Errors` section (fails above 3584 bytes and leaves memory unchanged in that case) and an `# Examples` section.
- The `impl Default` has a line saying it's the same as `new()`.

**`main.rs`** got one line each on `run` and `main`.

## Conventions worth knowing

- **`//!` vs `///`:** `//!` documents the item it sits *inside* (the crate or module), while `///` documents the item *right after* it.
- **Links between docs:** writing `` [`Chip8::load_rom`] `` makes rustdoc turn it into a link, and it warns you if the target is renamed or removed.
- **Public docs can't link to private items.** My first version linked `PROGRAM_START` from the public `new`/`load_rom` docs, and `cargo doc` warned that the link would be broken for anyone reading the public docs. Those now say `0x200` in plain text. The docs on private fields can still link to it, because they're only rendered with `--document-private-items`.
- **Code examples in docs run as tests.** The `load_rom` example is a real test of the error path: it uses `matches!` on `RomTooLarge { size: 4000, max: 3584 }`. It ends with the hidden line `# Ok::<(), LoadError>(())`, which lets the example use `?` without showing that line in the rendered docs (rules `doc-question-mark`, `doc-hidden-setup`). Note that these examples can only use the public API (`chip8_rs::...`).

Run `cargo doc --open` to see the result, or add `--document-private-items` to include the fields and constants.

I didn't add `#[must_use]` or fix the `Display` style nits, because you asked for docs only. The three unit tests (3584 bytes loads, 3585 is rejected, bytes land at `0x200`) are still yours to write in `chip8.rs`.

### 🧑 David

What is must_use

### 🤖 Claude

`#[must_use]` is an attribute that makes the compiler **warn when a value is thrown away without being used**. It's a check done at compile time only, with **no runtime cost**: the compiled program is identical.

## Where you've already seen it

`Result` is declared in the standard library as:

```rust
#[must_use = "this `Result` may be an `Err` variant, which should be handled"]
pub enum Result<T, E> { ... }
```

This is why `chip8.load_rom(&buffer);` without `?` gave you an `unused_must_use` warning earlier. An ignored `Result` is a swallowed error.

## Two places to put it

**On a type**, like `Result` above: every function that returns that type is covered automatically.

**On a function**: the return value of *that function* must be used.
```rust
#[must_use]
pub fn new() -> Self { ... }

Chip8::new();   // warning: unused return value of `Chip8::new` that must be used
```

You can add a reason, which is shown in the warning: `#[must_use = "creating a Chip8 does nothing on its own"]`.

## When to use it

Use it when **the return value is the whole point** of calling the function, so ignoring it is always a bug:
- Constructors (`new`) and builder methods.
- **Pure functions** that compute a new value without changing anything.

Don't use it when the function's real job is a side effect and the return value is just extra information. For example, `HashMap::insert` returns the old value, but ignoring it is normal, so it isn't marked.

## Why it matters for your emulator

The standard library marks the integer methods with it: `wrapping_add`, `checked_add`, `saturating_sub` and so on. These **return** a new value and **don't change** the original. Here's a classic CHIP-8 bug when implementing `7XNN` (add NN to VX):

```rust
self.variables[x].wrapping_add(nn);   // computes the sum... and throws it away
```

VX never changes, and the program fails in strange ways. Thanks to `#[must_use]` the compiler warns you: *"this returns the result of the operation, without modifying the original"*. The correct form assigns the result back to the register.

## Silencing it on purpose

`let _ = value;` tells the compiler "I'm deliberately ignoring this". Use it only when you've actually decided that, which is why I suggested removing it earlier when you didn't need it.

**Linting:** Clippy pedantic's `must_use_candidate` flags public functions that look like they should have it. Your `new()` is one, which is why I suggested it.

### 🧑 David

Make my tests

> 🔧 **Bash** — Show the rest of chip8.rs
> 🔧 **Bash** — Append unit tests module and run tests

### 🤖 Claude

I'll check that the tests catch real bugs by temporarily putting back your earlier "copy to address 0" bug:

> 🔧 **Bash** — Mutation check: reintroduce old bug, run tests, restore
> 🔧 **Bash** — Remove temporary backup file

### 🤖 Claude

I added 8 unit tests in a `#[cfg(test)] mod tests` block at the bottom of `src/chip8.rs`, and they all pass. To check they actually catch bugs, I put your earlier "copy to address 0" bug back in for a moment: 4 of the 8 failed. Then I restored the file.

| Test | What it checks |
|---|---|
| `new_starts_pc_at_program_start_with_zeroed_state` | PC is `0x200`; `I`, both timers, the registers and all of memory are zero |
| `load_rom_copies_bytes_at_program_start` | The ROM bytes end up at `0x200..` |
| `load_rom_leaves_memory_outside_rom_untouched` | Nothing is written below `0x200` or after the ROM |
| `load_rom_accepts_empty_rom` | A 0-byte ROM loads without error and changes nothing |
| `load_rom_accepts_rom_of_exactly_max_size` | 3584 bytes is accepted, and the last byte lands at `0xFFF` |
| `load_rom_rejects_rom_one_byte_over_max_size` | 3585 bytes is rejected with `RomTooLarge { size: 3585, max: 3584 }` |
| `load_rom_rejected_rom_does_not_modify_memory` | A rejected ROM doesn't overwrite a ROM that was already loaded |
| `load_error_display_shows_size_and_max` | Checks the exact error message text |

## Techniques worth noting

- **`use super::*;`** gives the test module access to the parent module's private items. That's how the tests can read `chip8.memory` and `program_counter` while those fields stay private.
- **Testing on both sides of the limit (3584 and 3585 bytes).** Off-by-one bugs sit exactly at the limit. Writing `>=` instead of `>` would pass a "4000 bytes is rejected" test, but fails `accepts_rom_of_exactly_max_size`.
- **ROMs with a pattern, not zeros.** `patterned_rom` uses `(0..=u8::MAX).cycle().take(len).collect()`, so bytes copied to the wrong place or in the wrong order are caught. An all-zero ROM written over all-zero memory looks correct even when it isn't. The pattern also avoids an `as u8` cast, which would silently cut off larger values and which Clippy pedantic warns about.
- **`matches!` with a guard (`if size == ... && max == ...`)** checks the variant and its field values without deriving `PartialEq` on `LoadError`.
- **`.unwrap()` is fine in tests.** If it panics, the test fails and shows the error, which is what you want. The "no unwrap" rule only applies to production code.
- **One behavior per test, and names that describe it.** When a test fails, its name tells you what broke.

Run a single test with a substring of its name, e.g. `cargo test rejects`. Run the 8 unit tests without the documentation examples with `cargo test --lib`.

The next step that needs tests is `fetch`. Check that it reads a big-endian opcode and moves the PC forward by 2.

### 🧑 David

What is the fetch method

### 🤖 Claude

`fetch` is the first of the three steps the CPU repeats forever, the **fetch → decode → execute** cycle:

```
loop {
    opcode = fetch()      // read the next instruction from memory
    decode(opcode)        // split it into parts: kind, X, Y, N, NN, NNN
    execute(...)          // change registers, memory, display, PC
}
```

The fetch step has one job: **read the instruction at the PC and move the PC to the next one.**

## What it does, step by step

1. **Read two bytes.** Every CHIP-8 instruction is 2 bytes long, but memory is `[u8]`, so you read `memory[pc]` and `memory[pc + 1]`.
2. **Combine them into one `u16`, big-endian.** The first byte is the high byte. The IBM ROM starts with the bytes `00 E0`, which is opcode `0x00E0` (clear screen), not `0xE000`.
3. **Advance the PC by 2** so it points to the next instruction.
4. **Return the opcode** for decode/execute.

The signature follows from this: `fn fetch(&mut self) -> u16`. It needs `&mut self` because it changes the PC, and it returns a `u16` because opcodes are 16 bits.

## Why the PC moves *before* execute

This is a CHIP-8 rule, and the other instructions depend on it:
- **Jumps** (`1NNN`) simply overwrite the PC. If the PC moved after execute, every jump would land 2 bytes too far.
- **Skips** (`3XNN`, `4XNN`, `5XY0`, `9XY0`, `EX9E`, `EXA1`) add another 2 to skip the next instruction.
- **Call** (`2NNN`) pushes the PC onto the stack, and by then it already holds the *return address*: the instruction after the call.
- **`FX0A`** (wait for a key) works by moving the PC *back* by 2, so the same instruction runs again.

## Combining the bytes

There are two equivalent ways to combine the bytes:
- `u16::from_be_bytes([hi, lo])` names the byte order explicitly (`be` = big-endian). It's easy to read and hard to get wrong.
- `(u16::from(hi) << 8) | u16::from(lo)` does it with a shift and an OR. You have to widen to `u16` **before** shifting, because a `u8` shifted left by 8 is always 0 (and is a compile error for a constant shift).

Both compile to the same machine code, typically two byte loads plus a shift/OR, or one 16-bit load and a byte swap on x86, which is little-endian. Pick the one that reads best; `from_be_bytes` is the more idiomatic.

## The bounds trap

If the PC reaches `0xFFF`, then `pc + 1` is `0x1000`, which is outside memory, and indexing **panics**. A buggy or malicious ROM can jump there. Two choices:
- **Wrap like the hardware:** mask each address with `& 0xFFF`, so `0xFFF + 1` reads address `0x000`. Masking also lets the compiler drop the bounds checks, as discussed at the start. It's the cheapest option and matches the 12-bit address bus.
- **Return an error** such as `Result<u16, ...>` and stop the emulator. It's stricter, but adds a branch and a `Result` on every instruction in the hottest code path.

Masking is the usual choice. The PC also needs to wrap when you add 2, using a mask or `wrapping_add` depending on its type. This is where your choice of `usize` vs `u16` for `program_counter` starts to matter.

## Visibility and tests

- **Keep `fetch` private** (no `pub`). It's an internal step. Later you'll expose a public `step()` or `cycle()` that does fetch, decode and execute, and `main` only calls that.
- **Tests:** load `[0x12, 0x34]` and check that `fetch()` returns `0x1234` (not `0x3412`) and that the PC is now `0x202`. Then add a test with the PC set to `0xFFE` or `0xFFF` to check the wrapping you chose.

Try writing it, and I'll review it.

### 🧑 David

review my fetch method

> 🔧 **Bash** — Show fetch method, build and test

### 🤖 Claude

Your `fetch` reads the opcode correctly, but **it never advances the PC**, and it can **panic at the top of memory**. It compiles and your 8 existing tests still pass, but nothing tests `fetch` yet.

## What's right

- **`u16::from_be_bytes([hi, lo])`** gives the right byte order and the clearest code. `00 E0` becomes `0x00E0`.
- **Private** (no `pub`): it's an internal step, as it should be.
- **`#[must_use]`** fits: calling `fetch` and ignoring the opcode is always a bug.
- **Indexing with `usize`** works directly because `program_counter` is a `usize`, so no conversion is needed.

## 1. The PC never moves (the main issue)

The method takes `&self`, so it *can't* change `program_counter`. Every call returns the same opcode, and the emulator would run the first instruction forever.

You have two options:
- **Advance inside `fetch`:** change it to `&mut self` and add 2 to the PC after reading. "Read and move to the next instruction" is then one indivisible operation that can't be forgotten. **I recommend this.** It's what the spec describes, and it guarantees the PC has already moved when execute runs, which jumps, skips and calls depend on.
- **Keep it `&self` as a pure "read" and advance in `step()`:** this works too, but a future caller could use `fetch` and forget to advance. If you go this way, rename it to something like `peek_opcode`, because "fetch" suggests moving forward.

## 2. Panic at the end of memory

If `program_counter` is `0xFFF`, then `self.program_counter + 1` is `4096`, one past the end, and the index **panics**. A ROM that jumps to `0xFFF` (`1FFF`) would crash your emulator instead of misbehaving the way a real CHIP-8 does.

The fix is the hardware's 12-bit wrap: **mask both indices with `& 0xFFF`** and mask the PC again when you add 2. This has two benefits:
- **Correctness:** `0xFFF + 1` reads address `0x000`, like a 12-bit address bus. It can never panic.
- **Speed:** today each `fetch` has **two bounds checks**, each a compare and a branch to a panic path, in the hottest function of the emulator. A value masked with `0xFFF` is always below 4096 = `memory.len()`, so the compiler can remove both checks. An AND instruction costs less than a compare plus a branch.

Instruction addresses don't have to be even. ROMs can jump to odd addresses, so don't add an alignment assumption.

**Choosing the PC type:** with `program_counter: usize`, you mask every time you advance or jump. With `u16`, you'd still mask, because 16 bits is wider than 12, *and* you'd convert to `usize` when indexing. Both are fine. `usize` saves the conversions, `u16` is closer to the hardware register. Just mask consistently in one place.

## 3. Missing tests

Add tests once the PC advances:
- Load `[0x12, 0x34]`; `fetch()` returns `0x1234` (not `0x3412`), and the PC is now `0x202`.
- Two fetches in a row return two consecutive opcodes. This checks the PC really advances.
- PC set to `0xFFF`: reads `memory[0xFFF]` and `memory[0x000]`, no panic, and the PC wraps to `0x001`. Tests in the same module can set the private PC directly.
- PC set to `0xFFE`: the PC wraps to `0x000`.

## Small notes

- The new `method fetch is never used` warning is expected until a `step()` calls it. Tests don't count as "used" for the normal build.
- No `#[inline]` needed: the function is private and in the same crate, so the compiler can see it and will inline it anyway.

### 🧑 David

review my fetch method now. then add tests and doc.

> 🔧 **Bash** — Show fetch method, build and test
> 🔧 **Bash** — Show pc_jump_to

### 🤖 Claude

Below is the review first. Then I'll add the tests and docs without changing your code. Some of the new tests are expected to **fail**, because they test the wrapping that isn't fixed yet.

> 🔧 **Bash** — Add docs and fetch tests, run unit tests

### 🤖 Claude

Your `fetch` now reads the opcode and advances the PC, but the wrap at the end of memory still has two bugs. I added 9 tests and doc comments without changing your code, and 3 of the new tests fail because of those bugs. Out of 17 unit tests, 14 pass.

## Review

**What's right:**
- `&mut self`, and the PC advances inside `fetch`, so reading and moving to the next instruction is one step that can't be split.
- The jump logic lives in one helper, `pc_jump_to`, which `1NNN`, `2NNN`, `00EE` and `BNNN` can reuse later.
- `from_be_bytes` and `#[must_use]` are kept.

**Bug 1: off-by-one in `pc_jump_to`.** The check `to_jump > MEMORY_SIZE` should be `>=`. With PC = `0xFFE`, `+ 2` gives `4096`, which isn't *greater* than 4096, so the PC is set to 4096. The next fetch then panics. Two tests catch this: `left: 4096, right: 0`.

**Bug 2: `memory[pc + 1]` is still not wrapped.** With PC = `0xFFF`, the second index is `4096` and it panics on line 142. `pc_jump_to` only protects the PC itself, not the second byte you read. The test `fetch_at_0xfff_...` shows the panic.

**Fixing both, efficiently:** use the modulo/mask operation instead of a subtraction:
- `x % MEMORY_SIZE` (or `x & 0xFFF`) is correct for **any** value. Your subtraction only works for values below 8192, and the `if` adds a branch.
- `MEMORY_SIZE` is a constant power of two, so the compiler turns `% MEMORY_SIZE` on a `usize` into a single AND. You get readable code and the speed of a mask, with no branch.
- Apply it to the **second index** in `fetch` as well, i.e. `(pc + 1) % MEMORY_SIZE`. Once every index is reduced like this, the compiler can prove each one is below 4096 and drop the bounds checks.

**Style:** use `res` as the final expression instead of `return res;`. In Rust, a function returns the value of its last expression when it has no `;`. Clippy flags this as `needless_return`.

## What I added

**Docs:** `///` on `fetch` (big-endian, why the PC moves before execute, the wrap at `0xFFF`) and on `pc_jump_to` (wraps at 12 bits). They're private, so view them with `cargo doc --document-private-items`.

**Tests**:

| Test | Result |
|---|---|
| `fetch_combines_two_bytes_big_endian`: `12 34` → `0x1234` | ok |
| `fetch_advances_pc_by_two` | ok |
| `fetch_twice_returns_consecutive_opcodes` | ok |
| `fetch_works_at_odd_address` (ROMs may jump to odd addresses) | ok |
| `fetch_at_0xffe_wraps_pc_to_zero` | **FAILED** (bug 1) |
| `fetch_at_0xfff_reads_last_byte_then_first_byte` | **FAILED** (bug 2) |
| `pc_jump_to_in_range_sets_pc` | ok |
| `pc_jump_to_end_of_memory_wraps_to_zero` | **FAILED** (bug 1) |
| `pc_jump_to_past_end_of_memory_wraps` | ok |

The tests set `program_counter` and `memory` directly, since the test module can see private fields, so they can reach the edge cases without a ROM that jumps there.

Fix the two bugs, then run `cargo test --lib`. When all 17 pass, the wrap is correct.

### 🧑 David

review my fetch method now

> 🔧 **Bash** — Show fetch and pc_jump_to, build, test

### 🤖 Claude

Both bugs are fixed, and all 17 unit tests and both doc examples pass.

## What's right

- **`pc_jump_to` uses `% MEMORY_SIZE`.** It's correct for any value, has no branch, and because `MEMORY_SIZE` is a power-of-two constant, the compiler turns it into a single AND.
- **The second byte's address wraps:** `(self.program_counter + 1) % MEMORY_SIZE`. Fetching at `0xFFF` now reads `0xFFF` then `0x000` instead of panicking.
- **The PC advances through `pc_jump_to`.** Every write to the PC goes through the one function that wraps it, so the PC can't end up outside memory.

## Remaining points

1. **`return res;` → `res`.** As a final expression without `;`, `res` is the return value. Clippy flags this as `needless_return`. You could even drop the variable by building the opcode, advancing, then returning, but that ordering requires the variable, so keeping `res` and removing `return` is the right fix.

2. **The first index still has a bounds check.** *You* know `program_counter < 4096`, because all writes go through `pc_jump_to`. **The compiler doesn't**: a field's value can't be tracked across separate calls, so `self.memory[self.program_counter]` keeps a compare and a branch to a panic path.
   - **Option A:** index with `self.program_counter % MEMORY_SIZE` too. It's one AND per fetch, the check goes away, and the code is safe even if a test or a future opcode sets the field directly.
   - **Option B:** leave it as is. One correctly predicted branch per instruction costs almost nothing at about 700 instructions per second, and the panic would point out a broken invariant.

   Both are defensible. A is more consistent with how you index the second byte.

3. **Write the invariant down.** Add a line to the `program_counter` field doc: "Always `< MEMORY_SIZE`; only modify through `pc_jump_to`". This rule is what keeps the code correct, and right now it only exists in your head. A later step would be a newtype like `struct Address(u16)` whose only constructor masks to 12 bits. Then the compiler enforces the rule (rule `type-newtype-validated`). This isn't needed yet; consider it when `I` and the stack also hold addresses.

The `never used` warnings will go away once a `step()` calls `fetch`.

## Next: decode

Split the `u16` opcode into its parts using shifts and masks:

| Part | Bits | Extraction |
|---|---|---|
| kind (1st nibble) | `F000` | `(op >> 12) & 0xF` |
| `X` | `0F00` | `(op >> 8) & 0xF` |
| `Y` | `00F0` | `(op >> 4) & 0xF` |
| `N` | `000F` | `op & 0xF` |
| `NN` | `00FF` | `op & 0xFF` |
| `NNN` | `0FFF` | `op & 0xFFF` |

Then `match` on the nibbles. A `match` on a tuple like `(kind, x, y, n)` with patterns such as `(0x0, 0x0, 0xE, 0x0) => clear screen` is the idiomatic way to write it. The compiler turns it into a jump table, which is faster than a chain of `if`s. Start with the IBM subset: `00E0`, `1NNN`, `6XNN`, `7XNN`, `ANNN`, `DXYN`.

### 🧑 David

review my fetch method now then explain me how do the decode function

> 🔧 **Bash** — Show struct, fetch, build, test

### 🤖 Claude

## `fetch`: done ✅

All 17 tests pass, and both points from the last review are fixed:
- `return res;` is now just `res`.
- The first index is also reduced with `% MEMORY_SIZE`. Both reads are now provably below 4096, so the compiler can remove both bounds checks.

One optional thing is still open: the invariant on the `program_counter` field. Add a doc line like "always `< MEMORY_SIZE`; write it only through `pc_jump_to`". There's nothing else to change; `fetch` is finished.

---

## How to design `decode`

### 1. Two designs

**A. Decode and execute in one big `match`.** Each match arm runs the instruction directly: `(0x0, 0x0, 0xE, 0x0) => self.clear_screen()`. It's the simplest and the most common in tutorials.

**B. Decode into an `enum Instruction`, then execute it.**

```rust
enum Instruction {
    ClearScreen,                 // 00E0
    Jump(u16),                   // 1NNN
    SetRegister { x: usize, nn: u8 },  // 6XNN
    // ...
}
fn decode(opcode: u16) -> Result<Instruction, ...>
fn execute(&mut self, instr: Instruction)
```

**I recommend B.** The extra layer is worth it because:
- **`decode` is a pure function** (`u16` in, `Instruction` out, with no `self`). You can test it without building a machine: `assert_eq!(decode(0x6A42), SetRegister { x: 0xA, nn: 0x42 })`.
- **The bit manipulation is done once.** Execute arms get named, already-correctly-typed fields, so no `& 0xF` is scattered around 35 arms.
- **Unknown opcodes become a value** (`Err`) instead of a panic in the middle of execution.
- **It enables a disassembler or debugger later.** Printing `Instruction` with `Debug` gives you a readable trace for free.

**The cost is about zero.** `decode` is small and in the same crate, so it gets inlined into `step()` and the compiler can usually remove the intermediate enum completely. Even when it doesn't, the enum is tiny: its size is the largest variant plus the tag, about 4 bytes if you keep the fields as `u8`/`u16`. Making `x` a `usize` makes the enum bigger (16 bytes) but avoids a conversion in every execute arm. Either is fine; just decide on purpose.

### 2. Extracting the nibbles without casts

You *can* shift and mask the `u16` (`(op >> 8) & 0xF`). But the results are `u16`, and narrowing to `u8` needs `as u8`, which Clippy pedantic warns about (`cast_possible_truncation`).

A cleaner trick: split the opcode back into its bytes with `let [hi, lo] = opcode.to_be_bytes();`. Then everything is a `u8` with no casts:

| Part | From the bytes |
|---|---|
| kind | `hi >> 4` |
| `X` | `hi & 0x0F` |
| `Y` | `lo >> 4` |
| `N` | `lo & 0x0F` |
| `NN` | `lo` (no operation needed) |
| `NNN` | `opcode & 0x0FFF` (keep it `u16`) |

`let [hi, lo] = ...` is **array destructuring**: it works because `to_be_bytes()` returns `[u8; 2]`, whose length is known at compile time. It costs nothing, since it's the same bits viewed differently.

Precedence: in Rust, `>>` binds tighter than `&`, but add parentheses anyway when you mix them. Readers shouldn't have to remember the precedence table.

### 3. The `match`

Match on a **tuple of the four nibbles** and use `_` for "any value":

```rust
match (kind, x, y, n) {
    (0x0, 0x0, 0xE, 0x0) => Ok(Instruction::ClearScreen),
    (0x1, _, _, _)       => Ok(Instruction::Jump(nnn)),
    // ...
    _ => Err(...unknown opcode...),
}
```

- **Arm order matters:** the first matching arm wins. Put specific patterns (`00E0`, `00EE`) before general ones. Otherwise a `(0x0, _, _, _)` arm would catch `00E0`.
- **The final `_` arm is required here.** It covers the opcodes that aren't defined (e.g. `5XY1`, `8XY8`, `EX00`). Return an error carrying the raw opcode, such as `DecodeError::UnknownOpcode(u16)`. Don't panic, because a bad ROM is input, not a bug in your program. Display it as `{:04X}` so the error message shows `0x5XY1` in the familiar hexadecimal form.
- **Performance:** the compiler turns a `match` on small integers into a jump table or a binary tree of comparisons. That's faster than a chain of `if/else`, and you didn't have to write it by hand.

### 4. Free bounds checks on the registers

If `x` is a nibble (`hi & 0x0F`, so at most 15) and `variables` is `[u8; 16]`, then `self.variables[usize::from(x)]` can **never** be out of range. The compiler proves it and removes the check. This is a direct benefit of using a fixed-size array instead of a `Vec`: the length is part of the type, so the optimizer knows it.

### 5. Putting it together

```rust
pub fn step(&mut self) -> Result<(), ...> {
    let opcode = self.fetch();
    let instr = decode(opcode)?;
    self.execute(instr);
    Ok(())
}
```

This is the public method `main` will call about 700 times per second. Once it exists, the `never used` warnings for `fetch` and `pc_jump_to` go away.

### Where to start

1. Define `Instruction` with only the IBM subset (`00E0`, `1NNN`, `6XNN`, `7XNN`, `ANNN`, `DXYN`) and write `decode` for them. Everything else goes to the `_` error arm for now.
2. Write tests for `decode` alone, one per instruction plus one unknown opcode. `#[derive(Debug, PartialEq)]` on `Instruction` lets `assert_eq!` compare them.
3. Then write `execute` for the easy ones: `1NNN` uses your `pc_jump_to`, then `6XNN`, `7XNN` (use `wrapping_add`, which is `#[must_use]`, so assign the result!) and `ANNN`. Leave `00E0` and `DXYN` until the display buffer exists.

Once `decode.rs` (or `instruction.rs`) holds the enum and its decode function, it's a good point to split it into its own module.

### 🧑 David

Is it a good way to get the correct instruction in my decode function ?

> 🔧 **Bash** — Show decode code, build, test
> 🔧 **Bash** — Show Instruction enum

### 🤖 Claude

Your approach (one `match` on a tuple of the opcode's parts) is the right idea, but it doesn't compile yet and has a bug in the jump.

## 1. It doesn't compile

```
error[E0308]: expected `Box<dyn Error>`, found `String`
165 |  _ => Err("Unkniwn instruction".to_string()),
```

`?` converts error types automatically by calling `From::from`, but **writing `Err(...)` yourself doesn't**. You could write `.into()`, but the real fix is the error type itself (point 4).

## 2. Bug: `opcode & 7`

`7` is `0b0000_0000_0111`, so it keeps only the 3 lowest bits. `1228` would jump to `0x0`, not `0x228`. `NNN` is the low 12 bits: `opcode & 0x0FFF`. Use hexadecimal masks for anything about nibbles, so the mask lines up with the `NNN` notation in the spec.

## 3. Masking without shifting works, but makes things harder

Your tuple holds the masked values in their original positions (`0x1000`, `0x0F00`, ...):

- **The patterns are hard to read:** `(0x0000, 0x0000, 0x00E0, 0x0000)` vs `(0x0, 0x0, 0xE, 0x0)`. The second reads like the spec's `00E0`.
- **The values can't be used as is:** X is `0x0A00`, not `0xA`. Every arm that needs X or Y would have to shift it again, and that's exactly the kind of repeated bit manipulation `decode` is meant to do once.

Shift each part down to a nibble (0–15) before matching. The cleanest way, with no casts, is the bytes trick from last time:

```rust
let [hi, lo] = opcode.to_be_bytes();
let (kind, x, y, n) = (hi >> 4, hi & 0x0F, lo >> 4, lo & 0x0F);
// nn = lo, nnn = opcode & 0x0FFF
```

Then `match (kind, x, y, n)` with patterns like `(0x0, 0x0, 0xE, 0x0)` and `(0x6, _, _, _)`. The values are already `u8`s you can put directly into the variants. Performance is the same: a few shifts/ANDs, then a jump table or comparison tree in both cases.

## 4. Error type: use your own enum, not `Box<dyn Error>`

The core shouldn't return `Box<dyn Error>`:
- **It allocates** on every bad opcode.
- **It loses the information:** a string can't be matched on, and it doesn't contain *which* opcode failed.
- `Box<dyn Error>` belongs at the application edge (`run()` in `main.rs`), not in the emulator core.

Use the same pattern as `LoadError`: `enum DecodeError { UnknownOpcode(u16) }` with `Display` printing `unknown opcode {:04X}`. There's no allocation, the opcode is available for debugging, and `?` in `main` still turns it into `Box<dyn Error>`. (Also, "Unkniwn" is a typo.)

## 5. `decode` shouldn't take `&self`

It doesn't read the machine; only the opcode matters. Taking `&self` hides that it's a pure function and forces tests to build a `Chip8`. Here are the options, from good to most idiomatic:
- A free function or an associated function without `self`: `fn decode(opcode: u16) -> Result<Instruction, DecodeError>`.
- **`impl TryFrom<u16> for Instruction`**: the standard trait for a conversion that can fail (rule `conv-tryfrom-fallible`). Callers write `Instruction::try_from(opcode)?`, and anyone reading Rust knows immediately what it does.

Also remove the `#[must_use]`: `Result` is already `#[must_use]`, and Clippy flags the duplicate (`double_must_use`).

## 6. The `Instruction` enum

```rust
enum Instruction {
    ClearScreen,
    Jump(u16),
    SetRegistery { vx: usize, nn: u8 },
    AddValueRegistery { vx: usize, nn: u8 },
    SetIndex(u16),
    Display,
}
```

- **`Display` carries no data**, but `DXYN` needs X, Y and N. Use something like `Draw { x: u8, y: u8, n: u8 }`. Avoid calling it `Display`, which is also the name of the trait you implement.
- **Use `x`, not `vx`:** the field is the register's *number* X. The spec writes `Vx` for the register's *value*, so `vx` would be confusing in `execute`.
- **"Registery" should be "Register".**
- **Field types:** `x: u8` keeps the enum small, and you'd write `usize::from(x)` when indexing. `x: usize` saves that conversion but makes each variant 8 bytes bigger. Pick one and use it for every variant.
- **Add `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`.** `Debug` and `PartialEq` let tests use `assert_eq!(decode(0x6A42), Ok(SetRegister { x: 0xA, nn: 0x42 }))`. `Copy` fits because the enum is a few bytes of plain data.

## Summary

| | Keep / change |
|---|---|
| `match` on a tuple + `_` for unknown opcodes | ✅ keep |
| Masks without shifts | ➜ shift to nibbles (`to_be_bytes`) |
| `opcode & 7` | ❌ `opcode & 0x0FFF` |
| `Box<dyn Error>` + `String` | ➜ `DecodeError::UnknownOpcode(u16)` |
| `&self` | ➜ no `self`, ideally `TryFrom<u16>` |
| `Display` variant without fields | ➜ `Draw { x, y, n }` |

Once it compiles, write the `decode` tests (one per instruction plus one unknown opcode) and I'll review.

### 🧑 David

Review my decode

> 🔧 **Bash** — Locate decode code, build, test
> 🔧 **Bash** — Show errors, enum, and decode

### 🤖 Claude

Your `decode` builds, and the core of it is right: nibble extraction, correct masks, the specific `00E0` arm before the general ones, and an error that carries the opcode. The remaining points are mostly types and naming. There are no `decode` tests yet: the 17 existing tests still pass.

## What's right

- `to_be_bytes()` → `(kind, x, y, n)` as `u8` nibbles, with no casts.
- Readable patterns like `(0x0, 0x0, 0xE, 0x0)` that match how the spec writes `00E0`.
- `opcode & 0x0FFF` for `NNN` (the `& 7` bug is gone).
- `x.into()` to widen `u8` → `usize` uses `From`, so no data can be lost.
- `_ =>` returns an error carrying the opcode, not a panic and not a `String`.

## To fix

**1. `nn` and `n` should be `u8`, not `u16`.**
- `NN` is a byte, and it gets stored in an 8-bit register. With `nn: u16`, executing `6XNN` would need a narrowing `as u8`, which is exactly the cast you avoided.
- You already have it as a `u8`: it's `lo`. So `nn: lo` replaces `opcode & 0x00FF`.
- `n` is the sprite height (0–15), so it should be a `u8` too.
- The rule: give each field the type of where it ends up, so the conversion happens once in `decode` and never in `execute`.

**2. The error message prints the opcode in decimal.** `{}` on a `u16` prints `20481` for `0x5001`. Use `{opcode:#06X}` → `0x5001`: `#` adds the `0x`, `06` pads to 6 characters including the `0x`, and `X` is uppercase hexadecimal. Also start the message lowercase, as you did for `LoadError`: `"unknown opcode {opcode:#06X}"`.

**3. Error naming.** `ExecutionError::DecodeError` says "Error" twice, and the variant name doesn't say *what* went wrong. Use `ExecutionError::UnknownOpcode(u16)` (or `{ opcode: u16 }`). The type name gives the category and the variant gives the cause. Keeping a broader type `ExecutionError` is a good idea: stack overflow on `2NNN` and stack underflow on `00EE` will be other variants of it later.
- Add `///` docs to it, like `LoadError`.
- Re-export it in `lib.rs` (`pub use chip8::{..., ExecutionError}`) once the public `step()` returns it.

**4. Still `&self` and `#[must_use]`** (from last review). `decode` doesn't read the machine. Remove `&self`, or better, `impl TryFrom<u16> for Instruction`. Remove `#[must_use]`, because `Result` already has it (Clippy: `double_must_use`).

**5. The `Instruction` enum** (from last review):
- `SetRegistery` / `AddValueRegistery` → `SetRegister` / `AddToRegister`.
- `vx` / `vy` → `x` / `y`: they're register *numbers*, while `Vx` in the spec means the *value* in that register.
- `Display` → `Draw`, so it doesn't clash with the `fmt::Display` trait name.
- Add `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`, which the tests need for `assert_eq!`.
- A short `///` per variant with its opcode (`/// 6XNN: set VX to NN`) makes it easy to map back to the spec.

**6. Size (low priority).** With `usize` fields, `Draw { vx: usize, vy: usize, n: u16 }` is about 18 bytes of data, padded to 8-byte alignment, plus the variant tag: the enum is around 32 bytes. With `u8` fields it would be 4 bytes. That matters little here, because `decode` is inlined into `step()` and the value usually never reaches memory. But it's the trade-off I mentioned last time: `usize` saves a conversion in `execute`, `u8` keeps the enum tiny. Your choice is fine; `nn`/`n` as `u16` is the part to fix, because it's neither the size nor the conversion you'd want.

## Note for later

`0NNN` (call a machine-code routine on the original computer) currently ends up in `_` and returns an error. Modern emulators **ignore** it, because no ROM written for an interpreter really needs it. When you implement the full instruction set, decide whether it's a no-op or an error, and make sure `00E0`/`00EE` stay above any `(0x0, _, _, _)` arm.

## Next

1. Fix points 1–5.
2. Write the `decode` tests: one per instruction, checking the fields (e.g. `0x6A42` → `SetRegister { x: 0xA, nn: 0x42 }`, `0xD123` → `Draw { x: 1, y: 2, n: 3 }`), plus `0x5001` → `UnknownOpcode(0x5001)`. Choose operands so the different nibbles have different values; with `0x6111` a swapped X/Y or NN would go unnoticed.

### 🧑 David

Explain me more the point 6

### 🤖 Claude

I'll measure the real sizes first, rather than relying on my estimate:

> 🔧 **Bash** — Measure enum sizes for each field-type choice

### 🤖 Claude

I measured each option by compiling a small program. In my last review I estimated "around 32 bytes" for your enum: **the real size is 24**. The reason is below.

| Field types | `size_of::<Instruction>()` | alignment |
|---|---|---|
| Yours: `vx: usize`, `nn: u16` | **24 bytes** | 8 |
| `x: usize`, `nn: u8` | **24 bytes** | 8 |
| all `u8` (plus `u16` for `NNN`) | **4 bytes** | 2 |
| `Result<Instruction, u16>`, all-`u8` version | **4 bytes** | |
| `Result<Instruction, u16>`, your version | **24 bytes** | |

## 1. How an enum is stored in memory

A Rust enum is a **tagged union**:
- All variants **share the same storage**, because a value is only one variant at a time.
- So the storage is as big as the **largest variant**.
- Next to it sits a small **tag** (discriminant) recording which variant it is. You have 6 variants, so 1 byte is enough.

The size is: largest variant + tag, **rounded up to a multiple of the alignment**.

## 2. Alignment is what makes it large

Each type has an **alignment**: its address must be a multiple of that number. `u8` has alignment 1, `u16` 2, and `usize` 8 on 64-bit. The CPU loads aligned values in a single access, and the compiler guarantees that alignment.

A type's alignment is that of its most demanding field. As soon as **one** variant contains a `usize`, the **whole enum** must be 8-byte aligned, and its size rounded to a multiple of 8. This is true even for `ClearScreen`, which holds nothing.

**Your largest variant**, `Display { vx: usize, vy: usize, n: u16 }`:
```
offset  0..8   vx   (usize)
offset  8..16  vy   (usize)
offset 16..18  n    (u16)
offset 18      tag  (u8)   ← placed in what would be padding
offset 19..24  padding to reach a multiple of 8
                         = 24 bytes
```
That's why it's 24 and not 32 as I said. Rust doesn't promise to keep fields in the order you wrote them (unlike C, unless you add `#[repr(C)]`). It reorders them and **puts the tag into what would otherwise be padding**. I counted the tag as a separate 8-byte block, which was wrong.

**The all-`u8` version.** The largest variant is `Jump(u16)` (2 bytes) or `Draw { x, y, n }` (3 × `u8` = 3 bytes). The alignment is 2 because of the `u16`. 3 bytes + tag = 4, already a multiple of 2, so **4 bytes**.

Your `nn: u16` didn't change anything here (24 in both rows), because the `usize` fields dominate. Change `nn` to `u8` anyway, for the conversion reason from point 1 of the review, not for size.

## 3. Bonus: niche optimization

`Result<Instruction, u16>` is **the same size** as `Instruction`. The 1-byte tag can hold 256 values, but only 6 are used (0–5). The compiler stores `Result`'s own Ok/Err marker in one of the 250 unused values (a "niche"), so it adds no byte. This is also why `Option<&T>` and `Option<Box<T>>` are the same size as a plain pointer.

## 4. Does it matter for your emulator?

**Hardly today.** In `step()`, `decode` returns an `Instruction` and `execute` immediately consumes it. Once inlined, the value never touches memory: its fields stay in CPU registers. Even if it's not inlined:
- A 4-byte value is returned in **one register**.
- A 24-byte value is too big for registers. On x86-64 Linux, values over 16 bytes are returned through a **hidden pointer to stack memory**: a few extra stores and loads. Measurable in a microbenchmark, invisible at 700 instructions per second.

**It matters as soon as you store many of them.** For example, a cache of pre-decoded instructions (a common speed trick), one per even address = 2048 entries:
- 4 bytes × 2048 = **8 KB**, which fits easily in L1 cache (32–48 KB).
- 24 bytes × 2048 = **48 KB**, which fills L1 completely or overflows it, pushing out memory and registers.

Same code, 6× the cache usage, only because of field types.

## 5. The `usize` "saving" is almost nothing

I presented this as a trade-off: `usize` avoids a conversion in `execute`. In machine code, `u8` → `usize` is a **zero-extension** (`movzx` on x86). It usually gets merged into the load that reads the byte, so it's effectively **free**. The only real saving is typing `usize::from(x)` when indexing.

So the honest conclusion: **use `u8` for `x`, `y`, `n`, `nn` and `u16` for `nnn`.** You get the smallest enum, types that match the spec, and a conversion at the point of use that costs nothing. Your choice of `usize` wasn't wrong, but it doesn't buy anything worth 6×.

## 6. Locking the size

To make sure a future change doesn't grow the enum by accident, add a compile-time check next to it (rule `mem-assert-type-size`):

```rust
const _: () = assert!(std::mem::size_of::<Instruction>() == 4);
```

It's checked by the compiler, never runs, and costs nothing. If someone adds a `usize` field, the build fails and points to this line.

The test program is at `<scratchpad file>` if you want to try other layouts. Recompile with `rustc -O size.rs && ./size`.

### 🧑 David

Write the tests, write the code documentation and review my code

> 🔧 **Bash** — Show source files
> 🔧 **Bash** — Add docs and decode tests, run tests
> 🔧 **Bash** — Check docs build and doctests
> 🔧 **Bash** — Show public doc build output

### 🤖 Claude

I added 9 tests for `decode` and doc comments, without changing your code. **2 of the new tests fail**, and they point to a real bug in `6XNN`/`7XNN`. The other 24 unit tests pass, both documentation examples pass, and `cargo doc` builds with no warnings.

## Review

### Bug: `nn: n + y << 1`

The tests show it:
```
decode_6xnn_is_set_register ... FAILED
decode_7xnn_is_add_to_register ... FAILED
```

This expression is wrong in two ways:
1. **Precedence:** in Rust `+` binds tighter than `<<`, so it's parsed as `(n + y) << 1`.
2. **The formula itself:** even written `(y << 1) + n`, you'd need to shift `y` by **4** (one nibble), not 1.

For `0x6A42` you get `(2 + 4) << 1 = 0x0C` instead of `0x42`. The silent part: shifting a `u8` drops the high bits without any panic, even in debug builds, so nothing warns you.

**Fix:** `NN` is exactly the low byte, which you already have: `nn: lo`. No arithmetic, nothing to get wrong. Combining two nibbles back into the byte you just split them from is a roundabout way to get `lo`.

Choosing `0x6A42` as test input caught this. With `0x6111`, `(1 + 1) << 1 = 4` would still differ from `0x11`, but test values like `0x6000` would have hidden it.

### To clean up

1. **`x: x`, `y: y`, `n: n` → `x`, `y`, `n`.** Use field init shorthand when the variable has the same name as the field. Clippy flags this as `redundant_field_names`.
2. **`decode` still takes `&self` and has `#[must_use]`** (this is the third review where I've raised it). Nothing in `decode` uses `self`, which is why every test has to build a `Chip8` it doesn't need. `#[must_use]` on a function returning `Result` is redundant. Making it `impl TryFrom<u16> for Instruction` would make the tests just `Instruction::try_from(0x6A42)`.
3. **Add `#[derive(Debug, Clone, Copy, PartialEq, Eq)]` on `Instruction`.** I wrote the tests with `matches!` and literal patterns so I didn't have to touch your enum. But when a `matches!` test fails, it only says `assertion failed`. With `Debug` + `PartialEq` you can use `assert_eq!(..., Ok(Instruction::SetRegister { x: 0xA, nn: 0x42 }))`, and a failure prints `left: SetRegister { x: 10, nn: 12 }` vs `right: ...`, which shows the bug immediately. That needs `PartialEq` on `ExecutionError` too.
4. **Lock the 4-byte size** with `const _: () = assert!(std::mem::size_of::<Instruction>() == 4);`. Your fields now match the all-`u8` layout I measured.
5. **`LoadError`'s `Display` (lines 47–51)** still uses `std::fmt::Formatter` / `std::fmt::Result` and positional `{}` arguments. `ExecutionError` right above it already uses the clean style, so make them consistent.

### What's right

- Nibble extraction with `to_be_bytes()`, no casts.
- `& 0x0FFF` for `NNN`.
- The `00E0` arm is specific, and `00E1` correctly falls through to "unknown". I added a test for that near-miss.
- `ExecutionError::UnknownOpcode { opcode }` with `{opcode:#06X}` prints `unknown opcode: 0x5001`.
- `nn`, `n`, `x`, `y` are all `u8`, as discussed.

## What I added

**Docs:**
- `ExecutionError` and its variant/field.
- `Instruction`: a line on opcode notation (`X`, `Y`, `N`, `NN`, `NNN`), one doc per variant naming its opcode and effect (e.g. "`7XNN`: add `NN` to `VX`, wrapping; `VF` is not changed"), and each field.
- `decode`, with an `# Errors` section.

**Tests:**

| Test | Opcode | Result |
|---|---|---|
| `decode_00e0_is_clear_screen` | `00E0` | ok |
| `decode_1nnn_is_jump_to_nnn` | `1ABC` → `Jump(0xABC)` | ok |
| `decode_6xnn_is_set_register` | `6A42` → `x: 0xA, nn: 0x42` | **FAILED** |
| `decode_7xnn_is_add_to_register` | `7B3C` → `x: 0xB, nn: 0x3C` | **FAILED** |
| `decode_annn_is_set_index` | `A123` → `SetIndex(0x123)` | ok |
| `decode_dxyn_is_draw` | `D123` → `x: 1, y: 2, n: 3` | ok |
| `decode_unsupported_opcode_returns_unknown_opcode` | `5001` | ok |
| `decode_near_miss_of_00e0_is_unknown` | `00E1` | ok |
| `execution_error_display_shows_opcode_in_hex` | prints `"unknown opcode: 0x5001"` | ok |

Every operand uses distinct nibble values, so a swapped `X`/`Y`, a wrong shift or a wrong mask always gives a visibly different result.

Fix `nn` to `lo` and all 26 tests should pass. Next step is `execute` for the four instructions that don't need the display (`1NNN`, `6XNN`, `7XNN`, `ANNN`), then `step()` to connect fetch, decode and execute.

### 🧑 David

review my decode now, add unit tests yourself

> 🔧 **Bash** — Find decode-related code
> 🔧 **Bash** — Show errors, execute, step; build and test
> 🔧 **Bash** — Check Instruction derives and stale field references
> 🔧 **Bash** — Fix stale test field, add decode tests, run in patched copy

### 🤖 Claude

Your `decode` is now correct. With the two missing `execute` arms stubbed in the scratchpad copy, all 33 unit tests pass, including the 15 for `decode`. Your real `src/` still won't compile until you handle `ClearScreen` and `Draw` in `execute`.

## Review of `decode`

**Fixed:** `nn: lo`. The two tests that failed last time (`6A42`, `7B3C`) now pass.

**Left to clean up** (style only, not bugs):
1. **`x: x`, `y: y`, `n: n`**: use the shorthand `x`, `y`, `n` (Clippy: `redundant_field_names`).
2. **`&self` + `#[must_use]`**: it's still a pure function, yet every test builds an unused `Chip8`. Remove `&self`, or use `impl TryFrom<u16> for Instruction`. Drop the `#[must_use]`, since `Result` already has it.
3. **No `#[derive(Debug, Clone, Copy, PartialEq, Eq)]` on `Instruction`**: the tests are stuck with `matches!`, which only reports `assertion failed` without showing the value it got.

## New tests

| Test | What it checks |
|---|---|
| `decode_6xnn_extracts_max_register_and_value` | `6FFF` → `x: 0xF, nn: 0xFF`: no bits lost at the top |
| `decode_7xnn_extracts_min_register_and_value` | `7000` → `x: 0, nn: 0` |
| `decode_1nnn_keeps_all_12_address_bits` | `1000` → `0x000`, `1FFF` → `0xFFF` |
| `decode_annn_keeps_all_12_address_bits` | `AFFF` → `0xFFF` |
| `decode_dxyn_extracts_max_nibbles` | `DFEF` → `x: F, y: E, n: F` (distinct values, so a swapped X/Y fails) |
| `decode_0nnn_machine_routine_is_unknown` | Pins down today's choice that `0NNN` is rejected. Change this test if you decide to ignore it instead |
| **`decode_accepts_exactly_the_supported_opcodes`** | Tries **all 65,536 opcodes**: only `00E0` and the `1`/`6`/`7`/`A`/`D` families decode; everything else returns `UnknownOpcode` with the same opcode |

The last one is the strongest. A `u16` has only 65,536 values, so checking all of them takes milliseconds and proves `decode` correct for every input, not just the ones I picked. If you add an arm later and forget the error case, or a pattern grabs more than it should, this test names the opcode. When you add new instructions, extend its `supported` line.

I also fixed my old test that read `chip8.variables`. It now checks `chip8.register == [0; REGISTER_SIZE]`.

## Beyond `decode`: issues in your new `execute` code

You're working on it, but these need fixing before `step()` can run a ROM:

1. **Overflow panic in `add_register`.** `self.register[index] += value` **panics in debug builds** when the sum goes past 255, e.g. `V0 = 0xFF`, then `7001`. The spec says `7XNN` wraps and doesn't touch VF. Use `wrapping_add`, and remember it's `#[must_use]`: you have to assign the result back. Add a test for `0xFF + 1 → 0x00`.
2. **`IndexToHigh` handles a case that can't happen.** `x` comes from a nibble (`hi & 0x0F`), so it's always ≤ 15 and always a valid index into `[u8; 16]`. The check adds a branch and an error case no ROM can trigger, and the compiler can already prove the indexing is in range. Remove the variant and the checks; then `set_register` and `add_register` don't need to return a `Result`. (If you keep it, the spelling is "Too", not "To".)
3. **`Ok(self.pc_jump_to(...))` and `Ok(self.i = nnn)`**: this wraps a statement that returns `()` in `Ok`. Run the statement first, then return `Ok(())` (Clippy: `unit_arg`). Once `set_register` and `add_register` no longer return `Result`, `execute` itself might not need to either.
4. **`step()`**: `self.execute(instruction)?; Ok(())` can just be `self.execute(instruction)`. `step` also needs to be `pub` for `main` to call it, and `ExecutionError` needs re-exporting in `lib.rs`.
5. **The non-exhaustive `match` error is useful.** It's the compiler listing what's left to implement. To unblock now, add an arm for `ClearScreen | Draw { .. }` that returns an error, or a `todo!()` you remove once the display exists.
6. **`REGISTER_SIZE`** suggests bytes. Use `REGISTER_COUNT`, or `V_REGISTERS`, since it counts registers.

Once `execute` compiles, I can write the tests for `execute`/`step`.
