//! The CHIP-8 machine: memory, registers, timers, and ROM loading.
//!
//! This module does no I/O. It receives bytes and updates its state, so the
//! front-end (window, keyboard, sound) stays in the binary.

use std::{error::Error, fmt};

use crate::{HEIGHT, WIDTH};

/// Total addressable RAM: 4 KiB, addresses `0x000..=0xFFF`.
const MEMORY_SIZE: usize = 4096;
/// Address where ROMs are loaded and where execution starts.
///
/// The first 512 bytes were reserved for the interpreter on the original
/// COSMAC VIP; today they only hold the font.
const PROGRAM_START: usize = 0x200;
/// Largest ROM that fits between [`PROGRAM_START`] and the end of memory (3584 bytes).
const MAX_ROM_SIZE: usize = MEMORY_SIZE - PROGRAM_START;

const REGISTER_SIZE: usize = 16;

/// Error returned while running a program.
#[derive(Debug)]
pub enum ExecutionError {
    /// The opcode does not match any supported CHIP-8 instruction.
    UnknownOpcode {
        /// The raw 16-bit opcode that was fetched.
        opcode: u16,
    },
    IndexToHigh {
        index: usize,
        max: usize,
    },
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOpcode { opcode } => {
                write!(f, "unknown opcode: {opcode:#06X}")
            }
            Self::IndexToHigh { index, max } => {
                write!(f, "index to high: {index}, max is {max}")
            }
        }
    }
}

impl Error for ExecutionError {}

