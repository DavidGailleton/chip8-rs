//! The CHIP-8 machine: memory, registers, timers, and ROM loading.
//!
//! This module does no I/O. It receives bytes and updates its state, so the
//! front-end (window, keyboard, sound) stays in the binary.

use std::{error::Error, fmt};

use rand::random_range;

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

/// Number of general-purpose registers (`V0`–`VF`).
const REGISTER_SIZE: usize = 16;

const VF: usize = 0xF;

const FONT_START: usize = 0x050;
const FONT: [u8; 80] = [
    0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
    0x20, 0x60, 0x20, 0x20, 0x70, // 1
    0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
    0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
    0x90, 0x90, 0xF0, 0x10, 0x10, // 4
    0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
    0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
    0xF0, 0x10, 0x20, 0x40, 0x40, // 7
    0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
    0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
    0xF0, 0x90, 0xF0, 0x90, 0x90, // A
    0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
    0xF0, 0x80, 0x80, 0x80, 0xF0, // C
    0xE0, 0x90, 0x90, 0x90, 0xE0, // D
    0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
    0xF0, 0x80, 0xF0, 0x80, 0x80, // F
];
const FONT_END: usize = FONT_START + FONT.len();

/// Error returned while running a program.
#[derive(Debug)]
pub enum ExecutionError {
    /// The opcode does not match any supported CHIP-8 instruction.
    UnknownOpcode {
        /// The raw 16-bit opcode that was fetched.
        opcode: u16,
    },
    /// A register number is outside `V0`–`VF`.
    IndexToHigh {
        /// The rejected register number.
        index: usize,
        /// Number of registers; valid numbers are `0..max`.
        max: usize,
    },
    /// `00EE` (return) ran with no subroutine call on the stack.
    EmptyStack,
    /// A address is outside `0x0`–`MEMORY_SIZE`.
    AddressOutOfRange {
        /// The rejected address.
        address: usize,
        /// Number addresses; valid numbers are `0..max`.
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
            Self::EmptyStack => {
                write!(f, "try to pop on an empty stack")
            }
            Self::AddressOutOfRange { address, max } => {
                write!(f, "address to high: {address}, max is {max}")
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
    DoNothing,
    /// `00E0`: clear the display.
    ClearScreen,
    /// `00EE`: return from a subroutine: pop the return address off the stack into the PC.
    ReturnSubroutine,
    /// `1NNN`: jump to address `NNN`.
    Jump(u16),
    /// `2NNN`: call the subroutine at `NNN`: push the return address, then jump.
    CallSubroutine {
        /// Address of the subroutine.
        nnn: u16,
    },
    /// `3XNN` (`VX == NN`) and `5XY0` (`VX == VY`): skip the next instruction
    /// if `a == b`. The operands are read from the registers at decode time.
    SkipConditionAND {
        /// Left operand (value of `VX`).
        a: u8,
        /// Right operand (`NN`, or value of `VY`).
        b: u8,
    },
    /// `4XNN` (`VX != NN`) and `9XY0` (`VX != VY`): skip the next instruction
    /// if `a != b`. The operands are read from the registers at decode time.
    SkipConditionXOR {
        /// Left operand (value of `VX`).
        a: u8,
        /// Right operand (`NN`, or value of `VY`).
        b: u8,
    },
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
    /// `8XYN`: logic and arithmetic between `VX` and `VY`; `N` selects the
    /// operation (see [`Chip8::logic_instruction`]).
    Logical {
        /// Destination register, and first operand.
        x: u8,
        /// Second operand register.
        y: u8,
        /// Operation selector (`0..=7` or `E`).
        n: u8,
    },
    /// `BNNN`: jump to `NNN + V0` (COSMAC VIP behavior; CHIP-48 uses `XNN + VX`,
    /// see the `BNNN` quirk).
    JumpWithOffset(u16),
    /// `CXNN`: set `VX` to a random byte ANDed with `NN`.
    Random {
        /// Destination register.
        x: u8,
        /// Mask applied to the random byte.
        nn: u8,
    },
    /// `EX9E`: skip the next instruction if the key whose value is in `VX` is pressed.
    SkipIfPressed {
        /// Register holding the key value (`0x0..=0xF`).
        x: u8,
    },
    /// `EXA1`: skip the next instruction if the key whose value is in `VX` is
    /// *not* pressed.
    SkipIfNotPressed {
        /// Register holding the key value (`0x0..=0xF`).
        x: u8,
    },
    /// `FX07`: set `VX` to the current value of the delay timer.
    SetRegisterFromDelayTimer {
        /// Destination register.
        x: u8,
    },
    /// `FX15`: set the delay timer to `VX`.
    SetDelayTimer {
        /// Source register.
        x: u8,
    },
    /// `FX18`: set the sound timer to `VX`.
    SetSoundTimer {
        /// Source register.
        x: u8,
    },
    /// `FX1E`: add `VX` to the index register `I`. Follows the Amiga quirk:
    /// `VF` is set to 1 when `I` goes past `0x0FFF`.
    AddToIndex {
        /// Register holding the value to add.
        x: u8,
    },
    /// `FX0A`: wait for a key press and store its value in `VX`. Blocks by
    /// moving the PC back onto the instruction until a key is pressed.
    GetKey {
        /// Destination register.
        x: u8,
    },
    /// `FX29`: point `I` at the font sprite of the hex digit held in `VX`.
    FontCharacter {
        /// Register holding the digit (`0x0..=0xF`).
        x: u8,
    },
    /// `FX33`: store the decimal digits of `VX` in memory: hundreds at `I`,
    /// tens at `I + 1`, ones at `I + 2` (156 → 1, 5, 6).
    DecimalConversion {
        /// Register holding the number to convert.
        x: u8,
    },
    /// `FX55`: store `V0..=VX` in memory starting at `I`. `I` is left
    /// unchanged (modern behavior, see the `FX55`/`FX65` quirk).
    StoreMemory {
        /// Last register to store (inclusive).
        x: u8,
    },
    /// `FX65`: load `V0..=VX` from memory starting at `I`. `I` is left
    /// unchanged (modern behavior, see the `FX55`/`FX65` quirk).
    LoadMemory {
        /// Last register to load (inclusive).
        x: u8,
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
    /// The 64×32 monochrome screen, one `u64` per row. The most significant
    /// bit is the leftmost pixel (x = 0); a set bit is a lit pixel.
    pub display: [u64; HEIGHT],
    /// Return addresses pushed by `2NNN` and popped by `00EE`.
    stack: Vec<u16>,
    /// The CHIP-8 key (`0x0..=0xF`) currently held down, if any. Set by the
    /// front-end before each [`Chip8::step`].
    pub key_pressed: Option<u8>,

    /// Delay timer, read by `FX07` and set by `FX15`. Meant to count down at 60 Hz.
    delay_timer: u8,
    /// Sound timer, set by `FX18`. The buzzer sounds while it is non-zero.
    sound_timer: u8,
    /// Set by `00E0` and `DXYN` when the display changes. The front-end
    /// redraws only when it is set, then clears it.
    pub display_changed: bool,
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
        let mut memory = [0; MEMORY_SIZE];
        memory[FONT_START..FONT_END].copy_from_slice(&FONT);

        Self {
            memory,
            program_counter: PROGRAM_START,
            i: 0x0,
            register: [0; REGISTER_SIZE],
            display: [0; HEIGHT],
            stack: Vec::new(),
            key_pressed: None,
            delay_timer: 0,
            sound_timer: 0,
            display_changed: true,
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

    /// Stores `value` in register `V{index}`.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError::IndexToHigh`] if `index` is not in `0..16`.
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

    /// Adds `value` to register `V{index}`, wrapping on overflow (`7XNN`).
    /// `VF` is not changed.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError::IndexToHigh`] if `index` is not in `0..16`.
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

    /// Returns the value of register `V{index}`.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError::IndexToHigh`] if `index` is not in `0..16`.
    fn get_register(&self, index: usize) -> Result<u8, ExecutionError> {
        if REGISTER_SIZE <= index {
            return Err(ExecutionError::IndexToHigh {
                index,
                max: REGISTER_SIZE,
            });
        }
        Ok(self.register[index])
    }

    /// Draws an `n`-row sprite read from memory at `I`, at the position held
    /// in registers `VX` and `VY` (`DXYN`).
    ///
    /// Each sprite byte is one row, most significant bit on the left. Pixels
    /// are XORed onto the display. The start position wraps around the
    /// screen (x mod 64, y mod 32) but the sprite itself is clipped at the
    /// right and bottom edges. `VF` is set to 1 if any lit pixel is turned
    /// off (a collision), otherwise 0.
    fn draw(&mut self, x: u8, y: u8, n: u8) {
        let col = usize::from(self.register[usize::from(x)]) % WIDTH;
        let row = usize::from(self.register[usize::from(y)]) % HEIGHT;

        let mut collision: bool = false;

        let sprite = &self.memory[self.i..self.i + usize::from(n)];

        for (i, item) in sprite.iter().enumerate().take(usize::from(n)) {
            if row + i >= HEIGHT {
                break;
            }

            let mask = (u64::from(*item) << (u64::BITS - u8::BITS)) >> col;

            if self.display[row + i] & mask != 0 {
                collision = true;
            }

            self.display[row + i] ^= mask;
        }
        self.register[0xF] = u8::from(collision);
        self.display_changed = true;
    }

    /// Stores `a + b` (wrapping) in `V{index}` and returns the carry: 1 if the
    /// true sum does not fit in a byte, else 0. Used by `8XY4`.
    fn overflowing_add(&mut self, index: usize, a: u8, b: u8) -> Result<u8, ExecutionError> {
        self.set_register(index, a + b)?;

        if u16::from(a) + u16::from(b) > 255 {
            Ok(1_u8)
        } else {
            Ok(0_u8)
        }
    }

    /// Stores `a - b` (wrapping) in `V{index}` and returns the "no borrow"
    /// flag: 1 if `a >= b`, else 0. Used by `8XY5` and `8XY7`.
    fn overflowing_sub(&mut self, index: usize, a: u8, b: u8) -> Result<u8, ExecutionError> {
        self.set_register(index, a - b)?;

        if a < b { Ok(0_u8) } else { Ok(1_u8) }
    }

    /// Runs an `8XYN` logic or arithmetic instruction, selected by `n`:
    ///
    /// | `n` | Operation | `VF` |
    /// |-----|-----------|------|
    /// | `0` | `VX = VY` | unchanged |
    /// | `1` | `VX \|= VY` | unchanged |
    /// | `2` | `VX &= VY` | unchanged |
    /// | `3` | `VX ^= VY` | unchanged |
    /// | `4` | `VX += VY` | 1 on overflow (carry), else 0 |
    /// | `5` | `VX = VX - VY` | 1 if `VX >= VY` (no borrow), else 0 |
    /// | `6` | `VX = VY >> 1` | the bit shifted out |
    /// | `7` | `VX = VY - VX` | 1 if `VY >= VX` (no borrow), else 0 |
    /// | `E` | `VX = VY << 1` | the bit shifted out |
    ///
    /// The shifts follow the COSMAC VIP behavior (`VX = VY` before shifting);
    /// see the shift quirk. The flag is written after the result, so it wins
    /// when `X` is `F`.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError::IndexToHigh`] if `x` or `y` is not a register number.
    fn logic_instruction(&mut self, x: u8, y: u8, n: u8) -> Result<(), ExecutionError> {
        match n {
            0x0 => Ok(self.set_register(x.into(), self.get_register(y.into())?)?),
            0x1 => Ok(self.set_register(
                usize::from(x),
                self.get_register(x.into())? | self.get_register(y.into())?,
            )?),
            0x2 => Ok(self.set_register(
                usize::from(x),
                self.get_register(x.into())? & self.get_register(y.into())?,
            )?),
            0x3 => Ok(self.set_register(
                usize::from(x),
                self.get_register(x.into())? ^ self.get_register(y.into())?,
            )?),
            0x4 => {
                let vx = self.get_register(usize::from(x))?;
                let vy = self.get_register(usize::from(y))?;

                let is_overflow = self.overflowing_add(x.into(), vx, vy)?;

                self.set_register(VF, is_overflow)?;

                Ok(())
            }
            0x5 => {
                let vx = self.get_register(usize::from(x))?;
                let vy = self.get_register(usize::from(y))?;

                let is_overflow = self.overflowing_sub(x.into(), vx, vy)?;

                self.set_register(VF, is_overflow)?;

                Ok(())
            }
            0x6 => {
                self.set_register(usize::from(x), self.get_register(y.into())?)?;
                let bit_shifted_out = self.get_register(usize::from(x))? & 0b0000_0001;
                self.set_register(usize::from(x), self.get_register(x.into())? >> 1)?;
                Ok(self.set_register(VF, bit_shifted_out)?)
            }
            0x7 => {
                let vx = self.get_register(usize::from(x))?;
                let vy = self.get_register(usize::from(y))?;

                let is_overflow = self.overflowing_sub(x.into(), vy, vx)?;

                self.set_register(VF, is_overflow)?;

                Ok(())
            }
            0xE => {
                self.set_register(usize::from(x), self.get_register(y.into())?)?;
                let bit_shifted_out = self.get_register(usize::from(x))? & 0b1000_0000;
                self.set_register(usize::from(x), self.get_register(x.into())? << 1)?;
                Ok(self.set_register(VF, bit_shifted_out >> 7)?)
            }
            _ => Err(ExecutionError::UnknownOpcode {
                opcode: (u16::from(0x80 + x) << 8) + (u16::from((y << 4) + n)),
            }),
        }
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
            (0x0, 0x0, 0xE, 0xE) => Ok(Instruction::ReturnSubroutine),
            (0x0, _, _, _) => Ok(Instruction::DoNothing),

            (0x1, _, _, _) => Ok(Instruction::Jump(opcode & 0x0FFF)),
            (0x2, _, _, _) => Ok(Instruction::CallSubroutine {
                nnn: opcode & 0x0FFF,
            }),
            (0x3, _, _, _) => Ok(Instruction::SkipConditionAND {
                a: self.get_register(usize::from(x))?,
                b: lo,
            }),
            (0x4, _, _, _) => Ok(Instruction::SkipConditionXOR {
                a: self.get_register(usize::from(x))?,
                b: lo,
            }),
            (0x5, _, _, 0x0) => Ok(Instruction::SkipConditionAND {
                a: self.get_register(usize::from(x))?,
                b: self.get_register(usize::from(y))?,
            }),
            (0x6, _, _, _) => Ok(Instruction::SetRegister { x, nn: lo }),
            (0x7, _, _, _) => Ok(Instruction::AddToRegister { x, nn: lo }),
            (0x8, _, _, 0x8..=0xD | 0xF) => Err(ExecutionError::UnknownOpcode { opcode }),
            (0x8, _, _, _) => Ok(Instruction::Logical { x, y, n }),
            (0x9, _, _, 0x0) => Ok(Instruction::SkipConditionXOR {
                a: self.get_register(usize::from(x))?,
                b: self.get_register(usize::from(y))?,
            }),
            (0xA, _, _, _) => Ok(Instruction::SetIndex(opcode & 0x0FFF)),
            (0xB, _, _, _) => Ok(Instruction::JumpWithOffset(opcode & 0x0FFF)),
            (0xC, _, _, _) => Ok(Instruction::Random { x, nn: lo }),
            (0xD, _, _, _) => Ok(Instruction::Draw { x, y, n }),
            (0xE, _, 0x9, 0xE) => Ok(Instruction::SkipIfPressed { x }),
            (0xE, _, 0xA, 0x1) => Ok(Instruction::SkipIfNotPressed { x }),
            (0xF, _, 0x0, 0x7) => Ok(Instruction::SetRegisterFromDelayTimer { x }),
            (0xF, _, 0x1, 0x5) => Ok(Instruction::SetDelayTimer { x }),
            (0xF, _, 0x1, 0x8) => Ok(Instruction::SetSoundTimer { x }),
            (0xF, _, 0x1, 0xE) => Ok(Instruction::AddToIndex { x }),
            (0xF, _, 0x0, 0xA) => Ok(Instruction::GetKey { x }),
            (0xF, _, 0x2, 0x9) => Ok(Instruction::FontCharacter { x }),
            (0xF, _, 0x3, 0x3) => Ok(Instruction::DecimalConversion { x }),
            (0xF, _, 0x5, 0x5) => Ok(Instruction::StoreMemory { x }),
            (0xF, _, 0x6, 0x5) => Ok(Instruction::LoadMemory { x }),
            _ => Err(ExecutionError::UnknownOpcode { opcode }),
        }
    }

    fn put_hex_to_pseudo_dec_in_memory(&mut self, byte: u8) {
        self.memory[self.i] = byte / 100;
        self.memory[self.i + 1] = (byte / 10) % 10;
        self.memory[self.i + 2] = byte % 10;
    }

    /// Applies a decoded [`Instruction`] to the machine state.
    ///
    /// The PC has already been advanced past the instruction by
    /// [`Chip8::fetch`], so a skip adds 2 more and a call pushes the current PC
    /// as the return address.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError::EmptyStack`] for `00EE` with nothing to return
    /// to, and [`ExecutionError::IndexToHigh`] for an invalid register number.
    fn execute(&mut self, instruction: Instruction) -> Result<(), ExecutionError> {
        match instruction {
            Instruction::DoNothing => Ok(()),
            Instruction::ClearScreen => {
                self.display = [0; HEIGHT];
                self.display_changed = true;
                Ok(())
            }
            Instruction::ReturnSubroutine => {
                self.program_counter =
                    usize::from(self.stack.pop().ok_or(ExecutionError::EmptyStack)?);
                Ok(())
            }
            Instruction::Jump(nnn) => {
                self.pc_jump_to(nnn.into());
                Ok(())
            }
            Instruction::CallSubroutine { nnn } => {
                self.stack
                    .push(u16::try_from(self.program_counter).expect("PC is always below 0x1000"));
                self.pc_jump_to(nnn.into());
                Ok(())
            }
            Instruction::SkipConditionAND { a, b } => {
                self.program_counter += if a == b { 2 } else { 0 };
                Ok(())
            }
            Instruction::SkipConditionXOR { a, b } => {
                self.program_counter += if a != b { 2 } else { 0 };
                Ok(())
            }
            Instruction::SetRegister { x, nn } => self.set_register(x.into(), nn),
            Instruction::AddToRegister { x, nn } => self.add_register(x.into(), nn),
            Instruction::SetIndex(nnn) => {
                self.i = usize::from(nnn);
                Ok(())
            }
            Instruction::Draw { x, y, n } => {
                self.draw(x, y, n);
                Ok(())
            }
            Instruction::Logical { x, y, n } => {
                self.logic_instruction(x, y, n)?;
                Ok(())
            }
            Instruction::JumpWithOffset(nnn) => {
                self.pc_jump_to(usize::from(nnn) + usize::from(self.get_register(0)?));
                Ok(())
            }
            Instruction::Random { x, nn } => {
                self.set_register(usize::from(x), random_range(0x0..=0xFF) & nn)?;
                Ok(())
            }
            Instruction::SkipIfPressed { x } => {
                if Some(self.get_register(x.into())?) == self.key_pressed {
                    self.program_counter += 2;
                }
                Ok(())
            }
            Instruction::SkipIfNotPressed { x } => {
                if Some(self.get_register(x.into())?) != self.key_pressed {
                    self.program_counter += 2;
                }
                Ok(())
            }
            Instruction::SetRegisterFromDelayTimer { x } => {
                self.set_register(usize::from(x), self.delay_timer)?;
                Ok(())
            }
            Instruction::SetDelayTimer { x } => {
                self.delay_timer = self.get_register(usize::from(x))?;
                Ok(())
            }
            Instruction::SetSoundTimer { x } => {
                self.sound_timer = self.get_register(usize::from(x))?;
                Ok(())
            }
            Instruction::AddToIndex { x } => {
                self.i += usize::from(self.get_register(x.into())?);

                if self.i > 0x0FFF {
                    self.set_register(VF, 0x1)?;
                }

                Ok(())
            }
            Instruction::GetKey { x } => {
                if self.key_pressed.is_none() {
                    self.program_counter -= 2;
                } else {
                    self.set_register(usize::from(x), self.key_pressed.unwrap())?;
                }
                Ok(())
            }
            Instruction::FontCharacter { x } => {
                let vx = usize::from(self.get_register(usize::from(x))?);
                self.i = FONT_START + vx * 5;
                Ok(())
            }
            Instruction::DecimalConversion { x } => {
                let nb = self.get_register(usize::from(x))?;

                self.put_hex_to_pseudo_dec_in_memory(nb);

                Ok(())
            }

            Instruction::StoreMemory { x } => {
                if usize::from(x) + self.i >= MEMORY_SIZE {
                    return Err(ExecutionError::AddressOutOfRange {
                        address: usize::from(x) + self.i,
                        max: MEMORY_SIZE,
                    });
                }

                self.memory[self.i..=self.i + usize::from(x)]
                    .copy_from_slice(&self.register[0..=usize::from(x)]);

                Ok(())
            }

            Instruction::LoadMemory { x } => {
                if usize::from(x) + self.i >= MEMORY_SIZE {
                    return Err(ExecutionError::AddressOutOfRange {
                        address: usize::from(x) + self.i,
                        max: MEMORY_SIZE,
                    });
                }

                self.register[0..=usize::from(x)]
                    .copy_from_slice(&self.memory[self.i..=self.i + usize::from(x)]);

                Ok(())
            }
        }
    }

    /// Runs one fetch–decode–execute cycle: reads the instruction at the PC,
    /// advances the PC by 2, and executes it.
    ///
    /// # Errors
    ///
    /// Returns an `ExecutionError` if the opcode is unknown or the
    /// instruction cannot run (for example, `00EE` with an empty stack).
    ///
    /// # Examples
    ///
    /// ```
    /// use chip8_rs::Chip8;
    ///
    /// let mut chip8 = Chip8::new();
    /// chip8.load_rom(&[0x00, 0xE0]).unwrap(); // CLS
    /// chip8.step().unwrap();
    /// assert!(chip8.display.iter().all(|&row| row == 0));
    /// ```
    pub fn step(&mut self) -> Result<(), ExecutionError> {
        let opcode = self.fetch();
        let instruction = self.decode(opcode)?;
        self.execute(instruction)?;
        Ok(())
    }

    /// Counts both timers down by one, stopping at zero. Call it at 60 Hz,
    /// independently of how many instructions run.
    ///
    /// `saturating_sub` stops at 0 instead of panicking (debug) or wrapping
    /// to 255 (release), and usually compiles without a branch.
    pub fn tick_timers(&mut self) {
        self.delay_timer = self.delay_timer.saturating_sub(1);
        self.sound_timer = self.sound_timer.saturating_sub(1);
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

    /// A fresh machine: PC at 0x200, everything zeroed except the font.
    #[test]
    fn new_starts_pc_at_program_start_with_zeroed_state() {
        let chip8 = Chip8::new();

        assert_eq!(chip8.program_counter, PROGRAM_START);
        assert_eq!(chip8.i, 0);
        assert_eq!(chip8.register, [0; REGISTER_SIZE]);
        assert_eq!(chip8.key_pressed, None);
        assert_eq!((chip8.delay_timer, chip8.sound_timer), (0, 0));
        assert!(chip8.memory[..FONT_START].iter().all(|&byte| byte == 0));
        assert!(chip8.memory[FONT_END..].iter().all(|&byte| byte == 0));
    }

    /// The built-in font sits at 0x050..0x0A0 so `FX29` can point at it.
    #[test]
    fn new_loads_font_at_font_start() {
        let chip8 = Chip8::new();

        assert_eq!(chip8.memory[FONT_START..FONT_END], FONT);
        assert_eq!(FONT_END, 0x0A0);
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

        // Below 0x200 only the font must remain, exactly as `new` left it.
        assert_eq!(
            chip8.memory[..PROGRAM_START],
            Chip8::new().memory[..PROGRAM_START]
        );
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
        assert_eq!(chip8.memory, Chip8::new().memory);
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

    /// Walks all 65 536 opcodes: only the 35 CHIP-8 instructions decode
    /// (`00E0`, `00EE`, `1NNN`–`4XNN`, `5XY0`, `6XNN`, `7XNN`, `8XY0`–`8XY7`,
    /// `8XYE`, `9XY0`, `ANNN`–`DXYN`, `EX9E`, `EXA1`, and the `FXNN` group);
    /// every other opcode is rejected and reported as itself.
    #[test]
    fn decode_accepts_exactly_the_supported_opcodes() {
        let chip8 = Chip8::new();

        for opcode in 0..=u16::MAX {
            let kind = opcode >> 12;
            let n = opcode & 0x000F;
            let lo = opcode & 0x00FF;
            let supported = matches!(opcode, 0x00E0 | 0x00EE)
                || matches!(kind, 0x1..=0x4 | 0x6 | 0x7 | 0xA..=0xD)
                || (matches!(kind, 0x5 | 0x9) && n == 0x0)
                || (kind == 0x8 && matches!(n, 0x0..=0x7 | 0xE))
                || (kind == 0xE && matches!(lo, 0x9E | 0xA1))
                || (kind == 0xF
                    && matches!(
                        lo,
                        0x07 | 0x0A | 0x15 | 0x18 | 0x1E | 0x29 | 0x33 | 0x55 | 0x65
                    ));

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

    // ---------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------

    /// A machine with `rom` loaded at [`PROGRAM_START`].
    fn machine_with(rom: &[u8]) -> Chip8 {
        let mut chip8 = Chip8::new();
        chip8.load_rom(rom).unwrap();
        chip8
    }

    /// Decodes and executes `opcode` directly, without fetching it from memory.
    fn exec(chip8: &mut Chip8, opcode: u16) -> Result<(), ExecutionError> {
        let instruction = chip8.decode(opcode)?;
        chip8.execute(instruction)
    }

    /// Runs `step` `count` times, panicking on the first error.
    fn step_n(chip8: &mut Chip8, count: usize) {
        for _ in 0..count {
            chip8.step().unwrap();
        }
    }

    // ---------------------------------------------------------------------
    // Default and errors
    // ---------------------------------------------------------------------

    #[test]
    fn default_matches_new() {
        let chip8 = Chip8::default();

        assert_eq!(chip8.program_counter, PROGRAM_START);
        assert_eq!(chip8.i, 0);
        assert_eq!(chip8.register, [0; REGISTER_SIZE]);
        assert_eq!(chip8.display, [0; HEIGHT]);
        assert!(chip8.stack.is_empty());
    }

    #[test]
    fn execution_error_display_index_to_high_shows_index_and_max() {
        let message = ExecutionError::IndexToHigh { index: 20, max: 16 }.to_string();

        assert!(message.contains("20"), "{message}");
        assert!(message.contains("16"), "{message}");
    }

    #[test]
    fn execution_error_display_empty_stack() {
        assert_eq!(
            ExecutionError::EmptyStack.to_string(),
            "try to pop on an empty stack"
        );
    }

    // ---------------------------------------------------------------------
    // Registers
    // ---------------------------------------------------------------------

    #[test]
    fn set_register_stores_value() {
        let mut chip8 = Chip8::new();

        chip8.set_register(0x5, 0x42).unwrap();

        assert_eq!(chip8.register[0x5], 0x42);
    }

    #[test]
    fn set_register_accepts_vf() {
        let mut chip8 = Chip8::new();

        chip8.set_register(0xF, 0x01).unwrap();

        assert_eq!(chip8.register[0xF], 0x01);
    }

    #[test]
    fn set_register_rejects_index_16_without_writing() {
        let mut chip8 = Chip8::new();

        let result = chip8.set_register(REGISTER_SIZE, 0x42);

        assert!(matches!(
            result,
            Err(ExecutionError::IndexToHigh { index: 16, max: 16 })
        ));
        assert_eq!(chip8.register, [0; REGISTER_SIZE]);
    }

    #[test]
    fn get_register_returns_value() {
        let mut chip8 = Chip8::new();
        chip8.register[0xA] = 0x99;

        assert_eq!(chip8.get_register(0xA).unwrap(), 0x99);
    }

    #[test]
    fn get_register_rejects_index_16() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.get_register(REGISTER_SIZE),
            Err(ExecutionError::IndexToHigh { index: 16, max: 16 })
        ));
    }

    #[test]
    fn add_register_adds_value() {
        let mut chip8 = Chip8::new();
        chip8.register[0x3] = 10;

        chip8.add_register(0x3, 5).unwrap();

        assert_eq!(chip8.register[0x3], 15);
    }

    #[test]
    fn add_register_wraps_on_overflow_and_leaves_vf_unchanged() {
        let mut chip8 = Chip8::new();
        chip8.register[0x3] = 0xFF;

        chip8.add_register(0x3, 0x02).unwrap();

        assert_eq!(chip8.register[0x3], 0x01);
        assert_eq!(chip8.register[0xF], 0x00);
    }

    #[test]
    fn add_register_rejects_index_16() {
        let mut chip8 = Chip8::new();

        assert!(matches!(
            chip8.add_register(REGISTER_SIZE, 1),
            Err(ExecutionError::IndexToHigh { index: 16, max: 16 })
        ));
    }

    // ---------------------------------------------------------------------
    // draw (DXYN)
    // ---------------------------------------------------------------------

    /// A machine with `sprite` at `I = 0x300`, `V0 = vx` and `V1 = vy`, ready
    /// for `draw(0, 1, n)`.
    fn machine_with_sprite(sprite: &[u8], vx: u8, vy: u8) -> Chip8 {
        let mut chip8 = Chip8::new();
        chip8.i = 0x300;
        chip8.memory[0x300..0x300 + sprite.len()].copy_from_slice(sprite);
        chip8.register[0x0] = vx;
        chip8.register[0x1] = vy;
        chip8
    }

    #[test]
    fn draw_one_row_at_origin() {
        let mut chip8 = machine_with_sprite(&[0b1111_0000], 0, 0);

        chip8.draw(0x0, 0x1, 1);

        assert_eq!(chip8.display[0], 0b1111_0000 << 56);
        assert!(chip8.display[1..].iter().all(|&row| row == 0));
        assert_eq!(chip8.register[0xF], 0);
    }

    #[test]
    fn draw_at_column_offset_shifts_sprite_right() {
        let mut chip8 = machine_with_sprite(&[0b1111_0000], 8, 0);

        chip8.draw(0x0, 0x1, 1);

        assert_eq!(chip8.display[0], 0b1111_0000 << 48);
    }

    #[test]
    fn draw_multiple_rows_at_row_offset() {
        let mut chip8 = machine_with_sprite(&[0b1000_0001, 0b0110_0110], 0, 5);

        chip8.draw(0x0, 0x1, 2);

        assert_eq!(chip8.display[5], 0b1000_0001 << 56);
        assert_eq!(chip8.display[6], 0b0110_0110 << 56);
        assert_eq!(chip8.display[4], 0);
        assert_eq!(chip8.display[7], 0);
    }

    #[test]
    fn draw_twice_erases_and_sets_vf() {
        let mut chip8 = machine_with_sprite(&[0b1111_0000], 0, 0);

        chip8.draw(0x0, 0x1, 1);
        chip8.draw(0x0, 0x1, 1);

        assert_eq!(chip8.display[0], 0);
        assert_eq!(chip8.register[0xF], 1);
    }

    #[test]
    fn draw_partial_overlap_xors_and_sets_vf() {
        let mut chip8 = machine_with_sprite(&[0b0110_0000], 0, 0);
        chip8.display[0] = 0b1100_0000 << 56;

        chip8.draw(0x0, 0x1, 1);

        assert_eq!(chip8.display[0], 0b1010_0000 << 56);
        assert_eq!(chip8.register[0xF], 1);
    }

    #[test]
    fn draw_without_collision_clears_vf() {
        let mut chip8 = machine_with_sprite(&[0b1111_0000], 0, 0);
        chip8.register[0xF] = 1;

        chip8.draw(0x0, 0x1, 1);

        assert_eq!(chip8.register[0xF], 0);
    }

    #[test]
    fn draw_start_position_wraps_around_screen() {
        // x = 72 → 72 mod 64 = 8, y = 35 → 35 mod 32 = 3
        let mut chip8 = machine_with_sprite(&[0b1111_0000], 72, 35);

        chip8.draw(0x0, 0x1, 1);

        assert_eq!(chip8.display[3], 0b1111_0000 << 48);
    }

    #[test]
    fn draw_clips_at_right_edge() {
        let mut chip8 = machine_with_sprite(&[0b1111_1111], 60, 0);

        chip8.draw(0x0, 0x1, 1);

        // Only the 4 leftmost sprite pixels fit; nothing wraps to x = 0.
        assert_eq!(chip8.display[0], 0b1111);
    }

    #[test]
    fn draw_clips_at_bottom_edge() {
        let mut chip8 = machine_with_sprite(&[0b1111_1111, 0b1111_1111], 0, 31);

        chip8.draw(0x0, 0x1, 2);

        assert_eq!(chip8.display[31], 0b1111_1111 << 56);
        assert_eq!(chip8.display[0], 0, "second row must not wrap to the top");
    }

    #[test]
    fn draw_zero_rows_draws_nothing() {
        let mut chip8 = machine_with_sprite(&[0b1111_1111], 0, 0);

        chip8.draw(0x0, 0x1, 0);

        assert_eq!(chip8.display, [0; HEIGHT]);
        assert_eq!(chip8.register[0xF], 0);
    }

    // ---------------------------------------------------------------------
    // logic_instruction (8XYN)
    // ---------------------------------------------------------------------

    /// Runs `8 1 2 n` with `V1 = vx`, `V2 = vy`, and `VF` preset to `0xAA` so
    /// that a flag that is never written is caught.
    fn logic(vx: u8, vy: u8, n: u16) -> Chip8 {
        let mut chip8 = Chip8::new();
        chip8.register[0x1] = vx;
        chip8.register[0x2] = vy;
        chip8.register[0xF] = 0xAA;
        exec(&mut chip8, 0x8120 | n).unwrap();
        chip8
    }

    #[test]
    fn logic_8xy0_copies_vy_into_vx() {
        let chip8 = logic(0x11, 0x22, 0x0);

        assert_eq!(chip8.register[0x1], 0x22);
        assert_eq!(chip8.register[0x2], 0x22);
    }

    #[test]
    fn logic_8xy1_or() {
        assert_eq!(logic(0b1100, 0b1010, 0x1).register[0x1], 0b1110);
    }

    #[test]
    fn logic_8xy2_and() {
        assert_eq!(logic(0b1100, 0b1010, 0x2).register[0x1], 0b1000);
    }

    #[test]
    fn logic_8xy3_xor() {
        assert_eq!(logic(0b1100, 0b1010, 0x3).register[0x1], 0b0110);
    }

    #[test]
    fn logic_8xy4_add_without_carry_clears_vf() {
        let chip8 = logic(10, 20, 0x4);

        assert_eq!(chip8.register[0x1], 30);
        assert_eq!(chip8.register[0xF], 0);
    }

    #[test]
    fn logic_8xy4_add_with_carry_wraps_and_sets_vf() {
        let chip8 = logic(0xFF, 0x02, 0x4);

        assert_eq!(chip8.register[0x1], 0x01);
        assert_eq!(chip8.register[0xF], 1);
    }

    #[test]
    fn logic_8xy5_sub_without_borrow_sets_vf() {
        let chip8 = logic(5, 3, 0x5);

        assert_eq!(chip8.register[0x1], 2);
        assert_eq!(chip8.register[0xF], 1);
    }

    #[test]
    fn logic_8xy5_sub_equal_values_sets_vf() {
        let chip8 = logic(5, 5, 0x5);

        assert_eq!(chip8.register[0x1], 0);
        assert_eq!(chip8.register[0xF], 1);
    }

    #[test]
    fn logic_8xy5_sub_with_borrow_wraps_and_clears_vf() {
        let chip8 = logic(3, 5, 0x5);

        assert_eq!(chip8.register[0x1], 0xFE);
        assert_eq!(chip8.register[0xF], 0);
    }

    #[test]
    fn logic_8xy7_reverse_sub_without_borrow_sets_vf() {
        let chip8 = logic(3, 5, 0x7);

        assert_eq!(chip8.register[0x1], 2);
        assert_eq!(chip8.register[0xF], 1);
    }

    #[test]
    fn logic_8xy7_reverse_sub_with_borrow_wraps_and_clears_vf() {
        let chip8 = logic(5, 3, 0x7);

        assert_eq!(chip8.register[0x1], 0xFE);
        assert_eq!(chip8.register[0xF], 0);
    }

    // The shift tests use VX == VY, so they pass with both the COSMAC VIP
    // (`VX = VY` first) and the modern (shift VX in place) behavior.

    #[test]
    fn logic_8xy6_shift_right_puts_low_bit_in_vf() {
        let chip8 = logic(0b0000_0011, 0b0000_0011, 0x6);

        assert_eq!(chip8.register[0x1], 0b0000_0001);
        assert_eq!(chip8.register[0xF], 1);
    }

    #[test]
    fn logic_8xy6_shift_right_without_low_bit_clears_vf() {
        let chip8 = logic(0b0000_0010, 0b0000_0010, 0x6);

        assert_eq!(chip8.register[0x1], 0b0000_0001);
        assert_eq!(chip8.register[0xF], 0);
    }

    #[test]
    fn logic_8xye_shift_left_puts_high_bit_in_vf() {
        let chip8 = logic(0b1000_0001, 0b1000_0001, 0xE);

        assert_eq!(chip8.register[0x1], 0b0000_0010);
        assert_eq!(chip8.register[0xF], 1);
    }

    #[test]
    fn logic_8xye_shift_left_without_high_bit_clears_vf() {
        let chip8 = logic(0b0100_0000, 0b0100_0000, 0xE);

        assert_eq!(chip8.register[0x1], 0b1000_0000);
        assert_eq!(chip8.register[0xF], 0);
    }

    #[test]
    fn logic_8fy4_with_vf_as_destination_keeps_the_flag() {
        let mut chip8 = Chip8::new();
        chip8.register[0xF] = 0xFF;
        chip8.register[0x1] = 0x02;

        exec(&mut chip8, 0x8F14).unwrap();

        // The flag is written after the result, so VF holds the carry.
        assert_eq!(chip8.register[0xF], 1);
    }

    #[test]
    fn logic_8xy8_is_unknown_opcode() {
        let mut chip8 = Chip8::new();

        assert!(matches!(
            exec(&mut chip8, 0x8128),
            Err(ExecutionError::UnknownOpcode { opcode: 0x8128 })
        ));
    }

    // ---------------------------------------------------------------------
    // decode: instructions added after the IBM-logo subset
    // ---------------------------------------------------------------------

    #[test]
    fn decode_00ee_is_return_subroutine() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x00EE),
            Ok(Instruction::ReturnSubroutine)
        ));
    }

