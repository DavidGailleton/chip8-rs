# Timers decrementations

*Claude Code session, 2026-11-13. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

`/clear`

### 🧑 David

How to implement the timers decrementations ?

> 🔧 **Bash** — Inspect source layout and timer-related code
> 🔧 **Bash** — Read frontend window setup and step function

### 🤖 Claude

Your main loop already has the clock you need. The window is capped at 60 FPS by `set_target_fps(FPS)` in `src/frontend.rs`, and the CHIP-8 timers count down at 60 Hz. So each pass through the `while` loop in `src/main.rs` is one timer tick.

## Before that: the CPU runs too slowly

Right now each frame runs `step()` once, so the CPU does 60 instructions per second. It should do about 700. Timers and CPU speed are linked, so both get fixed in the same place.

## Recommended structure: one frame = many instructions + one tick

```
each frame (60 Hz, paced by minifb):
    read input
    repeat INSTRUCTIONS_PER_FRAME times:  step()
    tick_timers()        // once per frame
    draw
```

`INSTRUCTIONS_PER_FRAME` is about 700 / 60, so 11 or 12. Make it a `const` so the compiler can unroll or simplify the inner loop. Making it a setting is useful later, because some ROMs want different speeds.

### Design points

1. **Keep `step()` to one instruction, and add a separate public `tick_timers()` on `Chip8`.** Don't decrement inside `step()`: then the timers would follow the instruction rate, not 60 Hz.
2. **Keep the clock out of `Chip8`.** Don't put `std::time::Instant` in the struct. If the core never reads the clock, its behaviour depends only on its inputs. Your tests can then call `tick_timers()` three times and check that the delay timer dropped by three, with no sleeping and no flaky timing. The frontend owns time, and the core owns state.
3. **How to decrement.** Each timer goes down by one per tick and stops at 0. You have three choices for a `u8`:
   - `timer -= 1`: wrong. When the timer is 0, it panics in debug builds and wraps to 255 in release builds.
   - `if timer > 0 { timer -= 1 }`: correct, but it adds a branch.
   - `u8::saturating_sub(1)`: correct and states the intent. On x86 it usually compiles to code with no branch (`sub` plus a conditional move or `adc`), so there's no mispredicted jump. It's the most idiomatic choice. With only two timers ticking 60 times a second, the speed difference is tiny, but the habit pays off in hot loops.
4. **Sound.** The buzzer is on while `sound_timer > 0`. Add a small getter like `is_sound_on()` (or similar) for the frontend to read. Leave audio itself out of the core.

## Alternatives, and why I'd skip them for now

- **Measuring real time (`Instant` with an accumulator).** Each loop, add the elapsed time to a total. While the total is at least 16.67 ms, tick and subtract 16.67 ms. This is more accurate if the frame rate jitters, and subtracting (instead of resetting to 0) avoids drift. The cost is that the frontend gets more complex and runs become less repeatable. With the frame-based design, a slow frame slows the CPU and the timers by the same amount, so the game stays internally consistent. Use this only if you later stop capping the frame rate, for example to support high-refresh-rate or uncapped rendering.
- **A separate timer thread** (for example sharing `Arc<AtomicU8>` between threads). Avoid it. It costs synchronization on every `FX07` read, makes the timing non-repeatable, and is harder to test. Real emulators don't do this.

## Order of operations

Put `tick_timers()` after the batch of instructions in each frame. Then a ROM that sets the delay timer and immediately reads it with `FX07` gets back the value it wrote, not one less, which matches real hardware.

Write `tick_timers()` and the loop change, and I'll review them. A good first test: set the delay timer to 2, tick three times, and check that it's 0 and didn't wrap to 255.

### 🧑 David

I instead will limit the main loop to 700 per second then start the window draw 60 time per second. Is it possible to get system time then start fnctions with modulo ?

### 🤖 Claude

Yes, that works. Two changes from your idea: use `Instant` instead of system time, and use deadlines instead of modulo.

## Use `Instant`, not `SystemTime`

`SystemTime` is the wall clock. NTP or a manual clock change can move it backwards or make it jump, and then your emulator would freeze or speed up. `std::time::Instant` is a clock that only moves forward, which is what you want for measuring intervals. Reading it is cheap: on Linux it doesn't make a real system call, it costs about 20 ns.

## Why modulo on time doesn't work

