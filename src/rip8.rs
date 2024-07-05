// Implementation of Chip8 based on:
// - RCA COSMAC VIP CDP18S711 Instruction Manual
// - https://github.com/mattmikolay/chip-8/wiki/CHIP%E2%80%908-Instruction-Set
// - http://devernay.free.fr/hacks/chip8/C8TECH10.HTM

pub const RIP8_MEMORY_SIZE: usize = 0x1000;
pub const RIP8_ROM_START: u16 = 0x200;
pub const RIP8_STACK_MAX_SIZE: usize = 0x40;
pub const RIP8_DISPLAY_WIDTH: usize = 64;
pub const RIP8_DISPLAY_HEIGHT: usize = 32;
pub const RIP8_KEY_COUNT: usize = 0x10;
pub const RIP8_DISPLAY_SIZE: usize = RIP8_DISPLAY_WIDTH * RIP8_DISPLAY_HEIGHT / 8;

pub struct Rip8 {
    pc: u16,
    memory: Vec<u8>,
    stack: Vec<u8>, // on the original COSMAC VIP interpreter the stack was
                    // located on the main memory, but later implementations
                    // differ and programs can't rely on the stack being on
                    // any specifit memory location. Thus, we store it 
                    // separately and keep the extra memory
    v: [u8; 16],
    i: u16,
    display: Vec<u8>,
    keyboard: [bool; RIP8_KEY_COUNT],
    dt: u8,
    st: u8,

    awaiting_input: bool,
    awaiter_index: usize,
    elapsed: f64,
    get_random: fn() -> u8,
}

impl Rip8 {
    pub fn from_image_at_start(image: &Vec<u8>, start_address: u16, get_random: fn() -> u8) -> Self {
        assert!(image.len() == RIP8_MEMORY_SIZE);

        Self {
            pc: start_address,
            memory: image.clone(),
            stack: Vec::with_capacity(RIP8_STACK_MAX_SIZE),
            v: [0xff; 16],
            i: 0xff,
            display: vec![0x00; RIP8_DISPLAY_SIZE],
            keyboard: [false; RIP8_KEY_COUNT],
            dt: 0x00,
            st: 0x00,

            awaiting_input: false,
            awaiter_index: 0,
            elapsed: 0.0,
            get_random,
        }
    }

    pub fn from_image(image: &Vec<u8>, get_random: fn() -> u8) -> Self {
        Self::from_image_at_start(image, RIP8_ROM_START, get_random)
    }

    pub fn from_rom_at_address(rom: &Vec<u8>, loading_address: u16, get_random: fn() -> u8) -> Self {
        assert!(loading_address >= RIP8_ROM_START);
        assert!(rom.len() <= RIP8_MEMORY_SIZE - loading_address as usize);

        let mut memory: Vec<u8> = Vec::with_capacity(RIP8_MEMORY_SIZE);

        let font_data: [u8; 0x10 * 5] = [
            0xf0, 0x90, 0x90, 0x90, 0xf0,
            0x20, 0x60, 0x20, 0x20, 0x70,
            0xf0, 0x10, 0xf0, 0x80, 0xf0,
            0xf0, 0x10, 0xf0, 0x10, 0xf0,
            0x90, 0x90, 0xf0, 0x10, 0x10,
            0xf0, 0x80, 0xf0, 0x10, 0xf0,
            0xf0, 0x80, 0xf0, 0x90, 0xf0,
            0xf0, 0x10, 0x20, 0x40, 0x40,
            0xf0, 0x90, 0xf0, 0x90, 0xf0,
            0xf0, 0x90, 0xf0, 0x10, 0xf0,
            0xf0, 0x90, 0xf0, 0x90, 0x90,
            0xe0, 0x90, 0xe0, 0x90, 0xe0,
            0xf0, 0x80, 0x80, 0x80, 0xf0,
            0xe0, 0x90, 0x90, 0x90, 0xe0,
            0xf0, 0x80, 0xf0, 0x80, 0xf0,
            0xf0, 0x80, 0xf0, 0x80, 0x80];

        // Fill reserved memory region
        for i in 0..loading_address as usize {
            if i < font_data.len() {
                memory.push(font_data[i]);
            } else {
                memory.push(0xff);
            }
        }

        // Copy rom code, pad as needed
        for i in 0..rom.len() {
            memory.push(rom[i]);
        }

        let needed = RIP8_MEMORY_SIZE - memory.len();
        for _ in 0..needed {
            memory.push(0xff);
        }

        Self::from_image_at_start(&memory, loading_address, get_random)
    }
    
