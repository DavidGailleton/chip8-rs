use std::{
    error::Error,
    process::ExitCode,
    thread::sleep,
    time::{Duration, Instant},
};

use chip8_rs::{Chip8, FPS, MfbWindow};

/// CPU speed, in instructions per second.
const CPU_HZ: u32 = 700;
/// Timer and display rate, in ticks per second.
const FRAME_HZ: u32 = FPS;
/// Length of one frame (`16_666_666` ns at 60 Hz). Computed at compile time.
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / FRAME_HZ as u64);
/// Being later than this means the loop was stalled (window drag, suspend,
/// debugger): restart the schedule instead of running frames back to back.
const MAX_LAG: Duration = Duration::from_millis(100);

/// Reads the ROM path from the command line, loads it, and runs the emulator.
fn run() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: chip8-rs <rom_path>")?;

    let rom = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let mut window = MfbWindow::new()?;

    let mut chip8 = Chip8::new();

    chip8.load_rom(&rom)?;

    // Spreads CPU_HZ instructions over FRAME_HZ frames with integers only:
    // 700 / 60 = 11.67, so frames run 11 or 12 steps, exactly 700 per second.
    let mut acc: u32 = 0;

    let mut next_frame = Instant::now() + FRAME;

    while window.window.is_open() {
        // Input was polled by the previous frame's window update.
        chip8.key_pressed = window.get_key_pressed();

        // CPU batch.
        acc += CPU_HZ;
        while acc >= FRAME_HZ {
            chip8.step()?;
            acc -= FRAME_HZ;
        }

        chip8.tick_timers();

        // Redraw only if something changed; otherwise still pump window events.
        if chip8.display_changed {
            window.update_buffer(&chip8.display);
            window.draw()?;
            chip8.display_changed = false;
        } else {
            window.update();
        }

        // Pace to 60 Hz: sleep until the next frame time, which stays on a
        // fixed schedule (`+= FRAME`) so late wake-ups don't add up.
        let now = Instant::now();
        match next_frame.checked_duration_since(now) {
            Some(wait) => sleep(wait),
            None if now.duration_since(next_frame) > MAX_LAG => next_frame = now,
            None => {}
        }
        next_frame += FRAME;
    }

    Ok(())
}

/// Entry point: runs [`run`] and turns an error into a message on stderr and exit code 1.
fn main() -> ExitCode {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
