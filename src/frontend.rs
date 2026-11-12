use minifb::{Window, WindowOptions};

pub const WIDTH: usize = 64;
pub const HEIGHT: usize = 32;

const _: () = assert!(WIDTH == u64::BITS as usize);

const FPS: usize = 60;

const ON_COLOR: u32 = 0x00FFFFFF;
const OFF_COLOR: u32 = 0x00000000;

pub struct MfbWindow {
    buffer: [[u32; WIDTH]; HEIGHT],
    pub window: Window,
}

impl MfbWindow {
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
        win.window.set_target_fps(FPS);
        Ok(win)
    }

    pub fn update_buffer(&mut self, new_buffer: &[u64; HEIGHT]) {
        for (row, bits) in self.buffer.iter_mut().zip(new_buffer) {
            for (x, pixel) in row.iter_mut().enumerate() {
                let bit = (bits >> (WIDTH - 1 - x)) & 1;

                *pixel = if bit == 1 { ON_COLOR } else { OFF_COLOR };
            }
        }
    }

    pub fn draw(&mut self) -> Result<(), minifb::Error> {
        self.window
            .update_with_buffer(self.buffer.as_flattened(), WIDTH, HEIGHT)
    }
}
