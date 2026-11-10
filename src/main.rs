use std::{error::Error, fmt, process::ExitCode};

const MEMORY_SIZE: usize = 4096;
const PROGRAM_START: usize = 0x200;
const MAX_ROM_SIZE: usize = MEMORY_SIZE - PROGRAM_START;

#[derive(Debug)]
enum LoadError {
    RomTooLarge { size: usize, max: usize },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RomTooLarge { size, max } => {
                write!(f, "rom too large: {} bytes, max is {}", size, max)
            }
        }
    }
}

impl Error for LoadError {}

struct Chip8 {
    memory: [u8; MEMORY_SIZE],
    program_counter: usize,
    i: u16,
    delay_timer: u8,
    sound_timer: u8,
    variables: [u8; 16],
}

impl Chip8 {
    fn new() -> Self {
        Self {
            memory: [0; MEMORY_SIZE],
            program_counter: PROGRAM_START,
            i: 0x0,
            delay_timer: 0x0,
            sound_timer: 0x0,
            variables: [0; 16],
        }
    }

    fn load_rom(&mut self, rom: &[u8]) -> Result<(), LoadError> {
        if rom.len() > MAX_ROM_SIZE {
            return Err(LoadError::RomTooLarge {
                size: rom.len(),
                max: MAX_ROM_SIZE,
            });
        }

        self.memory[PROGRAM_START..PROGRAM_START + rom.len()].copy_from_slice(rom);
        Ok(())
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: chip8-rs <rom_path>")?;

    let buffer =
        std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let mut chip8 = Chip8::new();

    chip8.load_rom(&buffer)?;

    println!(
        "{:02X?}",
        &chip8.memory[PROGRAM_START..PROGRAM_START + 0x10]
    );

    Ok(())
}

fn main() -> ExitCode {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