Something like `if elapsed_ms % 16 == 0 { draw }` breaks in two ways:

- **It can fire several times.** The loop runs about 700 times per second, so a single millisecond can see 0 or 1 iterations, and sometimes more. You can get two draws in the same millisecond.
- **It can miss.** If an iteration takes slightly longer and you jump from 15 ms to 17 ms, the tick is skipped.

Modulo only works on something that counts up by exactly 1 each time, like an instruction counter. Time doesn't do that.

## Option A: deadlines, if you want to read the clock

Keep a `next_draw: Instant`. Each iteration:

```rust
if now >= next_draw {
    // draw + tick timers
    next_draw += FRAME;        // add the period; don't use `next_draw = now + FRAME`
}
```

`+= FRAME` matters. If you use `now + FRAME`, the small lateness of every frame adds up and you slowly drift below 60 Hz. With `+=`, a late frame is made up on the next one.

Pace the CPU the same way, with `next_instr += 1/700 s`.

## Option B: no clock for the timers (recommended)

What you want is 60 timer ticks for every 700 instructions. You can count that exactly with integers:

```
acc += 60            // after each instruction
if acc >= 700 { acc -= 700; tick timers + draw }
```

This is the same idea as Bresenham's line algorithm. It gives exactly 60 ticks per 700 instructions, uses no floats, and never drifts. It also stays repeatable, so tests can check timer behaviour without real time. The only place that needs the clock is the 700/s pacing.

## Two problems you'll hit

1. **`set_target_fps(60)` in the frontend.** minifb sleeps inside `window.update()` to hold 60 FPS. If you call the draw at any rate other than 60 Hz, that sleep slows down your whole loop. Either remove `set_target_fps(60)` and pace the loop yourself, or keep it and make draw the only thing that waits.
2. **Sleeping 1.43 ms before each instruction is too imprecise.** `thread::sleep` often wakes 50 µs to 1 ms late, so you'd end up well under 700/s. Sleep until a deadline (`next_instr - now`) instead of a fixed duration, so late wake-ups are made up. Better still, run a batch of about 11–12 instructions and then sleep once per batch. Fewer sleeps means less overshoot.

That second point is why most emulators end up with the "one frame = a batch of instructions" loop. You can still get there with deadlines, and it's a good exercise. Write it and I'll review it.

### 🧑 David

Explain me more how to implement it. Use the most optimized way

> 🔧 **Bash** — Read rest of frontend and minifb version

### 🤖 Claude

You can still run 700 instructions per second and draw 60 times per second. The fastest way is to pace the loop once per frame and run the CPU in batches, instead of pacing every single instruction.

## What costs time in this loop

From most to least expensive per call:

| Operation | Cost | How often it should run |
|---|---|---|
| `thread::sleep` / minifb's frame cap | System call, giving up the CPU, scheduler wake-up. Usually 50 µs to 1 ms late | 60/s |
| `update_with_buffer` | Converts 2,048 pixels, copies them to the window and scales ×16 (about 524k screen pixels) | at most 60/s, only when the screen changed |
| `get_keys()` | Allocates a `Vec` on every call | 60/s |
| `Instant::now()` | About 20 ns, no real system call | 60/s |
| `step()` | A few ns: fetch, `match`, execute | 700/s |

Pacing each instruction would mean 700 sleeps per second. Each sleep can overshoot by up to 1 ms, and each instruction's time slot is only 1.43 ms. So you'd lose a large share of your speed and the timing would jitter. Batching gives one sleep, one clock read and one input read per frame. Only the cheap part runs 700 times.

## The loop, step by step

Run these steps once per frame:

1. **Read input** once per frame. The window only collects input events during `update`, which happens once per frame, so reading more often wouldn't give fresher keys anyway.
2. **Run this frame's batch of instructions.** 700 / 60 = 11.67, so some frames run 11 instructions and some run 12. Use the integer counter from before: `acc += 700`, then while `acc >= 60` run `step()` and do `acc -= 60`. That gives exactly 700 instructions per second with no floats and no drift. The leftover in `acc` carries into the next frame. Make the numbers `const`s so the compiler can fold them.
3. **Tick the timers once**, with `saturating_sub(1)` on each.
4. **Draw, but only if the screen changed.** Most frames don't change the screen: games often redraw only a few times per second, and a key wait doesn't draw at all. Give `Chip8` a `bool` flag that `00E0` and `DXYN` set to true. The frontend converts and redraws only when the flag is set, then clears it. If nothing changed, you still need to call `window.update()` (no buffer). minifb only processes events and keyboard input inside that call.
5. **Sleep until the next frame time.** Keep a `next_frame: Instant`, add `FRAME` (16.667 ms) to it each frame, and sleep until that time if it's still in the future.
   - Use `next_frame += FRAME` rather than `now + FRAME` so the late wake-ups don't add up into drift.
   - **If you fall far behind** (window dragged, laptop suspended, debugger pause), set `next_frame = now`. Otherwise the loop runs hundreds of frames back to back with no sleep to catch up, and the game fast-forwards.

