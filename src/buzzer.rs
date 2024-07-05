use sdl2::Sdl;
use sdl2::audio::{AudioCallback, AudioSpecDesired, AudioDevice, AudioStatus};

pub struct Buzzer {
    device: AudioDevice<SquareWave>,
}

impl Buzzer {
    pub fn from_sdl_context(sdl_context: &Sdl) -> Self {
        let audio_subsystem = sdl_context.audio().unwrap();
    
        let desired_spec = AudioSpecDesired {
            freq: Some(44100),
            channels: Some(1),  // mono
            samples: None       // default sample size
        };
        
        let device = audio_subsystem.open_playback(None, &desired_spec, |spec| {
            // initialize the audio callback
            SquareWave {
                phase_inc: 440.0 / spec.freq as f32,
                phase: 0.0,
                volume: 0.25
            }
        }).unwrap();

        Buzzer { device }
    }

    pub fn is_on(&self) -> bool {
        self.device.status() == AudioStatus::Playing
    }

    pub fn start(&self) {
        self.device.resume();
    }

    pub fn stop(&self) {
        self.device.pause();
    }
}

pub struct SquareWave {
    phase_inc: f32,
    phase: f32,
    volume: f32
}

impl AudioCallback for SquareWave {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        // Generate a square wave
        for x in out.iter_mut() {
            *x = if self.phase <= 0.5 {
                self.volume
            } else {
                -self.volume
            };
            self.phase = (self.phase + self.phase_inc) % 1.0;
        }
    }
}