    pub fn from_rom(rom: &Vec<u8>, get_random: fn() -> u8) -> Self {
        Self::from_rom_at_address(rom, RIP8_ROM_START, get_random)
    }

    pub fn set_keydown(&mut self, k: usize, v: bool) {
        if k < 0x10 {
            // Handling keydown events is a bit involved because of the fx0a
            // instruction, for more information see:
            // https://retrocomputing.stackexchange.com/a/361
            if self.keyboard[k] && !v && self.awaiting_input {
                self.awaiting_input = false;
                self.v[self.awaiter_index] = k as u8;
            }
            self.keyboard[k] = v;
        }
    }

    pub fn get_display_spot(&self, x: usize, y: usize) -> bool {
        if x < RIP8_DISPLAY_WIDTH && y < RIP8_DISPLAY_HEIGHT {
            let byte_offset = y * RIP8_DISPLAY_WIDTH / 8 + x / 8;
            let bit_offset = x % 8;
            let bit_value = (self.display[byte_offset] >> (7 - bit_offset)) & 0x01;
            bit_value != 0
        } else {
            false
        }
    }

    pub fn is_tone_on(&self) -> bool {
        self.st != 0
    }

    fn set_spot_byte(&mut self, x: usize, y: usize, val: u8) -> bool {
        let mut unset_bits = false;
        if x < RIP8_DISPLAY_WIDTH && y < RIP8_DISPLAY_HEIGHT {
            let byte_offset = y * RIP8_DISPLAY_WIDTH / 8 + x / 8;
            let bit_offset = x % 8;

            unset_bits |= (self.display[byte_offset] & val) != 0x0;
            self.display[byte_offset] ^= val.checked_shr(bit_offset as u32).unwrap_or(0);
            if x / 8 < RIP8_DISPLAY_WIDTH / 8 - 1 {
                let val = val.checked_shl(8 - bit_offset as u32).unwrap_or(0);
                unset_bits |= (self.display[byte_offset + 1] & val) != 0x0;
                self.display[byte_offset + 1] ^= val;
            }
        }
        unset_bits
    }