## Who handles the sleep

There are two ways, and you should pick only one. Two pacers together just add their delays.

- **Keep minifb's `set_target_fps(60)` and remove step 5.** It's the simplest: the frame-time logic is already inside `update`. But its accuracy depends on minifb, and the speed of your emulator then depends on your draw code.
- **Disable minifb's limiter and use your own frame times.** You get exact 60 Hz and you control what happens when you fall behind. This is the better design, and a good exercise with `Instant` and `Duration`.

## Smaller details

- **`get_key_pressed` returns only the first key held down.** Some games need two keys at once. Later, store the 16 keys as a `u16`, one bit per key, built once per frame from the window's key state. Then `EX9E`/`EXA1` become a single bit test. That's 2 bytes instead of an array, with no branching on `Option`.
- **`?` inside the batch** costs almost nothing: the error branch is almost never taken, so the CPU predicts it correctly.
- **`FX0A` (wait for key)** moves the PC back onto itself, so while waiting it re-runs within each batch. That's 12 cheap instructions per frame, so leave it.
- **Bonus (COSMAC VIP display wait):** on the original machine, `DXYN` waited for the next screen refresh. With batches this is easy: stop the batch right after a draw instruction. Some test ROMs check this.

## Where each part lives

- **`Chip8` (the core):** `step()`, `tick_timers()`, the "screen changed" flag, the key state. No `Instant` here, so tests stay repeatable.
- **`main.rs` / frontend:** frame times, sleeping, the instruction counter, deciding whether to draw.

Write it and I'll review it. Check it two ways: count the instructions over 10 seconds (it should be 7,000), and run the delay-timer test ROMs from your test suite.

### 🧑 David

How to compare time to wait for the next step ?

### 🤖 Claude

Use `std::time::Instant` for points in time and `std::time::Duration` for lengths of time. The standard library lets you add and compare them directly.

## The building blocks

| What you want | API | Notes |
|---|---|---|
| Current time | `Instant::now()` | Never goes backwards |
| A future time | `instant + duration` | Gives an `Instant` |
| Has that time passed? | `now >= next_frame` | `Instant` can be compared with `<`, `>=` and so on |
| How long until then | `next_frame.checked_duration_since(now)` | `Some(d)` if it's still ahead, `None` if it has already passed |
| Same, but 0 if passed | `next_frame.saturating_duration_since(now)` | Never panics |
| Wait | `std::thread::sleep(d)` | Waits at least `d`, often a bit more |

Avoid `next_frame - now` (or `duration_since`) when `now` might be later than `next_frame`. Today it returns 0, but the docs say it may panic in a future Rust version. `checked_duration_since` and `saturating_duration_since` state what you mean and stay safe.

## The frame period as a constant

`Duration` arithmetic works in `const` context, so the period costs nothing at runtime:

```rust
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 60); // 16_666_666 ns
```

The integer division drops 0.67 ns per frame. That adds up to about 0.14 ms per hour, so you can ignore it.

## Waiting at the end of a frame

Read the clock once, then decide which case you're in:

```rust
let now = Instant::now();
match next_frame.checked_duration_since(now) {
    Some(wait) => thread::sleep(wait),   // early: sleep the rest of the frame
    None => { /* late: don't sleep */ }
}
next_frame += FRAME;
```

That shows the idea; you still have to choose where it goes in your loop. Three points:

1. **`+= FRAME`, not `= now + FRAME`.** The target times stay fixed to the original schedule. If `sleep` wakes 0.8 ms late, the next wait is 0.8 ms shorter. With `now + FRAME`, every late wake-up would push the whole schedule back, and the emulator would run below 60 Hz.
2. **Setup:** set `next_frame = Instant::now() + FRAME` once before the loop.
3. **If you fall far behind, reset.** In the `None` case, check how late you are with `now.duration_since(next_frame)`. That's safe here because `now` is later. If it's more than a few frames (say `FRAME * 4`), set `next_frame = now` so the loop doesn't race through hundreds of frames to catch up after a pause. If it's only slightly late, do nothing and the next frame makes up the difference.

## Why sleep and not a busy-wait

A loop like `while Instant::now() < next_frame {}` (a busy-wait) is accurate to within microseconds, but it keeps one CPU core at 100% for nothing. `sleep` hands the core back to the OS and may wake up to about 1 ms late. Because you add `FRAME` to fixed target times, that lateness only makes one frame start a little late. The average stays exactly 60 Hz, and the batch of instructions in that frame still runs at full speed. For an emulator, `sleep` alone is the right choice. Mixing them (sleep, then busy-wait the last ~1 ms) only matters for things like audio sync or fixed-step games where every frame must be precise.

## Remember

If you do your own timing like this, turn off minifb's `set_target_fps(60)` in `src/frontend.rs`. Otherwise both delays run and your frames get longer.

### 🧑 David

How to get duration from 1 / 700  ?

### 🤖 Claude

Divide one second, written in nanoseconds, by 700:

```rust
const INSTR_PERIOD: Duration = Duration::from_nanos(1_000_000_000 / 700); // 1_428_571 ns
```

## Why `from_nanos`

| Way | Works in `const`? | Cost |
|---|---|---|
| `Duration::from_nanos(1_000_000_000 / 700)` | Yes | None. The division happens at compile time, so the value is built into the program. |
| `Duration::from_secs(1) / 700` | No, the `/` operator on `Duration` isn't `const` | An integer division at runtime |
| `Duration::from_secs_f64(1.0 / 700.0)` | Don't count on it | Floating-point conversion, plus rounding to the nearest nanosecond, so the same error as above with more work |

Use integer nanoseconds: the value is exact, computed at compile time, and needs no floats. For a type with `u64` seconds and `u32` nanoseconds like `Duration`, `from_nanos` is the natural constructor.

## Rounding

1,000,000,000 / 700 = 1,428,571.43, so you lose 0.43 ns per instruction. That's about 0.3 µs per second, so you can ignore it.

If you ever want no error at all, multiply before you divide. Keep a counter `n` of instructions since `start`. The n-th instruction is due at `start + Duration::from_nanos(n * 1_000_000_000 / 700)`, and since you never add up rounded steps, the error never builds up. Use `u64` for `n`. It won't overflow: at 700 instructions per second, `n * 1e9` stays below `u64::MAX` for about 800 years.

## Reminder

If you sleep before every instruction, each sleep can wake up to about 1 ms late, which is 70% of a 1.43 ms slot. You'll get fewer than 700 instructions per second and the timing will jitter. Adding the period to a fixed schedule, as in the last answer, keeps the average correct. Still, running a batch of instructions per 60 Hz frame and sleeping once per frame is cheaper and smoother.

### 🧑 David

@src/main.rs#L30-48 Help me to finnish the timing loop

> 🔧 **Bash** — Read full main.rs and find timer/display flag code

### 🤖 Claude

Your loop has the right parts, but four bugs keep it from running at the right speed. I'll go through them, then show the structure to aim for. Per the project rules, I haven't edited `src/` and I'm giving the layout rather than ready-to-paste code.

## What's wrong now

**1. The sleep is too long, so the loop runs slower than 700/s** (`src/main.rs:31-32`, `:47`)
- `sleep(INSTR_DURATION)` sleeps a full 1.43 ms, not the time remaining until the next step.
- `last_step = Instant::now()` is set *after* `step()`. So each loop takes the sleep, plus `step()`, plus the drawing, plus however late the sleep woke up. Every loop runs long, and the error adds up.

The fix is the target-time approach: keep `next_step: Instant`, sleep for `next_step.checked_duration_since(now)` if it's `Some`, then do `next_step += INSTR_DURATION`. Never reset the target to `now` (except to recover after falling far behind).

