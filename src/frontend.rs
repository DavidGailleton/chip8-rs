//! The window front-end: shows the CHIP-8 display with `minifb`.

use minifb::{Key, Window, WindowOptions};

/// Display width in pixels. One display row fits exactly in a `u64`.
pub const WIDTH: usize = 64;
/// Display height in pixels.
pub const HEIGHT: usize = 32;

const _: () = assert!(WIDTH == u64::BITS as usize);

/// Window refresh rate, in frames per second.
pub const FPS: u32 = 60;

/// Color of a lit pixel (`0x00RRGGBB`).
const ON_COLOR: u32 = 0x00FFFFFF;
/// Color of an unlit pixel (`0x00RRGGBB`).
const OFF_COLOR: u32 = 0x00000000;

/// A `minifb` window scaled ×16, with the pixel buffer it displays.
pub struct MfbWindow {
    /// One `0x00RRGGBB` color per pixel, row by row.
    buffer: [[u32; WIDTH]; HEIGHT],
    /// The underlying window; use it to check [`Window::is_open`].
    pub window: Window,
}

impl MfbWindow {
    /// Opens a 64×32 window scaled ×16.
    ///
    /// minifb's own frame limiter (250 FPS by default) is turned off: the main
    /// loop paces frames itself, and two limiters would add their sleeps.
    ///
    /// # Errors
    ///
    /// Returns the `minifb` error if the window cannot be created.
    pub fn new() -> Result<Self, minifb::Error> {
        let mut win = Self {
            buffer: [[OFF_COLOR; WIDTH]; HEIGHT],
            window: Window::new(
                "chip8-rs",
                WIDTH,
                HEIGHT,
                WindowOptions {
                    scale: minifb::Scale::X16,
                    ..WindowOptions::default()
                },
            )?,
        };
        win.window.set_target_fps(0);
        Ok(win)
    }

    /// Converts the CHIP-8 display (one `u64` per row, most significant bit
    /// on the left) into pixel colors. Call [`MfbWindow::draw`] to show them.
    pub fn update_buffer(&mut self, new_buffer: &[u64; HEIGHT]) {
        for (row, bits) in self.buffer.iter_mut().zip(new_buffer) {
            for (x, pixel) in row.iter_mut().enumerate() {
                let bit = (bits >> (WIDTH - 1 - x)) & 1;

                *pixel = if bit == 1 { ON_COLOR } else { OFF_COLOR };
            }
        }
    }

    /// Shows the pixel buffer in the window and processes window events.
    ///
    /// # Errors
    ///
    /// Returns the `minifb` error if the window cannot be updated.
    pub fn draw(&mut self) -> Result<(), minifb::Error> {
        self.window
            .update_with_buffer(self.buffer.as_flattened(), WIDTH, HEIGHT)
    }

    /// Processes window events (keys, close button) without redrawing.
    ///
    /// minifb only polls events inside `update`/`update_with_buffer`, so call
    /// this on frames where the display did not change, or the window freezes.
    pub fn update(&mut self) {
        self.window.update();
    }

    pub fn get_key_pressed(&self) -> Option<u8> {
        // CHIP-8 keypad      QWERTY keys
        // 1 2 3 C            1 2 3 4
        // 4 5 6 D            Q W E R
        // 7 8 9 E            A S D F
        // A 0 B F            Z X C V
        match self.window.get_keys().first()? {
            Key::Key1 => Some(0x1),
            Key::Key2 => Some(0x2),
            Key::Key3 => Some(0x3),
            Key::Key4 => Some(0xC),
            Key::Q => Some(0x4),
            Key::W => Some(0x5),
            Key::E => Some(0x6),
            Key::R => Some(0xD),
            Key::A => Some(0x7),
            Key::S => Some(0x8),
            Key::D => Some(0x9),
            Key::F => Some(0xE),
            Key::Z => Some(0xA),
            Key::X => Some(0x0),
            Key::C => Some(0xB),
            Key::V => Some(0xF),
            _ => None,
        }
    }
}
