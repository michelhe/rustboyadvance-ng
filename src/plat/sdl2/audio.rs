use sdl2;
use sdl2::audio::{AudioDevice, AudioSpec, AudioSpecDesired};

use rustboyadvance_ng::AudioInterface;

pub struct Sdl2AudioPlayer {
    device: AudioDevice<GbaAudioCallback>,
    freq: u32,
}

impl AudioInterface for Sdl2AudioPlayer {
    fn get_sample_rate(&self) -> u32 {
        self.freq
    }
}

struct GbaAudioCallback {
    spec: AudioSpec,
}

impl sdl2::audio::AudioCallback for GbaAudioCallback {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        // TODO audio
        for x in out.iter_mut() {
            *x = 0.0;
        }
    }
}

pub fn create_audio_player(sdl: &sdl2::Sdl) -> Sdl2AudioPlayer {
    let audio_subsystem = sdl.audio().unwrap();

    let desired_spec = AudioSpecDesired {
        freq: Some(44100),
        channels: Some(1), // stereo
        samples: None,
    };

    let mut device = audio_subsystem
        .open_playback(None, &desired_spec, |spec| {
            println!("Obtained {:?}", spec);

            GbaAudioCallback { spec: spec }
        })
        .unwrap();

    let freq = (*device.lock()).spec.freq as u32;
    Sdl2AudioPlayer { device, freq }
}
