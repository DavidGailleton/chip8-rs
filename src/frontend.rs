use minifb::{Window, WindowOptions};

const WIDTH: usize = 64;
const HEIGHT: usize = 32;
const FPS: usize = 60;

const ON_COLOR: u32 = 0x00FFFFFF;
const OFF_COLOR: u32 = 0x00000000;

pub struct MfbWindow {
    buffer: Vec<u32>,
    pub window: Window,
}

impl MfbWindow {
    pub fn new() -> Result<Self, minifb::Error> {
        let mut win = Self {
            buffer: vec![0; WIDTH * HEIGHT],
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

    pub fn update_buffer(&mut self, new_buffer: &[u64; 32]) {
        for (row, bits) in self.buffer.chunks_exact_mut(HEIGHT).zip(new_buffer) {
            for (pixel, color) in row.iter_mut().zip(0..WIDTH) {
                let bit = (bits >> 1) & 1;

                if bit == 1 {
                    *pixel = ON_COLOR;
                } else {
                    *pixel = OFF_COLOR;
                }
            }
        }
    }

    pub fn draw(&mut self) -> Result<(), minifb::Error> {
        self.window.update_with_buffer(&self.buffer, WIDTH, HEIGHT)
    }
}
