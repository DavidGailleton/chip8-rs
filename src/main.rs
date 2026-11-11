use std::{error::Error, process::ExitCode};

use chip8_rs::Chip8;

/// Reads the ROM path from the command line, loads it, and runs the emulator.
fn run() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: chip8-rs <rom_path>")?;

    let rom = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let mut chip8 = Chip8::new();

    chip8.load_rom(&rom)?;

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