    #[test]
    fn decode_2nnn_is_call_subroutine() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x2ABC),
            Ok(Instruction::CallSubroutine { nnn: 0xABC })
        ));
    }

    #[test]
    fn decode_3xnn_reads_vx_and_nn() {
        let mut chip8 = Chip8::new();
        chip8.register[0x4] = 0x11;

        assert!(matches!(
            chip8.decode(0x3422),
            Ok(Instruction::SkipConditionAND { a: 0x11, b: 0x22 })
        ));
    }

    #[test]
    fn decode_4xnn_reads_vx_and_nn() {
        let mut chip8 = Chip8::new();
        chip8.register[0x4] = 0x11;

        assert!(matches!(
            chip8.decode(0x4422),
            Ok(Instruction::SkipConditionXOR { a: 0x11, b: 0x22 })
        ));
    }

    #[test]
    fn decode_5xy0_reads_vx_and_vy() {
        let mut chip8 = Chip8::new();
        chip8.register[0x1] = 0x11;
        chip8.register[0x2] = 0x22;

        assert!(matches!(
            chip8.decode(0x5120),
            Ok(Instruction::SkipConditionAND { a: 0x11, b: 0x22 })
        ));
    }

    #[test]
    fn decode_9xy0_reads_vx_and_vy() {
        let mut chip8 = Chip8::new();
        chip8.register[0x1] = 0x11;
        chip8.register[0x2] = 0x22;

        assert!(matches!(
            chip8.decode(0x9120),
            Ok(Instruction::SkipConditionXOR { a: 0x11, b: 0x22 })
        ));
    }

    #[test]
    fn decode_5xy1_and_9xy1_are_unknown() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x5121),
            Err(ExecutionError::UnknownOpcode { opcode: 0x5121 })
        ));
        assert!(matches!(
            chip8.decode(0x9121),
            Err(ExecutionError::UnknownOpcode { opcode: 0x9121 })
        ));
    }

    #[test]
    fn decode_8xyn_is_logical() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0x8124),
            Ok(Instruction::Logical {
                x: 0x1,
                y: 0x2,
                n: 0x4
            })
        ));
    }

    // ---------------------------------------------------------------------
    // execute / step: full fetch–decode–execute cycles from a ROM
    // ---------------------------------------------------------------------

    #[test]
    fn step_00e0_clears_display_and_advances_pc() {
        let mut chip8 = machine_with(&[0x00, 0xE0]);
        chip8.display = [u64::MAX; HEIGHT];

        chip8.step().unwrap();

        assert_eq!(chip8.display, [0; HEIGHT]);
        assert_eq!(chip8.program_counter, PROGRAM_START + 2);
    }

    #[test]
    fn step_1nnn_jumps() {
        let mut chip8 = machine_with(&[0x13, 0x45]);

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x345);
    }

    #[test]
    fn step_2nnn_pushes_return_address_and_jumps() {
        let mut chip8 = machine_with(&[0x23, 0x00]); // CALL 0x300

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x300);
        assert_eq!(chip8.stack, [0x202]);
    }

    #[test]
    fn step_call_then_return_resumes_after_the_call() {
        #[rustfmt::skip]
        let mut chip8 = machine_with(&[
            0x22, 0x04, // 0x200: CALL 0x204
            0x00, 0xE0, // 0x202: CLS
            0x00, 0xEE, // 0x204: RET
        ]);

        step_n(&mut chip8, 2);

        assert_eq!(chip8.program_counter, 0x202);
        assert!(chip8.stack.is_empty());
    }

    #[test]
    fn step_nested_calls_return_in_reverse_order() {
        #[rustfmt::skip]
        let mut chip8 = machine_with(&[
            0x22, 0x04, // 0x200: CALL 0x204
            0x00, 0xE0, // 0x202: CLS
            0x22, 0x08, // 0x204: CALL 0x208
            0x00, 0xEE, // 0x206: RET
            0x00, 0xEE, // 0x208: RET
        ]);

        step_n(&mut chip8, 2);
        assert_eq!(chip8.program_counter, 0x208);
        assert_eq!(chip8.stack, [0x202, 0x206]);

        chip8.step().unwrap();
        assert_eq!(chip8.program_counter, 0x206);

        chip8.step().unwrap();
        assert_eq!(chip8.program_counter, 0x202);
        assert!(chip8.stack.is_empty());
    }

    #[test]
    fn step_00ee_with_empty_stack_is_an_error() {
        let mut chip8 = machine_with(&[0x00, 0xEE]);

        assert!(matches!(chip8.step(), Err(ExecutionError::EmptyStack)));
    }

    #[test]
    fn step_3xnn_skips_when_equal() {
        let mut chip8 = machine_with(&[0x60, 0x42, 0x30, 0x42]); // V0 = 42; SE V0, 42

        step_n(&mut chip8, 2);

        assert_eq!(chip8.program_counter, 0x206);
    }

    #[test]
    fn step_3xnn_does_not_skip_when_different() {
        let mut chip8 = machine_with(&[0x60, 0x41, 0x30, 0x42]);

        step_n(&mut chip8, 2);

        assert_eq!(chip8.program_counter, 0x204);
    }

    #[test]
    fn step_4xnn_skips_when_different() {
        let mut chip8 = machine_with(&[0x60, 0x41, 0x40, 0x42]); // V0 = 41; SNE V0, 42

        step_n(&mut chip8, 2);

        assert_eq!(chip8.program_counter, 0x206);
    }

    #[test]
    fn step_4xnn_does_not_skip_when_equal() {
        let mut chip8 = machine_with(&[0x60, 0x42, 0x40, 0x42]);

        step_n(&mut chip8, 2);

        assert_eq!(chip8.program_counter, 0x204);
    }

    #[test]
    fn step_5xy0_skips_when_registers_equal() {
        // V0 = 7; V1 = 7; SE V0, V1
        let mut chip8 = machine_with(&[0x60, 0x07, 0x61, 0x07, 0x50, 0x10]);

        step_n(&mut chip8, 3);

        assert_eq!(chip8.program_counter, 0x208);
    }

    #[test]
    fn step_5xy0_does_not_skip_when_registers_differ() {
        let mut chip8 = machine_with(&[0x60, 0x07, 0x61, 0x08, 0x50, 0x10]);

        step_n(&mut chip8, 3);

        assert_eq!(chip8.program_counter, 0x206);
    }

    #[test]
    fn step_9xy0_skips_when_registers_differ() {
        // V0 = 7; V1 = 8; SNE V0, V1
        let mut chip8 = machine_with(&[0x60, 0x07, 0x61, 0x08, 0x90, 0x10]);

        step_n(&mut chip8, 3);

        assert_eq!(chip8.program_counter, 0x208);
    }

    #[test]
    fn step_9xy0_does_not_skip_when_registers_equal() {
        let mut chip8 = machine_with(&[0x60, 0x07, 0x61, 0x07, 0x90, 0x10]);

        step_n(&mut chip8, 3);

        assert_eq!(chip8.program_counter, 0x206);
    }

    #[test]
    fn step_6xnn_then_7xnn_sets_then_adds() {
        let mut chip8 = machine_with(&[0x6A, 0x10, 0x7A, 0x05]); // VA = 0x10; VA += 5

        step_n(&mut chip8, 2);

        assert_eq!(chip8.register[0xA], 0x15);
    }

    #[test]
    fn step_annn_sets_index() {
        let mut chip8 = machine_with(&[0xA1, 0x23]);

        chip8.step().unwrap();

        assert_eq!(chip8.i, 0x123);
    }

    #[test]
    fn step_dxyn_draws_sprite_from_memory() {
        #[rustfmt::skip]
        let mut chip8 = machine_with(&[
            0xA2, 0x04,  // 0x200: I = 0x204
            0xD0, 0x01,  // 0x202: DRW V0, V0, 1   (V0 = 0)
            0b1111_0000, // 0x204: sprite row
        ]);

        step_n(&mut chip8, 2);

        assert_eq!(chip8.display[0], 0b1111_0000 << 56);
    }

    #[test]
    fn step_unknown_opcode_is_an_error() {
        let mut chip8 = machine_with(&[0x01, 0x23]);

        assert!(matches!(
            chip8.step(),
            Err(ExecutionError::UnknownOpcode { opcode: 0x0123 })
        ));
    }

    // ---------------------------------------------------------------------
    // BNNN: jump with offset
    // ---------------------------------------------------------------------

    /// `BNNN` keeps the full 12-bit address.
    #[test]
    fn decode_bnnn_is_jump_with_offset() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xB123),
            Ok(Instruction::JumpWithOffset(0x123))
        ));
    }

    /// The target is `NNN + V0`.
    #[test]
    fn step_bnnn_jumps_to_nnn_plus_v0() {
        let mut chip8 = machine_with(&[0xB3, 0x00]); // JP V0, 0x300
        chip8.register[0x0] = 0x10;

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x310);
    }

    /// COSMAC VIP quirk: the offset always comes from `V0`, never from `VX`
    /// (`B220` jumps to `0x220 + V0`, not `0x220 + V2`).
    #[test]
    fn step_bnnn_uses_v0_not_vx() {
        let mut chip8 = machine_with(&[0xB2, 0x20]);
        chip8.register[0x2] = 0x50;

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x220);
    }

    /// `NNN + V0` can reach past `0xFFF`; the PC wraps like the address bus.
    #[test]
    fn exec_bnnn_wraps_past_end_of_memory() {
        let mut chip8 = Chip8::new();
        chip8.register[0x0] = 0xFF;

        exec(&mut chip8, 0xBFFF).unwrap();

        assert_eq!(chip8.program_counter, (0xFFF + 0xFF) % MEMORY_SIZE);
    }

    // ---------------------------------------------------------------------
    // CXNN: random
    //
    // The random byte cannot be predicted, so these tests check the mask,
    // which is the deterministic part of the instruction.
    // ---------------------------------------------------------------------

    #[test]
    fn decode_cxnn_is_random() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xC3F0),
            Ok(Instruction::Random { x: 0x3, nn: 0xF0 })
        ));
    }

    /// A zero mask always gives zero, whatever the random byte was.
    #[test]
    fn exec_cxnn_with_zero_mask_gives_zero() {
        let mut chip8 = Chip8::new();
        chip8.register[0x3] = 0xAA;

        exec(&mut chip8, 0xC300).unwrap();

        assert_eq!(chip8.register[0x3], 0);
    }

    /// Bits outside the mask are never set (1000 draws).
    #[test]
    fn exec_cxnn_never_sets_bits_outside_the_mask() {
        let mut chip8 = Chip8::new();

        for _ in 0..1000 {
            exec(&mut chip8, 0xC30F).unwrap();
            assert_eq!(chip8.register[0x3] & 0xF0, 0);
        }
    }

    /// With a full mask the values are actually random: 100 draws all equal
    /// would happen with probability 256^-99.
    #[test]
    fn exec_cxnn_with_full_mask_produces_different_values() {
        let mut chip8 = Chip8::new();
        let mut seen = [false; 256];

        for _ in 0..100 {
            exec(&mut chip8, 0xC3FF).unwrap();
            seen[usize::from(chip8.register[0x3])] = true;
        }

        assert!(seen.iter().filter(|&&s| s).count() > 1);
    }

    // ---------------------------------------------------------------------
    // EX9E / EXA1: skip on key state
    //
    // ROM: `E1 9E` (or `E1 A1`) at 0x200, `V1 = 0x5`. A skip leaves the PC
    // at 0x204, no skip at 0x202.
    // ---------------------------------------------------------------------

    /// A machine running `opcode` with `V1 = 5` and `key` held down.
    fn key_machine(opcode: [u8; 2], key: Option<u8>) -> Chip8 {
        let mut chip8 = machine_with(&opcode);
        chip8.register[0x1] = 0x5;
        chip8.key_pressed = key;
        chip8
    }

    #[test]
    fn decode_ex9e_and_exa1() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xE19E),
            Ok(Instruction::SkipIfPressed { x: 0x1 })
        ));
        assert!(matches!(
            chip8.decode(0xE1A1),
            Ok(Instruction::SkipIfNotPressed { x: 0x1 })
        ));
    }

    #[test]
    fn step_ex9e_skips_when_key_in_vx_is_pressed() {
        let mut chip8 = key_machine([0xE1, 0x9E], Some(0x5));

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x204);
    }

    #[test]
    fn step_ex9e_does_not_skip_when_another_key_is_pressed() {
        let mut chip8 = key_machine([0xE1, 0x9E], Some(0x6));

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x202);
    }

    #[test]
    fn step_ex9e_does_not_skip_when_no_key_is_pressed() {
        let mut chip8 = key_machine([0xE1, 0x9E], None);

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x202);
    }

    /// Key `0` is a real key: it must not be confused with "no key".
    #[test]
    fn step_ex9e_handles_key_zero() {
        let mut chip8 = key_machine([0xE2, 0x9E], Some(0x0)); // V2 = 0

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x204);
    }

    #[test]
    fn step_exa1_skips_when_no_key_is_pressed() {
        let mut chip8 = key_machine([0xE1, 0xA1], None);

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x204);
    }

    #[test]
    fn step_exa1_skips_when_another_key_is_pressed() {
        let mut chip8 = key_machine([0xE1, 0xA1], Some(0x6));

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x204);
    }

    #[test]
    fn step_exa1_does_not_skip_when_key_in_vx_is_pressed() {
        let mut chip8 = key_machine([0xE1, 0xA1], Some(0x5));

        chip8.step().unwrap();

        assert_eq!(chip8.program_counter, 0x202);
    }

    // ---------------------------------------------------------------------
    // FX07 / FX15 / FX18: timers
    // ---------------------------------------------------------------------

    #[test]
    fn decode_fx07_fx15_fx18() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xF307),
            Ok(Instruction::SetRegisterFromDelayTimer { x: 0x3 })
        ));
        assert!(matches!(
            chip8.decode(0xF315),
            Ok(Instruction::SetDelayTimer { x: 0x3 })
        ));
        assert!(matches!(
            chip8.decode(0xF318),
            Ok(Instruction::SetSoundTimer { x: 0x3 })
        ));
    }

    /// `FX07`: `VX = delay timer`.
    #[test]
    fn exec_fx07_reads_delay_timer_into_vx() {
        let mut chip8 = Chip8::new();
        chip8.delay_timer = 0x42;

        exec(&mut chip8, 0xF307).unwrap();

        assert_eq!(chip8.register[0x3], 0x42);
    }

    /// `FX15`: `delay timer = VX`; the sound timer is not touched.
    #[test]
    fn exec_fx15_sets_delay_timer_from_vx() {
        let mut chip8 = Chip8::new();
        chip8.register[0x3] = 0x42;

        exec(&mut chip8, 0xF315).unwrap();

        assert_eq!(chip8.delay_timer, 0x42);
        assert_eq!(chip8.sound_timer, 0);
    }

    /// `FX18`: `sound timer = VX`; the delay timer is not touched.
    #[test]
    fn exec_fx18_sets_sound_timer_from_vx() {
        let mut chip8 = Chip8::new();
        chip8.register[0x3] = 0x42;

        exec(&mut chip8, 0xF318).unwrap();

        assert_eq!(chip8.sound_timer, 0x42);
        assert_eq!(chip8.delay_timer, 0);
    }

    /// `FX15` then `FX07` copies a value from one register to another
    /// through the delay timer (no time passes between the two).
    #[test]
    fn step_fx15_then_fx07_round_trips_through_delay_timer() {
        // V1 = 0x99; DT = V1; V2 = DT
        let mut chip8 = machine_with(&[0x61, 0x99, 0xF1, 0x15, 0xF2, 0x07]);

        step_n(&mut chip8, 3);

        assert_eq!(chip8.register[0x2], 0x99);
    }

    // ---------------------------------------------------------------------
    // FX1E: I += VX
    // ---------------------------------------------------------------------

    #[test]
    fn decode_fx1e_is_add_to_index() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xF21E),
            Ok(Instruction::AddToIndex { x: 0x2 })
        ));
    }

    #[test]
    fn exec_fx1e_adds_vx_to_index() {
        let mut chip8 = Chip8::new();
        chip8.i = 0x100;
        chip8.register[0x2] = 0x20;

        exec(&mut chip8, 0xF21E).unwrap();

        assert_eq!(chip8.i, 0x120);
        assert_eq!(chip8.register[0x2], 0x20, "VX is only read");
    }

    /// Amiga quirk: `VF = 1` when `I` goes past `0x0FFF`.
    #[test]
    fn exec_fx1e_past_0xfff_sets_vf() {
        let mut chip8 = Chip8::new();
        chip8.i = 0xFFF;
        chip8.register[0x2] = 0x01;

        exec(&mut chip8, 0xF21E).unwrap();

        assert_eq!(chip8.register[0xF], 1);
    }

    // ---------------------------------------------------------------------
    // FX0A: wait for a key
    // ---------------------------------------------------------------------

    #[test]
    fn decode_fx0a_is_get_key() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xF30A),
            Ok(Instruction::GetKey { x: 0x3 })
        ));
    }

    /// With no key the PC is moved back onto `FX0A`, so the next step runs
    /// it again: this is how the instruction blocks.
    #[test]
    fn step_fx0a_without_key_stays_on_the_instruction() {
        let mut chip8 = machine_with(&[0xF3, 0x0A]);
        chip8.register[0x3] = 0xAA;

        step_n(&mut chip8, 3);

        assert_eq!(chip8.program_counter, PROGRAM_START);
        assert_eq!(chip8.register[0x3], 0xAA, "VX is untouched while waiting");
    }

    #[test]
    fn step_fx0a_with_key_stores_it_and_moves_on() {
        let mut chip8 = machine_with(&[0xF3, 0x0A]);
        chip8.key_pressed = Some(0xB);

        chip8.step().unwrap();

        assert_eq!(chip8.register[0x3], 0xB);
        assert_eq!(chip8.program_counter, PROGRAM_START + 2);
    }

    /// Blocks while no key is held, then resumes as soon as one is.
    #[test]
    fn step_fx0a_waits_then_resumes_when_a_key_arrives() {
        let mut chip8 = machine_with(&[0xF3, 0x0A]);

        step_n(&mut chip8, 2);
        assert_eq!(chip8.program_counter, PROGRAM_START);

        chip8.key_pressed = Some(0x0);
        chip8.step().unwrap();

        assert_eq!(chip8.register[0x3], 0x0);
        assert_eq!(chip8.program_counter, PROGRAM_START + 2);
    }

    // ---------------------------------------------------------------------
    // FX29: font character
    // ---------------------------------------------------------------------

    #[test]
    fn decode_fx29_is_font_character() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xF329),
            Ok(Instruction::FontCharacter { x: 0x3 })
        ));
    }

    /// `I` points at the sprite of the digit *stored in* `VX` (5 bytes per
    /// digit). X = 3 and VX = 0xA differ on purpose, so using the register
    /// number instead of its value is caught.
    #[test]
    fn exec_fx29_points_index_at_digit_in_vx() {
        let mut chip8 = Chip8::new();
        chip8.register[0x3] = 0xA;

        exec(&mut chip8, 0xF329).unwrap();

        assert_eq!(chip8.i, FONT_START + 0xA * 5);
        assert_eq!(
            chip8.memory[chip8.i..chip8.i + 5],
            FONT[0xA * 5..0xA * 5 + 5]
        );
    }

    /// Every digit 0..=F lands on its own 5-byte sprite.
    #[test]
    fn exec_fx29_covers_all_digits() {
        let mut chip8 = Chip8::new();

        for digit in 0x0..=0xF_u8 {
            chip8.register[0x0] = digit;

            exec(&mut chip8, 0xF029).unwrap();

            let start = usize::from(digit) * 5;
            assert_eq!(chip8.i, FONT_START + start, "digit {digit:X}");
            assert_eq!(chip8.memory[chip8.i..chip8.i + 5], FONT[start..start + 5]);
        }
    }

    // ---------------------------------------------------------------------
    // FX33: binary-coded decimal
    //
    // Memory at I is preset to 0xFF so a digit that is never written is caught.
    // ---------------------------------------------------------------------

    /// Runs `F3 33` with `V3 = value` and `I = 0x300`; returns the machine.
    fn bcd(value: u8) -> Chip8 {
        let mut chip8 = Chip8::new();
        chip8.i = 0x300;
        chip8.memory[0x300..0x303].fill(0xFF);
        chip8.register[0x3] = value;
        exec(&mut chip8, 0xF333).unwrap();
        chip8
    }

    #[test]
    fn decode_fx33_is_decimal_conversion() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xF333),
            Ok(Instruction::DecimalConversion { x: 0x3 })
        ));
    }

    /// The example from the spec: 156 → 1, 5, 6.
    #[test]
    fn exec_fx33_stores_three_digits_at_index() {
        let chip8 = bcd(156);

        assert_eq!(chip8.memory[0x300..0x303], [1, 5, 6]);
    }

    /// Leading zeros are written too: always exactly three digits.
    #[test]
    fn exec_fx33_writes_leading_zeros() {
        assert_eq!(bcd(7).memory[0x300..0x303], [0, 0, 7]);
        assert_eq!(bcd(42).memory[0x300..0x303], [0, 4, 2]);
    }

    /// Zero is a value like any other: three zero digits.
    #[test]
    fn exec_fx33_of_zero_writes_three_zeros() {
        assert_eq!(bcd(0).memory[0x300..0x303], [0, 0, 0]);
    }

    /// The largest byte, 255.
    #[test]
    fn exec_fx33_of_255() {
        assert_eq!(bcd(255).memory[0x300..0x303], [2, 5, 5]);
    }

    /// `FX33` writes memory only: `I` and the registers are unchanged.
    #[test]
    fn exec_fx33_leaves_index_and_registers_unchanged() {
        let chip8 = bcd(156);

        assert_eq!(chip8.i, 0x300);
        let mut expected = [0; REGISTER_SIZE];
        expected[0x3] = 156;
        assert_eq!(chip8.register, expected);
        assert_eq!(chip8.memory[0x303], 0, "nothing past I + 2");
    }

    // ---------------------------------------------------------------------
    // FX55 / FX65: store and load registers
    // ---------------------------------------------------------------------

    #[test]
    fn decode_fx55_and_fx65() {
        let chip8 = Chip8::new();

        assert!(matches!(
            chip8.decode(0xF355),
            Ok(Instruction::StoreMemory { x: 0x3 })
        ));
        assert!(matches!(
            chip8.decode(0xF365),
            Ok(Instruction::LoadMemory { x: 0x3 })
        ));
    }

    /// `FX55` stores `V0..=VX` (inclusive) and nothing after; `I` is
    /// unchanged (modern quirk).
    #[test]
    fn exec_fx55_stores_v0_to_vx_at_index() {
        let mut chip8 = Chip8::new();
        chip8.i = 0x300;
        chip8.register[..5].copy_from_slice(&[1, 2, 3, 4, 5]);

        exec(&mut chip8, 0xF355).unwrap();

        assert_eq!(chip8.memory[0x300..0x305], [1, 2, 3, 4, 0]);
        assert_eq!(chip8.i, 0x300);
    }

    /// `F055` stores only `V0`.
    #[test]
    fn exec_f055_stores_only_v0() {
        let mut chip8 = Chip8::new();
        chip8.i = 0x300;
        chip8.register[..2].copy_from_slice(&[0xAA, 0xBB]);

        exec(&mut chip8, 0xF055).unwrap();

        assert_eq!(chip8.memory[0x300..0x302], [0xAA, 0x00]);
    }

    /// `FF55` stores all 16 registers, `VF` included.
    #[test]
    fn exec_ff55_stores_all_registers() {
        let mut chip8 = Chip8::new();
        chip8.i = 0x300;
        let values: [u8; REGISTER_SIZE] = core::array::from_fn(|n| n as u8 + 1);
        chip8.register = values;

        exec(&mut chip8, 0xFF55).unwrap();

        assert_eq!(chip8.memory[0x300..0x310], values);
    }

    /// `FX65` loads `V0..=VX` (inclusive); later registers and `I` are unchanged.
    #[test]
    fn exec_fx65_loads_v0_to_vx_from_index() {
        let mut chip8 = Chip8::new();
        chip8.i = 0x300;
        chip8.memory[0x300..0x304].copy_from_slice(&[9, 8, 7, 6]);
        chip8.register[0x3] = 0xAA;

        exec(&mut chip8, 0xF265).unwrap();

        assert_eq!(chip8.register[..4], [9, 8, 7, 0xAA]);
        assert_eq!(chip8.i, 0x300);
    }

    /// `FX55` then `FX65` restores the saved registers.
    #[test]
    fn step_fx55_then_fx65_round_trips_registers() {
        #[rustfmt::skip]
        let mut chip8 = machine_with(&[
            0xA3, 0x00, // I = 0x300
            0xF2, 0x55, // store V0..V2
            0x60, 0x00, // V0 = 0
            0x61, 0x00, // V1 = 0
            0x62, 0x00, // V2 = 0
            0xF2, 0x65, // load V0..V2
        ]);
        chip8.register[..3].copy_from_slice(&[0x11, 0x22, 0x33]);

        step_n(&mut chip8, 6);

        assert_eq!(chip8.register[..3], [0x11, 0x22, 0x33]);
    }

    /// Each tick counts both timers down by one, and they stop at 0 instead of wrapping.
    #[test]
    fn tick_timers_counts_down_and_stops_at_zero() {
        let mut chip8 = Chip8::new();
        chip8.delay_timer = 2;
        chip8.sound_timer = 1;

        chip8.tick_timers();
        assert_eq!((chip8.delay_timer, chip8.sound_timer), (1, 0));

        chip8.tick_timers();
        chip8.tick_timers();
        assert_eq!((chip8.delay_timer, chip8.sound_timer), (0, 0));
    }
}
