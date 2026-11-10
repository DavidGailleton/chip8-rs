const MEMORY_SIZE: usize = 4096;

struct Chip8 {
    memory: [u8; MEMORY_SIZE],
    program_counter: u16,
    i: u16,
    delay_timer: u8,
    sound_timer: u8,
    variables: [u8; 16],
}

fn main() {
    println!("Hello, world!");
}