    pub fn step(&mut self, delta_time: f64) -> bool {
        self.elapsed += delta_time;

        // Timers count down at 60hz
        let tick_duration = 0.0166666666;
        while self.elapsed >= tick_duration {
            self.dt = self.dt.saturating_sub(1);
            self.st = self.st.saturating_sub(1);
            self.elapsed -= tick_duration;
        }

        // fetch
        if self.awaiting_input {
            return true
        }

        let ir_hb = self.memory[self.pc as usize];
        self.pc = self.pc.wrapping_add(1);
        let ir_lb = self.memory[self.pc as usize];
        self.pc = self.pc.wrapping_add(1);
        let ir: u16 = u16::from_be_bytes([ir_hb, ir_lb]);

        // decode { exec }
        let x: usize = ((ir & 0x0f00) >> 8) as usize;
        let y: usize = ((ir & 0x00f0) >> 4) as usize;
        let k: u8 = (ir & 0x00ff) as u8;
        let i: u16 = ir & 0x0fff;
        let n: u8 = (ir & 0x000f) as u8; // this should really be a nibble,
                                         // but there is no u4 in rust
        if ir & 0xffff == 0x00e0 {
            for i in 0..self.display.len() {
                self.display[i] = 0x00;
            }
        } else if ir & 0xffff == 0x00ee {
            if self.stack.len() < 2 {
                // stack underflow
                return false
            }
            self.pc = (self.stack.pop().unwrap() as u16) << 8;
            self.pc |= self.stack.pop().unwrap() as usize as u16;
        } else if ir & 0xf000 == 0x1000 {
            self.pc = i;
        } else if ir & 0xf000 == 0x2000 {
            if self.stack.len() > RIP8_STACK_MAX_SIZE - 2 {
                // stack overflow
                return false
            }
            self.stack.push(((self.pc >> 0) & 0xff) as u8);
            self.stack.push(((self.pc >> 8) & 0xff) as u8);
            self.pc = i;
        } else if ir & 0xf000 == 0x3000 {
            if self.v[x] == k {
                self.pc = self.pc.wrapping_add(2);
            }
        } else if ir & 0xf000 == 0x4000 {
            if self.v[x] != k {
                self.pc = self.pc.wrapping_add(2);
            }
        } else if ir & 0xf00f == 0x5000 {
            if self.v[x] == self.v[y] {
                self.pc = self.pc.wrapping_add(2);
            }
        } else if ir & 0xf000 == 0x6000 {
            self.v[x] = k;
        } else if ir & 0xf000 == 0x7000 {
            self.v[x] = self.v[x].wrapping_add(k);
        } else if ir & 0xf00f == 0x8000 {
            self.v[x] = self.v[y];
        } else if ir & 0xf00f == 0x8001 {
            self.v[x] |= self.v[y];
        } else if ir & 0xf00f == 0x8002 {
            self.v[x] &= self.v[y];
        } else if ir & 0xf00f == 0x8003 {
            self.v[x] ^= self.v[y];
        } else if ir & 0xf00f == 0x8004 {
            let (v, o) = self.v[x].overflowing_add(self.v[y]);
            self.v[x] = v;
            self.v[0xf] = if o { 1 } else { 0 };
        } else if ir & 0xf00f == 0x8005 {
            let (v, o) = self.v[x].overflowing_sub(self.v[y]);
            self.v[x] = v;
            self.v[0xf] = if o { 0 } else { 1 };
        } else if ir & 0xf00f == 0x8006 {
            self.v[0xf] = self.v[y] & 0x1;
            self.v[x] = self.v[y].overflowing_shr(1).0;
        } else if ir & 0xf00f == 0x8007 {
            let (v, o) = self.v[y].overflowing_sub(self.v[x]);
            self.v[x] = v;
            self.v[0xf] = if o { 0 } else { 1 };
        } else if ir & 0xf00f == 0x800e {
            self.v[0xf] = (self.v[y] & 0x80) >> 7;
            self.v[x] = self.v[y].overflowing_shl(1).0;
        } else if ir & 0xf00f == 0x9000 {
            if self.v[x] != self.v[y] {
                self.pc = self.pc.wrapping_add(2);
            }
        } else if ir & 0xf000 == 0xa000 {
            self.i = i;
        } else if ir & 0xf000 == 0xb000 {
            self.pc = i.wrapping_add(self.v[0] as u16);
        } else if ir & 0xf000 == 0xc000 {
            self.v[x] = (self.get_random)() & k;
        } else if ir & 0xf000 == 0xd000 {
            let mut unset_bits = false;
            for idx in 0..n {
                unset_bits |= self.set_spot_byte(self.v[x] as usize,
                                    (self.v[y] + idx) as usize,
                                    self.memory[self.i as usize + idx as usize]);
            }
            self.v[0xf] = if unset_bits { 1 } else { 0 }
        } else if ir & 0xf0ff == 0xe09e {
            if self.keyboard[self.v[x] as usize] {
                self.pc = self.pc.wrapping_add(2);
            }
        } else if ir & 0xf0ff == 0xe0a1 {
            if ! self.keyboard[self.v[x] as usize] {
                self.pc = self.pc.wrapping_add(2);
            }
        } else if ir & 0xf0ff == 0xf007 {
            self.v[x] = self.dt;
        } else if ir & 0xf0ff == 0xf00a {
            self.awaiting_input = true;
            self.awaiter_index = x;
        } else if ir & 0xf0ff == 0xf015 {
            self.dt = self.v[x];
        } else if ir & 0xf0ff == 0xf018 {
            self.st = self.v[x];
        } else if ir & 0xf0ff == 0xf01e {
            self.i = self.i.wrapping_add(self.v[x] as u16);
        } else if ir & 0xf0ff == 0xf029 {
            self.i = (self.v[x] & 0xf) as u16 * 5;
        } else if ir & 0xf0ff == 0xf033 {
            self.memory[self.i as usize + 0] = (self.v[x] / 100) % 10;
            self.memory[self.i as usize + 1] = (self.v[x] / 10) % 10;
            self.memory[self.i as usize + 2] = (self.v[x] / 1) % 10;
        } else if ir & 0xf0ff == 0xf055 {
            for r in 0..(x+1) {
                self.memory[self.i as usize] = self.v[r];
                self.i = self.i.wrapping_add(1);
            }
        } else if ir & 0xf0ff == 0xf065 {
            for r in 0..(x+1) {
                self.v[r] = self.memory[self.i as usize];
                self.i = self.i.wrapping_add(1);
            }
        } else {
            // could not parse instruction, halt and catch fire
            return false
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::rip8::*;
    const ALWAYS_42: fn() -> u8 = || -> u8 { 0x42 };
    const ALWAYS_ZERO: fn() -> u8 = || -> u8 { 0x00 };

   fn rip8_with_rom(rom: &Vec<u8>) -> Rip8 {
        Rip8::from_rom(rom, ALWAYS_ZERO)
    }

    fn run(rip8: &mut Rip8) {
        while rip8.step(0.0) { }
    }

    fn run_rom_with_random(rom: &Vec<u8>, random: fn() -> u8) -> Rip8 {
        let mut rip8 = Rip8::from_rom(rom, random);
        run(&mut rip8);
        rip8
    }

    fn run_rom(rom: &Vec<u8>) -> Rip8 {
        run_rom_with_random(rom, ALWAYS_ZERO)
    }

    fn append_trailing_data_to_rom(code: &mut Vec<u8>, mut trailing_data: Vec<u8>) -> u16 {
        let sprite_length = trailing_data.len();
        let sprite_address = RIP8_ROM_START + (code.len() & 0xffff) as u16 + 2;

        code.append(&mut trailing_data);

        code.insert(0, 0xa0 | (sprite_address >> 8) as u8);
        code.insert(1, (sprite_address & 0xff) as u8);

        RIP8_ROM_START + (code.len() - sprite_length) as u16
    }

    #[test]
    fn test_jp_zero() {
        let rom = vec![0x10, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, 0x0002)
    }

    #[test]
    fn test_jp_chained() {
        let rom = vec![0x12, 0x02, 0x10, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, 0x0002)
    }

    #[test]
    fn test_call_zero() {
        let rom = vec![0x20, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, 0x0002);
        assert_eq!(
            u16::from_be_bytes([
                rip8.stack[rip8.stack.len() - 2],
                rip8.stack[rip8.stack.len() - 1],]),
            0x0202);
    }

    #[test]
    fn test_ld_const() {
        let rom = vec![0x60, 0x12, 0x6c, 0x54];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x6);
        assert_eq!(rip8.v[0x0], 0x12);
        assert_eq!(rip8.v[0xc], 0x54);
    }

    #[test]
    fn test_se_const_taken() {
        let rom = vec![0x60, 0x12, 0x30, 0x12];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x12);
    }

