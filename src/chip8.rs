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

/// Number of general-purpose registers (`V0`–`VF`).
const REGISTER_SIZE: usize = 16;

const VF: usize = 0xF;

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
    JumpWithOffset(u16),
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
            stack: Vec::new(),
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
            return Err(ExecutionError::IndexToHigh { index, max: VF });
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
            return Err(ExecutionError::IndexToHigh { index, max: VF });
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
            return Err(ExecutionError::IndexToHigh { index, max: VF });
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
    }

    fn overflowing_add(&mut self, index: usize, a: u8, b: u8) -> Result<u8, ExecutionError> {
        self.set_register(index, a + b)?;

        if u16::from(a) + u16::from(b) > 255 {
            Ok(1_u8)
        } else {
            Ok(0_u8)
        }
    }

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
            (0x8, _, _, _) => Ok(Instruction::Logical { x, y, n }),
            (0x9, _, _, 0x0) => Ok(Instruction::SkipConditionXOR {
                a: self.get_register(usize::from(x))?,
                b: self.get_register(usize::from(y))?,
            }),
            (0xA, _, _, _) => Ok(Instruction::SetIndex(opcode & 0x0FFF)),
            (0xD, _, _, _) => Ok(Instruction::Draw { x, y, n }),
            _ => Err(ExecutionError::UnknownOpcode { opcode }),
        }
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
            Instruction::ClearScreen => {
                self.display = [0; HEIGHT];
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

    /// Walks all 65 536 opcodes: only the implemented instructions decode
    /// (`00E0`, `00EE`, `1NNN`–`4XNN`, `5XY0`, `6XNN`, `7XNN`, `8XY0`–`8XY7`,
    /// `8XYE`, `9XY0`, `ANNN`, `DXYN`); every other opcode is rejected and
    /// reported as itself.
    #[test]
    fn decode_accepts_exactly_the_supported_opcodes() {
        let chip8 = Chip8::new();

        for opcode in 0..=u16::MAX {
            let kind = opcode >> 12;
            let n = opcode & 0x000F;
            let supported = matches!(opcode, 0x00E0 | 0x00EE)
                || matches!(kind, 0x1 | 0x2 | 0x3 | 0x4 | 0x6 | 0x7 | 0xA | 0xD)
                || (matches!(kind, 0x5 | 0x9) && n == 0x0)
                || (kind == 0x8 && matches!(n, 0x0..=0x7 | 0xE));

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
}