/// Error returned by [`Chip8::load_rom`].
#[derive(Debug)]
pub enum LoadError {
    /// The ROM does not fit in memory after the program start address (`0x200`).
    RomTooLarge {
        /// Size of the rejected ROM, in bytes.
        size: usize,
        /// Largest accepted size, in bytes.
        max: usize,
    },
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

/// A decoded CHIP-8 instruction, produced by [`Chip8::decode`].
///
/// Opcode notation: `X` and `Y` are register numbers (one nibble each), `N` is
/// a 4-bit value, `NN` an 8-bit value, and `NNN` a 12-bit address.
enum Instruction {
    /// `00E0`: clear the display.
    ClearScreen,
    /// `1NNN`: jump to address `NNN`.
    Jump(u16),
    /// `6XNN`: set register `VX` to `NN`.
    SetRegister {
        /// Register number (`0x0..=0xF`).
        x: u8,
        /// Value to store.
        nn: u8,
    },
    /// `7XNN`: add `NN` to register `VX`, wrapping on overflow; `VF` is not changed.
    AddToRegister {
        /// Register number (`0x0..=0xF`).
        x: u8,
        /// Value to add.
        nn: u8,
    },
    /// `ANNN`: set the index register `I` to `NNN`.
    SetIndex(u16),
    /// `DXYN`: draw an `N`-row sprite from memory at `I`, at the screen
    /// position held in `VX`, `VY`; `VF` is set to 1 if any pixel is erased.
    Draw {
        /// Register holding the X coordinate.
        x: u8,
        /// Register holding the Y coordinate.
        y: u8,
        /// Sprite height in rows (`0..=15`).
        n: u8,
    },
}

/// A CHIP-8 virtual machine.
///
/// Create one with [`Chip8::new`], then load a program with [`Chip8::load_rom`].
pub struct Chip8 {
    /// 4 KiB of RAM. The font lives at `0x050..0x0A0`, programs start at [`PROGRAM_START`].
    memory: [u8; MEMORY_SIZE],
    /// Address of the next instruction to fetch (PC).
    program_counter: usize,
    /// Index register `I`, used to point at memory (sprites, BCD, register dumps).
    i: usize,
    /// General-purpose registers `V0`–`VF`. `VF` doubles as the flag register.
    register: [u8; REGISTER_SIZE],
    pub display: [u64; HEIGHT],
}

impl Chip8 {
    /// Creates a machine with zeroed memory and registers, and the program
    /// counter at the program start address (`0x200`).
    ///
    /// # Examples
    ///
    /// ```
    /// use chip8_rs::Chip8;
    ///
    /// let chip8 = Chip8::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            memory: [0; MEMORY_SIZE],
            program_counter: PROGRAM_START,
            i: 0x0,
            register: [0; REGISTER_SIZE],
            display: [0; HEIGHT],
        }
    }

    /// Copies a ROM into memory starting at the program start address (`0x200`).
    ///
    /// Memory outside the ROM's range is left untouched.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError::RomTooLarge`] if `rom` is longer than 3584 bytes
    /// (`4096 - 0x200`). Memory is not modified in that case.
    ///
    /// # Examples
    ///
    /// ```
    /// use chip8_rs::{Chip8, LoadError};
    ///
    /// let mut chip8 = Chip8::new();
    /// chip8.load_rom(&[0x00, 0xE0])?; // a single CLS instruction
    ///
    /// let too_big = vec![0; 4000];
    /// assert!(matches!(
    ///     chip8.load_rom(&too_big),
    ///     Err(LoadError::RomTooLarge { size: 4000, max: 3584 })
    /// ));
    /// # Ok::<(), LoadError>(())
    /// ```
    pub fn load_rom(&mut self, rom: &[u8]) -> Result<(), LoadError> {
        if rom.len() > MAX_ROM_SIZE {
            return Err(LoadError::RomTooLarge {
                size: rom.len(),
                max: MAX_ROM_SIZE,
            });
        }

        self.memory[PROGRAM_START..PROGRAM_START + rom.len()].copy_from_slice(rom);
        Ok(())
    }

    /// Sets the program counter to `to_jump`, wrapping past the end of memory
    /// like the 12-bit address bus (`0x1000` becomes `0x000`).
    fn pc_jump_to(&mut self, to_jump: usize) {
        self.program_counter = to_jump % MEMORY_SIZE;
    }

    fn set_register(&mut self, index: usize, value: u8) -> Result<(), ExecutionError> {
        if REGISTER_SIZE <= index {
            return Err(ExecutionError::IndexToHigh {
                index,
                max: REGISTER_SIZE,
            });
        }
        self.register[index] = value;
        Ok(())
    }
    fn add_register(&mut self, index: usize, value: u8) -> Result<(), ExecutionError> {
        if REGISTER_SIZE <= index {
            return Err(ExecutionError::IndexToHigh {
                index,
                max: REGISTER_SIZE,
            });
        }
        self.register[index] += value;
        Ok(())
    }

    fn draw(&mut self, x: u8, y: u8, n: u8) {
        let col = usize::from(self.register[usize::from(x)]) % WIDTH;
        let row = usize::from(self.register[usize::from(y)]) % HEIGHT;

        let mut collision: bool = false;

        let sprite = &self.memory[self.i..self.i + usize::from(n)];

        for (i, item) in sprite.iter().enumerate().take(usize::from(n)) {
            if row + i >= HEIGHT {
                break;
            }

            let mask = (u64::from(*item) << (WIDTH - size_of::<u8>())) >> col;

            if self.display[row + i] & mask != 0 {
                collision = true;
            }

            self.display[row + i] ^= mask;
        }
        self.register[0xF] = u8::from(collision);
    }

    /// Reads the instruction at the program counter and advances the PC by 2.
    ///
    /// CHIP-8 instructions are two bytes, stored big-endian: the bytes
    /// `0x00 0xE0` give opcode `0x00E0`. The PC moves *before* the instruction
    /// is executed, so jumps can overwrite it, skips add another 2, and calls
    /// push the address of the next instruction.
    ///
    /// Addresses wrap at the end of memory: fetching at `0xFFF` reads
    /// `0xFFF` then `0x000`, and leaves the PC at `0x001`.
    #[must_use]
    fn fetch(&mut self) -> u16 {
        let res = u16::from_be_bytes([
            self.memory[self.program_counter % MEMORY_SIZE],
            self.memory[(self.program_counter + 1) % MEMORY_SIZE],
        ]);

        self.pc_jump_to(self.program_counter + 2);

        res
    }

    /// Turns a raw opcode into an [`Instruction`].
    ///
    /// The opcode is split into four nibbles (`kind`, `X`, `Y`, `N`) and matched
    /// against the supported instruction patterns.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError::UnknownOpcode`] if the opcode is not a
    /// supported instruction.
    #[must_use = "decoded opcode should always been executed"]
    fn decode(&self, opcode: u16) -> Result<Instruction, ExecutionError> {
        let [hi, lo] = opcode.to_be_bytes();
        let (kind, x, y, n) = (hi >> 4, hi & 0x0F, lo >> 4, lo & 0x0F);

        match (kind, x, y, n) {
            (0x0, 0x0, 0xE, 0x0) => Ok(Instruction::ClearScreen),
            (0x1, _, _, _) => Ok(Instruction::Jump(opcode & 0x0FFF)),
            (0x6, _, _, _) => Ok(Instruction::SetRegister { x, nn: lo }),
            (0x7, _, _, _) => Ok(Instruction::AddToRegister { x, nn: lo }),
            (0xA, _, _, _) => Ok(Instruction::SetIndex(opcode & 0x0FFF)),
            (0xD, _, _, _) => Ok(Instruction::Draw { x, y, n }),
            _ => Err(ExecutionError::UnknownOpcode { opcode }),
        }
    }

    fn execute(&mut self, instruction: Instruction) -> Result<(), ExecutionError> {
        match instruction {
            Instruction::Jump(nnn) => {
                self.pc_jump_to(nnn.into());
                Ok(())
            }
            Instruction::SetRegister { x, nn } => self.set_register(x.into(), nn),
            Instruction::AddToRegister { x, nn } => self.add_register(x.into(), nn),
            Instruction::SetIndex(nnn) => {
                self.i = usize::from(nnn);
                Ok(())
            }
            Instruction::ClearScreen => {
                self.display = [0; HEIGHT];
                Ok(())
            }
            Instruction::Draw { x, y, n } => {
                self.draw(x, y, n);
                Ok(())
            }
        }
    }

    pub fn step(&mut self) -> Result<(), ExecutionError> {
        let opcode = self.fetch();
        let instruction = self.decode(opcode)?;
        self.execute(instruction)?;
        Ok(())
    }
}