    #[test]
    fn test_se_const_not_taken() {
        let rom = vec![0x60, 0x12, 0x30, 0x13];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x6);
        assert_eq!(rip8.v[0x0], 0x12);
    }

    #[test]
    fn test_sne_const_taken() {
        let rom = vec![0x60, 0x12, 0x40, 0x13];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x12);
    }

    #[test]
    fn test_sne_const_not_taken() {
        let rom = vec![0x60, 0x12, 0x40, 0x12];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x6);
        assert_eq!(rip8.v[0x0], 0x12);
    }

    #[test]
    fn test_se_reg_taken() {
        let rom = vec![0x60, 0x12, 0x61, 0x12, 0x50, 0x10];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0xa);
        assert_eq!(rip8.v[0x0], 0x12);
        assert_eq!(rip8.v[0x1], 0x12);
    }

    #[test]
    fn test_se_reg_not_taken() {
        let rom = vec![0x60, 0x12, 0x61, 0x13, 0x50, 0x10];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x12);
        assert_eq!(rip8.v[0x1], 0x13);
    }

    #[test]
    fn test_add_const() {
        let rom = vec![0x60, 0x12, 0x70, 0x21];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x6);
        assert_eq!(rip8.v[0x0], 0x33);
    }

    #[test]
    fn test_add_const_overflow() {
        let rom = vec![0x60, 0xff, 0x70, 0x01];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x6);
        assert_eq!(rip8.v[0x0], 0x00);
    }

    #[test]
    fn test_ld_reg() {
        let rom = vec![0x60, 0x12, 0x83, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x6);
        assert_eq!(rip8.v[0x0], 0x12);
        assert_eq!(rip8.v[0x3], 0x12);
    }

    #[test]
    fn test_or() {
        let rom = vec![0x60, 0x07, 0x61, 0xe0, 0x80, 0x11];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0xe7);
        assert_eq!(rip8.v[0x1], 0xe0);
    }

    #[test]
    fn test_and() {
        let rom = vec![0x68, 0x07, 0x6a, 0xec, 0x88, 0xa2];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x8], 0x04);
        assert_eq!(rip8.v[0xa], 0xec);
    }

    #[test]
    fn test_xor() {
        let rom = vec![0x6b, 0x1f, 0x6a, 0xf8, 0x8b, 0xa3];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0xb], 0xe7);
        assert_eq!(rip8.v[0xa], 0xf8);
    }

    #[test]
    fn test_add_flags_without_carry() {
        let rom = vec![0x64, 0x78, 0x6e, 0x32, 0x84, 0xe4];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x4], 0xaa);
        assert_eq!(rip8.v[0xe], 0x32);
        assert_eq!(rip8.v[0xf], 0);
    }

    #[test]
    fn test_add_flags_with_carry() {
        let rom = vec![0x64, 0xff, 0x6e, 0x01, 0x84, 0xe4];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x4], 0x00);
        assert_eq!(rip8.v[0xe], 0x01);
        assert_eq!(rip8.v[0xf], 1);
    }

    #[test]
    fn test_sub_flags_without_borrow() {
        let rom = vec![0x64, 0x01, 0x63, 0x01, 0x84, 0x35];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x4], 0x00);
        assert_eq!(rip8.v[0x3], 0x01);
        assert_eq!(rip8.v[0xf], 1);
    }

    #[test]
    fn test_sub_flags_with_borrow() {
        let rom = vec![0x64, 0x00, 0x63, 0x01, 0x84, 0x35];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x4], 0xff);
        assert_eq!(rip8.v[0x3], 0x01);
        assert_eq!(rip8.v[0xf], 0);
    }

    #[test]
    fn test_shr_lsb_zero() {
        let rom = vec![0x60, 0x00, 0x62, 0x02, 0x80, 0x26];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x01);
        assert_eq!(rip8.v[0x2], 0x02);
        assert_eq!(rip8.v[0xf], 0);
    }

    #[test]
    fn test_shr_lsb_set() {
        let rom = vec![0x60, 0x00, 0x62, 0x81, 0x80, 0x26];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x40);
        assert_eq!(rip8.v[0x2], 0x81);
        assert_eq!(rip8.v[0xf], 1);
    }

    #[test]
    fn test_shr_overflow() {
        let rom = vec![0x60, 0x00, 0x62, 0x01, 0x80, 0x26];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x00);
        assert_eq!(rip8.v[0x2], 0x01);
        assert_eq!(rip8.v[0xf], 1);
    }

    #[test]
    fn test_subn_without_borrow() {
        let rom = vec![0x60, 0x00, 0x61, 0x01, 0x80, 0x17];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x01);
        assert_eq!(rip8.v[0x1], 0x01);
        assert_eq!(rip8.v[0xf], 1);
    }

    #[test]
    fn test_subn_with_borrow() {
        let rom = vec![0x60, 0x02, 0x61, 0x01, 0x80, 0x17];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0xff);
        assert_eq!(rip8.v[0x1], 0x01);
        assert_eq!(rip8.v[0xf], 0);
    }

    #[test]
    fn test_shl_msb_zero() {
        let rom = vec![0x60, 0x00, 0x61, 0x08, 0x80, 0x1e];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x10);
        assert_eq!(rip8.v[0x1], 0x08);
        assert_eq!(rip8.v[0xf], 0);
    }

    #[test]
    fn test_shl_msb_set() {
        let rom = vec![0x60, 0x00, 0x61, 0x88, 0x80, 0x1e];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x10);
        assert_eq!(rip8.v[0x1], 0x88);
        assert_eq!(rip8.v[0xf], 1);
    }

    #[test]
    fn test_shl_overflow() {
        let rom = vec![0x60, 0x00, 0x61, 0x80, 0x80, 0x1e];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x00);
        assert_eq!(rip8.v[0x1], 0x80);
        assert_eq!(rip8.v[0xf], 1);
    }

    #[test]
    fn test_sne_reg_taken() {
        let rom = vec![0x60, 0x44, 0x61, 0x88, 0x90, 0x10];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0xa);
        assert_eq!(rip8.v[0x0], 0x44);
        assert_eq!(rip8.v[0x1], 0x88);
    }

    #[test]
    fn test_sne_reg_not_taken() {
        let rom = vec![0x60, 0x44, 0x61, 0x44, 0x90, 0x10];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x8);
        assert_eq!(rip8.v[0x0], 0x44);
        assert_eq!(rip8.v[0x1], 0x44);
    }

    #[test]
    fn test_ld_addr() {
        let rom = vec![0xa1, 0x23];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x4);
        assert_eq!(rip8.i, 0x0123);
    }

    #[test]
    fn test_jp_offset() {
        let rom = vec![0x60, 0x12, 0xb3, 0x21];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, 0x335);
        assert_eq!(rip8.v[0], 0x12);
    }

    #[test]
    fn test_rnd_fixed() {
        let rom = vec![0xc0, 0xff, 0xc1, 0x61];

        let rip8 = run_rom_with_random(&rom, ALWAYS_42);

        assert_eq!(rip8.pc, RIP8_ROM_START + 0x6);
        assert_eq!(rip8.v[0], 0x42);
        assert_eq!(rip8.v[1], 0x40);
    }

    #[test]
    fn test_draw_stripes() {
        let mut rom: Vec<u8> = vec![0x60, 0x00, 0xd0, 0x08, 0x00, 0x00];
        let sprite: Vec<u8> = vec![0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55];
        let stop_address = append_trailing_data_to_rom(&mut rom, sprite);

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.i, stop_address);
        assert_eq!(rip8.pc, stop_address);
        for y in 0..32 {
            for x in 0..64 {
                if x < 8 && y < 8 && x % 2 == y % 2 {
                    assert!(rip8.get_display_spot(x, y));
                } else {
                    assert!(!rip8.get_display_spot(x, y));
                }
            }
        }
        assert_eq!(rip8.v[0xf], 0);
    }

    #[test]
    fn test_draw_unset_spot() {
        let mut rom: Vec<u8> = vec![0x60, 0x00, 0xd0, 0x08, 0xd0, 0x08, 0x00, 0x00];
        let sprite: Vec<u8> = vec![0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55];
        let stop_address = append_trailing_data_to_rom(&mut rom, sprite);

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.i, stop_address);
        assert_eq!(rip8.pc, stop_address);
        for y in 0..32 {
            for x in 0..64 {
                assert!(!rip8.get_display_spot(x, y));
            }
        }
        assert_eq!(rip8.v[0xf], 1);
    }

    #[test]
    fn test_draw_stripes_offset() {
        let mut rom = vec![0x61, 0x01, 0xd1, 0x18, 0x00, 0x00];
        let sprite = vec![0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55];
        let stop_address = append_trailing_data_to_rom(&mut rom, sprite);

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.i, stop_address);
        assert_eq!(rip8.pc, stop_address);
        for y in 0..32 {
            for x in 0..64 {
                if x > 0 && x < 9 && y > 0 && y < 9 && x % 2 == y % 2 {
                    assert!(rip8.get_display_spot(x, y));
                } else {
                    assert!(!rip8.get_display_spot(x, y));
                }
            }
        }
    }

    #[test]
    fn test_draw_clipped() {
        let mut rom = vec![0x61, 0x39, 0x62, 0x19, 0xd1, 0x28, 0x00, 0x00];
        let sprite = vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        let stop_address = append_trailing_data_to_rom(&mut rom, sprite);

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.i, stop_address);
        assert_eq!(rip8.pc, stop_address);
        for y in 0..32 {
            for x in 0..64 {
                if y > 24 && x > 56 {
                    assert!(rip8.get_display_spot(x, y));
                } else {
                    assert!(!rip8.get_display_spot(x, y));
                }
            }
        }
    }

    #[test]
    fn test_skp_taken() {
        let rom = vec![0x63, 0x01, 0xe3, 0x9e, 0x00, 0x00];

        let mut rip8 = rip8_with_rom(&rom);
        rip8.set_keydown(1, true);
        run(&mut rip8);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16 + 2);
    }

    #[test]
    fn test_skp_not_taken() {
        let rom = vec![0x63, 0x01, 0xe3, 0x9e, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
    }

    #[test]
    fn test_sknp_taken() {
        let rom = vec![0x62, 0x05, 0xe2, 0xa1, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16 + 2);
    }

    #[test]
    fn test_sknp_not_taken() {
        let rom = vec![0x62, 0x00, 0xe2, 0xa1, 0x00, 0x00];

        let mut rip8 = rip8_with_rom(&rom);
        rip8.set_keydown(0, true);
        run(&mut rip8);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
    }

    #[test]
    fn test_ld_reg_dt() {
        let rom = vec![0x60, 0xff, 0xf0, 0x07, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.v[0], rip8.dt);
    }

    #[test]
    fn test_ld_input() {
        let rom = vec![0xf0, 0x0a, 0xff, 0x0a, 0x00, 0x00];

        let mut rip8 = rip8_with_rom(&rom);

        // no matter how much we run, it should stop until it receives input
        for _ in 0..50 {
            rip8.step(0.1);
        }
        rip8.set_keydown(0xf, true);
        rip8.step(0.1);
        rip8.set_keydown(0xf, false);
        for _ in 0..50 {
            rip8.step(0.1);
        }
        rip8.set_keydown(0x0, true);
        rip8.step(0.1);
        rip8.set_keydown(0x0, false);
        // finish running
        run(&mut rip8);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.v[0x0], 0xf);
        assert_eq!(rip8.v[0xf], 0x0);
    }

    #[test]
    fn test_ld_dt_reg() {
        let rom = vec![0x61, 0x42, 0xf1, 0x15, 0x00, 0x00];

        let rip8 = run_rom_with_random(&rom, ALWAYS_42);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.dt, rip8.v[0x1]);
        assert_eq!(rip8.v[0x1], 0x42);
    }

    #[test]
    fn test_ld_st_reg() {
        let rom = vec![0x61, 0x42, 0xf1, 0x18, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.st, rip8.v[0x1]);
        assert_eq!(rip8.v[0x1], 0x42);
    }

    #[test]
    fn test_add_i_reg() {
        let rom = vec![0x61, 0x32, 0xa1, 0x23, 0xf1, 0x1e, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.v[0x1], 0x32);
        assert_eq!(rip8.i, 0x155);
    }

    #[test]
    fn test_ld_sprite_0() {
        let rom = vec![0x60, 0x00, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xf0);
    }

    #[test]
    fn test_ld_sprite_1() {
        let rom = vec![0x60, 0x01, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0x20);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x60);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0x20);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x20);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0x70);
    }

    #[test]
    fn test_ld_sprite_2() {
        let rom = vec![0x60, 0x02, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x10);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x80);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xf0);
    }

    #[test]
    fn test_ld_sprite_3() {
        let rom = vec![0x60, 0x03, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x10);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x10);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xf0);
    }

    #[test]
    fn test_ld_sprite_4() {
        let rom = vec![0x60, 0x04, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x10);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0x10);
    }

    #[test]
    fn test_ld_sprite_5() {
        let rom = vec![0x60, 0x05, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x80);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x10);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xf0);
    }

    #[test]
    fn test_ld_sprite_6() {
        let rom = vec![0x60, 0x06, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x80);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xf0);
    }

    #[test]
    fn test_ld_sprite_7() {
        let rom = vec![0x60, 0x07, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x10);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0x20);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x40);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0x40);
    }

    #[test]
    fn test_ld_sprite_8() {
        let rom = vec![0x60, 0x08, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xf0);
    }

    #[test]
    fn test_ld_sprite_9() {
        let rom = vec![0x60, 0x09, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x10);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xf0);
    }

    #[test]
    fn test_ld_sprite_a() {
        let rom = vec![0x60, 0x0a, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0x90);
    }

    #[test]
    fn test_ld_sprite_b() {
        let rom = vec![0x60, 0x0b, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xe0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xe0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xe0);
    }

    #[test]
    fn test_ld_sprite_c() {
        let rom = vec![0x60, 0x0c, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x80);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0x80);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x80);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xf0);
    }

    #[test]
    fn test_ld_sprite_d() {
        let rom = vec![0x60, 0x0d, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xe0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x90);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xe0);
    }

    #[test]
    fn test_ld_sprite_e() {
        let rom = vec![0x60, 0x0e, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x80);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x80);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0xf0);
    }

    #[test]
    fn test_ld_sprite_f() {
        let rom = vec![0x60, 0x0f, 0xf0, 0x29, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.memory[rip8.i as usize + 0], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 1], 0x80);
        assert_eq!(rip8.memory[rip8.i as usize + 2], 0xf0);
        assert_eq!(rip8.memory[rip8.i as usize + 3], 0x80);
        assert_eq!(rip8.memory[rip8.i as usize + 4], 0x80);
    }

    #[test]
    fn test_ld_bcd() {
        let rom = vec![
            0x60, 0xc6, // v0 = 0xc6
            0x61, 0x4c, // v1 = 0x4c
            0x62, 0xfe, // v2 = 0xfe
            0x63, 0x03, // v3 = 0x03
            0x64, 0x03, // v4 = 0x03
            0xa6, 0x00, // i = 0x300
            0xf0, 0x33, // *i = bcd(v0) = 198
            0xf4, 0x1e, // i += 3
            0xf1, 0x33, // *i = bcd(v1) = 76
            0xf4, 0x1e, // i += 3
            0xf2, 0x33, // *i = bcd(v2) = 254
            0xf4, 0x1e, // i += 3
            0xf3, 0x33, // *i = bcd(v3) = 3
            0xf4, 0x1e, // i += 3
            0x00, 0x00
        ];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.i, 0x60c);
        assert_eq!(rip8.memory[rip8.i as usize - 01], 0x03);
        assert_eq!(rip8.memory[rip8.i as usize - 02], 0x00);
        assert_eq!(rip8.memory[rip8.i as usize - 03], 0x00);

        assert_eq!(rip8.memory[rip8.i as usize - 04], 0x04);
        assert_eq!(rip8.memory[rip8.i as usize - 05], 0x05);
        assert_eq!(rip8.memory[rip8.i as usize - 06], 0x02);

        assert_eq!(rip8.memory[rip8.i as usize - 07], 0x06);
        assert_eq!(rip8.memory[rip8.i as usize - 08], 0x07);
        assert_eq!(rip8.memory[rip8.i as usize - 09], 0x00);

        assert_eq!(rip8.memory[rip8.i as usize - 10], 0x08);
        assert_eq!(rip8.memory[rip8.i as usize - 11], 0x09);
        assert_eq!(rip8.memory[rip8.i as usize - 12], 0x01);
    }

    #[test]
    fn test_store_registers() {
        let rom = vec![
            0x60, 0xff,
            0x61, 0x88,
            0x62, 0x44,
            0x63, 0x00,
            0xa6, 0x00,
            0xf3, 0x55,
            0x00, 0x00
        ];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        assert_eq!(rip8.i, 0x600 + 3 + 1);
        assert_eq!(rip8.memory[rip8.i as usize - 01], 0x00);
        assert_eq!(rip8.memory[rip8.i as usize - 02], 0x44);
        assert_eq!(rip8.memory[rip8.i as usize - 03], 0x88);
        assert_eq!(rip8.memory[rip8.i as usize - 04], 0xff);
    }

    #[test]
    fn test_load_registers() {
        let mut rom = vec![
            0x64, 0xff,
            0xf3, 0x65,
            0x00, 0x00
        ];
        let trailer = vec![0x42, 0x43, 0x44, 0x45];
        let stop_address = append_trailing_data_to_rom(&mut rom, trailer);

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, stop_address);
        assert_eq!(rip8.i, stop_address + 4);
        assert_eq!(rip8.v[0], 0x42);
        assert_eq!(rip8.v[1], 0x43);
        assert_eq!(rip8.v[2], 0x44);
        assert_eq!(rip8.v[3], 0x45);
    }

    #[test]
    fn test_cls() {
        let rom = vec![0x00, 0xe0, 0x00, 0x00];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        for x in 0..64 {
            for y in 0..32 {
                assert!(!rip8.get_display_spot(x, y));
            }
        }
    }

    #[test]
    fn test_draw_then_cls() {
        let rom = vec![
            0x60, 0x00, // v0 = 0
            0xf0, 0x29, // i = digits[v0]
            0xd0, 0x05, // draw i..i[5] at (v0, v0)
            0x00, 0xe0, // cls
            0x00, 0x00
        ];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + rom.len() as u16);
        for x in 0..64 {
            for y in 0..32 {
                assert!(!rip8.get_display_spot(x, y));
            }
        }
    }

    #[test]
    fn test_call_ret() {
        let rom = vec![0x22, 0x04, 0x00, 0x00, 0x00, 0xee];

        let rip8 = run_rom(&rom);

        assert_eq!(rip8.pc, RIP8_ROM_START + 4);
        assert_eq!(rip8.stack.len(), 0);
    }

    #[test]
    fn test_dt_counts_down_at_60hz() {
        let rom = vec![0x60, 0xff, 0xf0, 0x15, 0x12, 0x04];

        let mut rip8 = rip8_with_rom(&rom);
        rip8.step(0.0);
        rip8.step(0.0);
        assert_eq!(rip8.dt, 0xff);
        rip8.step(1.0001);
        assert_eq!(rip8.dt, 0xc3);
    }
}

