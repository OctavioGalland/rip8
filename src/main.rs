extern crate sdl2;

use std::fs;

use sdl2::pixels::Color;
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Scancode};
use sdl2::rect::Rect;

use clap::Parser;

pub mod rip8;
pub mod buzzer;

use rip8::*;
use buzzer::*;

const SCANCODE_MAPPING: [Scancode; RIP8_KEY_COUNT] = [
    Scancode::X,
    Scancode::Num1,Scancode::Num2,Scancode::Num3,
    Scancode::Q,Scancode::W,Scancode::E,
    Scancode::A,Scancode::S,Scancode::D,
    Scancode::Z,Scancode::C,
    Scancode::Num4,Scancode::R,Scancode::F,Scancode::V
];

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg()]
    file: String,

    #[arg(short='i', long="image", default_value_t=false, help="Load FILE as a complete Rip8 image (must be 4096 bytes)")]
    is_image: bool,

    #[arg(short, long, default_value_t=540)]
    freq: u32,

    #[arg(short, long, default_value_t=0x200, help="Loading/start address")]
    address: u16,

    #[arg(long, default_value_t=800, help="Window width")]
    width: u32,

    #[arg(long, default_value_t=400, help="Window height")]
    height: u32,

    #[arg(short, default_value_t=false, help="S-CHIP semantics (affects shift, load/store instructions)")]
    s_chip: bool,
}

fn main() {
    let args = Args::parse();

    if args.width != args.height * 2 {
        println!("Running in an aspect ratio other than 2:1, display may look stretched!");
    }

    // Load rom, create VM and init timers
    let rom = match fs::read(&args.file) {
        Ok(bytes) => bytes,
        Err(_) => {
            println!("Could not open file {}, aborting!", args.file);
            std::process::exit(-1);
        }
    };

    let frequency = args.freq;

    let mut rip8 = (if args.is_image {
        Rip8::from_image_at_start
    } else {
        Rip8::from_rom_at_address
    })(&rom, frequency, args.address, || -> u8{ rand::random::<u8>() });

    rip8.set_s_chip_mode(args.s_chip);

    // Init SDL2, get a window and a buzzer
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();

    let window = video_subsystem.window("Rip8", args.width, args.height)
        .position_centered()
        .build()
        .unwrap();

    let idx = window.display_index().unwrap();
    let refresh_rate = video_subsystem.current_display_mode(idx).unwrap().refresh_rate as u32;

    let mut canvas = window.into_canvas().present_vsync().accelerated().build().unwrap();
    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    canvas.present();


    let mut event_pump = sdl_context.event_pump().unwrap();

    let buzzer = Buzzer::from_sdl_context(&sdl_context);

    // Main loop
    let mut running = true;
    let cycles_per_frame: f32 = args.freq as f32 / refresh_rate as f32;
    let mut cycles_due: f32 = 0.0;
    while running {
        // Clear screen and handle exit event
        canvas.set_draw_color(Color::RGB(0, 0, 0));
        canvas.clear();
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} |
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    running = false
                },
                _ => {}
            }
        }

        // Process input
        let keyboard_state = event_pump.keyboard_state();
        for k in 0..SCANCODE_MAPPING.len() {
            rip8.set_keydown(k, keyboard_state.is_scancode_pressed(SCANCODE_MAPPING[k]));
        }

        // Calculate delta since last step
        cycles_due += cycles_per_frame;
        let whole_cycles_due = cycles_due as u32;
        for _ in 0..whole_cycles_due {
            running &= rip8.step(1);
            cycles_due -= 1.0;
        }

        // Turn buzzer on/off & present screen
        if rip8.is_tone_on() && !buzzer.is_on() {
            buzzer.start();
        } else if !rip8.is_tone_on() && buzzer.is_on() {
            buzzer.stop();
        }

        for x in 0..RIP8_DISPLAY_WIDTH {
            for y in 0..RIP8_DISPLAY_HEIGHT {
                if rip8.get_display_spot(x, y) {
                    canvas.set_draw_color(Color::GREEN);
                } else {
                    canvas.set_draw_color(Color::BLACK);
                }
                let spot_width: u32 = args.width / RIP8_DISPLAY_WIDTH as u32;
                let spot_height: u32 = args.height / RIP8_DISPLAY_HEIGHT as u32;
                let spot = Rect::new(
                    x as i32 * spot_width as i32, y as i32 * spot_height as i32,
                    spot_width, spot_height);
                let _ = canvas.fill_rect(spot);
            }
        }

        canvas.present();
    }
}