/// Same as [`Chip8::new`].
impl Default for Chip8 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ROM of `len` bytes counting 0, 1, …, 255, 0, 1, … so that a copy at the
    /// wrong offset or in the wrong order is detected (an all-zero ROM would not be).
    fn patterned_rom(len: usize) -> Vec<u8> {
        (0..=u8::MAX).cycle().take(len).collect()
    }

    #[test]
    fn new_starts_pc_at_program_start_with_zeroed_state() {
        let chip8 = Chip8::new();

        assert_eq!(chip8.program_counter, PROGRAM_START);
        assert_eq!(chip8.i, 0);
        assert_eq!(chip8.register, [0; REGISTER_SIZE]);
        assert!(chip8.memory.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn load_rom_copies_bytes_at_program_start() {
        let mut chip8 = Chip8::new();
        let rom = [0x00, 0xE0, 0xA2, 0x2A]; // CLS; LD I, 0x22A

        chip8.load_rom(&rom).unwrap();

        assert_eq!(chip8.memory[PROGRAM_START..PROGRAM_START + rom.len()], rom);
    }

    #[test]
    fn load_rom_leaves_memory_outside_rom_untouched() {
        let mut chip8 = Chip8::new();
        let rom = [0xFF; 16];

        chip8.load_rom(&rom).unwrap();

        assert!(chip8.memory[..PROGRAM_START].iter().all(|&byte| byte == 0));
        assert!(
            chip8.memory[PROGRAM_START + rom.len()..]
                .iter()
                .all(|&byte| byte == 0)
        );
    }

    #[test]
    fn load_rom_accepts_empty_rom() {
        let mut chip8 = Chip8::new();

        assert!(chip8.load_rom(&[]).is_ok());
        assert!(chip8.memory.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn load_rom_accepts_rom_of_exactly_max_size() {
        let mut chip8 = Chip8::new();
        let rom = patterned_rom(MAX_ROM_SIZE);

        chip8.load_rom(&rom).unwrap();

        assert_eq!(chip8.memory[PROGRAM_START..], rom[..]);
        // The last ROM byte lands on the last address, 0xFFF.
        assert_eq!(chip8.memory[MEMORY_SIZE - 1], rom[MAX_ROM_SIZE - 1]);
    }

    #[test]
    fn load_rom_rejects_rom_one_byte_over_max_size() {
        let mut chip8 = Chip8::new();
        let rom = patterned_rom(MAX_ROM_SIZE + 1);

        let result = chip8.load_rom(&rom);

        assert!(matches!(
            result,
            Err(LoadError::RomTooLarge { size, max }) if size == MAX_ROM_SIZE + 1 && max == MAX_ROM_SIZE
        ));
    }

    #[test]
    fn load_rom_rejected_rom_does_not_modify_memory() {
        let mut chip8 = Chip8::new();
        chip8.load_rom(&[0xAB; 4]).unwrap();

        let _ = chip8.load_rom(&patterned_rom(MAX_ROM_SIZE + 1));

        assert_eq!(chip8.memory[PROGRAM_START..PROGRAM_START + 4], [0xAB; 4]);
        assert!(
            chip8.memory[PROGRAM_START + 4..]
                .iter()
                .all(|&byte| byte == 0)
        );
    }

    #[test]
    fn load_error_display_shows_size_and_max() {
        let error = LoadError::RomTooLarge {
            size: 5000,
            max: 3584,
        };

        assert_eq!(error.to_string(), "rom too large: 5000 bytes, max is 3584");
    }

    #[test]
    fn fetch_combines_two_bytes_big_endian() {
        let mut chip8 = Chip8::new();
        chip8.load_rom(&[0x12, 0x34]).unwrap();

        let opcode = chip8.fetch();

        assert_eq!(opcode, 0x1234);
    }

    #[test]
    fn fetch_advances_pc_by_two() {
        let mut chip8 = Chip8::new();
        chip8.load_rom(&[0x00, 0xE0]).unwrap();

        let _ = chip8.fetch();

        assert_eq!(chip8.program_counter, PROGRAM_START + 2);
    }

    #[test]
    fn fetch_twice_returns_consecutive_opcodes() {
        let mut chip8 = Chip8::new();
        chip8.load_rom(&[0x00, 0xE0, 0xA2, 0x2A]).unwrap();

        let first = chip8.fetch();
        let second = chip8.fetch();

        assert_eq!((first, second), (0x00E0, 0xA22A));
        assert_eq!(chip8.program_counter, PROGRAM_START + 4);
    }

    #[test]
    fn fetch_works_at_odd_address() {
        let mut chip8 = Chip8::new();
        chip8.load_rom(&[0xFF, 0x12, 0x34]).unwrap();
        chip8.program_counter = PROGRAM_START + 1;

        let opcode = chip8.fetch();

        assert_eq!(opcode, 0x1234);
        assert_eq!(chip8.program_counter, PROGRAM_START + 3);
    }

    #[test]
    fn fetch_at_0xffe_wraps_pc_to_zero() {
        let mut chip8 = Chip8::new();
        chip8.memory[0xFFE] = 0xAB;
        chip8.memory[0xFFF] = 0xCD;
        chip8.program_counter = 0xFFE;

        let opcode = chip8.fetch();

        assert_eq!(opcode, 0xABCD);
        assert_eq!(chip8.program_counter, 0x000);
    }

    #[test]
    fn fetch_at_0xfff_reads_last_byte_then_first_byte() {
        let mut chip8 = Chip8::new();
        chip8.memory[0xFFF] = 0x12;
        chip8.memory[0x000] = 0x34;
        chip8.program_counter = 0xFFF;

        let opcode = chip8.fetch();

        assert_eq!(opcode, 0x1234);
        assert_eq!(chip8.program_counter, 0x001);
    }

    #[test]
    fn pc_jump_to_in_range_sets_pc() {
        let mut chip8 = Chip8::new();

        chip8.pc_jump_to(0x345);

        assert_eq!(chip8.program_counter, 0x345);
    }

    #[test]
    fn pc_jump_to_end_of_memory_wraps_to_zero() {
        let mut chip8 = Chip8::new();

        chip8.pc_jump_to(MEMORY_SIZE);

        assert_eq!(chip8.program_counter, 0x000);
    }

    #[test]
    fn pc_jump_to_past_end_of_memory_wraps() {
        let mut chip8 = Chip8::new();

        chip8.pc_jump_to(MEMORY_SIZE + 1);

        assert_eq!(chip8.program_counter, 0x001);
    }

    #[test]
    fn decode_00e0_is_clear_screen() {
        let chip8 = Chip8::new();

        assert!(matches!(chip8.decode(0x00E0), Ok(Instruction::ClearScreen)));
    }

    #[test]
    fn decode_1nnn_is_jump_to_nnn() {
        let chip8 = Chip8::new();

        assert!(matches!(chip8.decode(0x1ABC), Ok(Instruction::Jump(0xABC))));
    }

    #[test]
    fn decode_6xnn_is_set_register() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x6A42),
            Ok(Instruction::SetRegister { x: 0xA, nn: 0x42 })
        ));
    }

    #[test]
    fn decode_7xnn_is_add_to_register() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x7B3C),
            Ok(Instruction::AddToRegister { x: 0xB, nn: 0x3C })
        ));
    }

    #[test]
    fn decode_annn_is_set_index() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xA123),
            Ok(Instruction::SetIndex(0x123))
        ));
    }

    #[test]
    fn decode_dxyn_is_draw() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xD123),
            Ok(Instruction::Draw {
                x: 0x1,
                y: 0x2,
                n: 0x3
            })
        ));
    }

    #[test]
    fn decode_unsupported_opcode_returns_unknown_opcode() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x5001),
            Err(ExecutionError::UnknownOpcode { opcode: 0x5001 })
        ));
    }

    #[test]
    fn decode_near_miss_of_00e0_is_unknown() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x00E1),
            Err(ExecutionError::UnknownOpcode { opcode: 0x00E1 })
        ));
    }

    #[test]
    fn execution_error_display_shows_opcode_in_hex() {
        let error = ExecutionError::UnknownOpcode { opcode: 0x5001 };

        assert_eq!(error.to_string(), "unknown opcode: 0x5001");
    }

    #[test]
    fn decode_6xnn_extracts_max_register_and_value() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x6FFF),
            Ok(Instruction::SetRegister { x: 0xF, nn: 0xFF })
        ));
    }

    #[test]
    fn decode_7xnn_extracts_min_register_and_value() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x7000),
            Ok(Instruction::AddToRegister { x: 0x0, nn: 0x00 })
        ));
    }

    #[test]
    fn decode_1nnn_keeps_all_12_address_bits() {
        let chip8 = Chip8::new();

        assert!(matches!(chip8.decode(0x1000), Ok(Instruction::Jump(0x000))));
        assert!(matches!(chip8.decode(0x1FFF), Ok(Instruction::Jump(0xFFF))));
    }

    #[test]
    fn decode_annn_keeps_all_12_address_bits() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xAFFF),
            Ok(Instruction::SetIndex(0xFFF))
        ));
    }

    #[test]
    fn decode_dxyn_extracts_max_nibbles() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xDFEF),
            Ok(Instruction::Draw {
                x: 0xF,
                y: 0xE,
                n: 0xF
            })
        ));
    }

    #[test]
    fn decode_0nnn_machine_routine_is_unknown() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x0123),
            Err(ExecutionError::UnknownOpcode { opcode: 0x0123 })
        ));
    }

    /// Walks all 65 536 opcodes: only `00E0` and the `1`, `6`, `7`, `A`, `D`
    /// families decode, every other opcode is rejected and reported as itself.
    #[test]
    fn decode_accepts_exactly_the_supported_opcodes() {
        let chip8 = Chip8::new();

        for opcode in 0..=u16::MAX {
            let kind = opcode >> 12;
            let supported = opcode == 0x00E0 || matches!(kind, 0x1 | 0x6 | 0x7 | 0xA | 0xD);

            match chip8.decode(opcode) {
                Ok(_) => assert!(supported, "{opcode:#06X} should be unknown"),
                Err(ExecutionError::UnknownOpcode { opcode: reported }) => {
                    assert!(!supported, "{opcode:#06X} should decode");
                    assert_eq!(reported, opcode);
                }
                Err(other) => panic!("{opcode:#06X}: unexpected error {other}"),
            }
        }
    }
}