**2. You draw 12 times per second, not 60** (`:35`, `:43-46`)
`acc` goes 0, 1, … 699, and `acc % 60 == 0` is true at 0, 60, …, 660. That's 12 times per 700 instructions, so 12 Hz. It mixes two separate ideas:
- **A modulo counter** only works if the period divides evenly. 700 / 60 isn't a whole number.
- **The accumulator:** `acc += 60` after each instruction, and when `acc >= 700`, do `acc -= 700` and run a frame. That gives exactly 60 frames per 700 instructions, spread as evenly as possible (one every 11 or 12 instructions).

**3. The timers never tick**
Nothing decrements `delay_timer` or `sound_timer`. Add `tick_timers()` to `Chip8` (with `saturating_sub(1)` on both) and call it inside the 60 Hz branch.

**4. The window freezes when the screen doesn't change** (`:37-41`)
minifb only processes events (key presses, window close) inside `update` or `update_with_buffer`. If `display_changed` stays false, for example while `FX0A` waits for a key, nothing calls them. Then:
- `get_keys()` keeps returning old keys, so the key wait never ends;
- `is_open()` never turns false, so the close button stops working;
- the desktop may mark the window as "not responding".

Add a frontend method that calls `self.window.update()` without a buffer, and call it in the `else` branch.

Smaller points:
- Read the keys *after* updating the window in a frame, so they come from the newest events. Either order works at 60 Hz, but this is the cleaner flow.
- `let mut acc = 0;` defaults to `i32`. Writing `u32` says what you mean (it's never negative).

## Structure to aim for

Two versions, both correct:

**A. Your design: pace each instruction, with the frame work inside**
```
setup: next_step = now + INSTR_DURATION, acc = 0

loop while open:
    wait until next_step   (sleep only if it's still ahead)
    next_step += INSTR_DURATION
    step()
    acc += 60
    if acc >= 700:              // runs 60 times per second
        acc -= 700
        tick timers
        draw if display changed, otherwise update the window
        read keys
```

**B. Optimized: pace each frame and run a batch inside**
```
setup: next_frame = now + FRAME, acc = 0

loop while open:
    read keys
    acc += 700
    while acc >= 60: step(); acc -= 60      // 11 or 12 steps
    tick timers
    draw if display changed, otherwise update the window
    wait until next_frame;  next_frame += FRAME
```

B sleeps 60 times per second instead of 700. Each sleep can wake up to about 1 ms late, which is 70% of a 1.43 ms instruction slot but only 6% of a 16.7 ms frame. B also reads the clock 60 times per second instead of 700, and never checks the 60 Hz condition after every instruction. Over a second, both run the same 700 instructions and 60 ticks. B's timing is smoother and uses less CPU. I'd go with B.

In either version, if you fall far behind (target time more than a few periods in the past), set the target to `now` so the loop doesn't race to catch up.

Show me the next version and I'll review it.

### 🧑 David

implement the optimised way

> 🔧 **Bash** — Read Chip8 struct, step, frontend, lib
> 🔧 **Bash** — Check minifb default frame rate limiter
> 🔧 **Bash** — Review uncommitted changes and test helpers

### 🤖 Claude

Now the edits: `tick_timers` in the core, a no-draw `update` in the frontend, and the frame loop.

> 🔧 **Bash** — View test helper and file end
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/chip8.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/frontend.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/frontend.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/frontend.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/main.rs
> 🔧 **Edit** — ~/Documents/chip8-rs/src/main.rs

### 🤖 Claude

File changed on disk; re-reading.

> 🔧 **Read** — ~/Documents/chip8-rs/src/main.rs

### 🤖 Claude

You had moved to version A in the meantime, so I'm replacing that loop with the batched version, and exporting `FPS` from `lib.rs`.

> 🔧 **Bash** — Replace loop with batched version, build, test, lint
> 🔧 **Bash** — Make FPS u32, format, re-lint new code, test
> 🔧 **Bash** — Fix doc backtick lint, check main.rs is clean

### 🤖 Claude

The timing loop now works the optimized way: each 60 Hz frame runs a batch of instructions, ticks the timers once, then sleeps once. The build passes, all tests pass (142 unit, 3 doc), and clippy pedantic shows no warnings in `src/main.rs`. I haven't run it on a ROM yet.

You had rewritten `main.rs` to the version that sleeps before every instruction while I was editing, so I replaced that loop.

**What changed**

- **`src/chip8.rs`**
  - New `tick_timers()`: counts both timers down by one with `saturating_sub`, so they stop at 0 instead of wrapping.
  - A doc comment on `display_changed`.
  - A test `tick_timers_counts_down_and_stops_at_zero`.
- **`src/frontend.rs`**
  - `set_target_fps(0)` turns off minifb's built-in frame limit (250 FPS by default), so only one thing paces the loop.
  - New `update()`: lets the window handle keys and the close button on frames with no redraw.
  - `FPS` is now a `u32`, which avoids a `usize` → `u32` cast.
- **`src/lib.rs`**: also exports `FPS`.
- **`src/main.rs`**: the new loop. Each frame:
  1. Read the keys once.
  2. Run the CPU batch. `acc += 700`, then while `acc >= 60`: `step()` and `acc -= 60`. That gives 11 or 12 steps per frame and exactly 700 per second, using only integers.
  3. Call `tick_timers()` once.
  4. Redraw if `display_changed` is set. Otherwise call `window.update()` so the window doesn't freeze.
  5. Sleep until `next_frame` if it's still ahead, then do `next_frame += FRAME`. If the loop is more than 100 ms late (`MAX_LAG`), it resets the schedule instead of rushing to catch up.

**Optimizations**

- **Fewer sleeps:** one per frame instead of one per instruction. A sleep can wake about 1 ms late, which is 6% of a frame but 70% of an instruction slot.
- **Fewer clock reads:** one `Instant::now()` per frame.
- **No floating point:** `FRAME` is computed at compile time.
- **Cheap frames:** the 2,048-pixel conversion runs only when the screen changed.

Nothing is committed. To check it, run the delay-timer test ROM in `test_suite/`. A rough second check is to time 10 seconds of a ROM and confirm about 7,000 instructions ran.

### 🧑 David

What is the delay timer ?

### 🤖 Claude

The delay timer is an 8-bit register (`u8`) that counts down by one, 60 times per second, until it reaches 0 and stays there. A program can do only two things with it:

| Opcode | Effect |
|---|---|
| `FX15` | Set the delay timer to the value of `VX` |
| `FX07` | Copy the current delay timer value into `VX` |

It has no interrupt or callback. When it reaches 0 nothing happens, and the program has to check it.

## Why it exists

The CPU speed isn't fixed. Different interpreters (the COSMAC VIP, the HP-48 calculators, emulators) run very different numbers of instructions per second. If a game waited by counting instructions, its speed would change on every machine. The delay timer is the one clock that always runs at the same rate: 60 Hz, tied to the screen refresh.

The value is in 1/60ths of a second, so the longest wait is 255 / 60 ≈ 4.25 s.

## How games use it

Usually to wait a fixed amount of time:

```
6X3C    V0 = 60           ; 60 ticks = 1 second
F015    delay = V0
loop:
F007    V0 = delay
3000    skip next if V0 == 0
1xxx    jump loop          ; not 0 yet: keep checking
...                        ; 1 second has passed
```

Typical uses:
- **Keeping game speed steady:** wait a tick or a few between frames, so the game runs at the same speed whatever the CPU speed is.
- **Pauses:** a short wait after losing a life, or a hold on a title screen.
- **Rate-limiting input:** so holding a key doesn't move the piece 700 times per second.

## On the original hardware

On the COSMAC VIP, the chip drawing the screen interrupted the CPU at each 60 Hz video frame. That interrupt routine decremented both timers. That's why the timers are tied to the display frame rate and are separate from instruction execution. In your emulator, `tick_timers()` called once per frame in the main loop plays the role of that interrupt.

## The sound timer

The sound timer works the same way: 8 bits, counts down at 60 Hz, set by `FX18`. The differences are that a program can't read it back, and the buzzer sounds while it's above 0. So `FX18` with 30 gives a half-second beep.

## Why the order inside a frame matters

A game often runs `FX15` and then `FX07` right after. Your loop ticks after the batch of instructions, so the game reads back the value it wrote. If you ticked partway through a batch, the read could come back one lower. Most games don't mind, but some timer test ROMs check for it.

### 🧑 David

run the delay timer test ROM

> 🔧 **Bash** — List test suite contents
> 🔧 **Bash** — Check display, screenshot/input tools, build release
> 🔧 **Bash** — Launch quirks ROM and locate its window
> 🔧 **Bash** — Screenshot menu, press key 1 for CHIP-8, screenshot result
> 🔧 **Bash** — Find Lua syntax for sendshortcut dispatcher
