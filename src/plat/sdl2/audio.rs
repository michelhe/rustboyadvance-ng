use sdl2;
use sdl2::audio::{AudioDevice, AudioQueue, AudioSpec, AudioSpecDesired, AudioStatus};

use rustboyadvance_ng::AudioInterface;

pub struct Sdl2AudioPlayer {
    device: AudioQueue<i16>,
    freq: i32,
}

impl AudioInterface for Sdl2AudioPlayer {
    fn get_sample_rate(&self) -> i32 {
        self.freq
    }

    fn play(&mut self, samples: &[i16]) {
        self.device.queue(&samples);
    }
}

pub fn create_audio_player(sdl: &sdl2::Sdl) -> Sdl2AudioPlayer {
    let audio_subsystem = sdl.audio().unwrap();

    let desired_spec = AudioSpecDesired {
        freq: Some(44_100),
        channels: Some(2), // stereo
        samples: None,
    };

    // let mut device = audio_subsystem
    //     .open_playback(None, &desired_spec, |spec| {
    //         println!("Obtained {:?}", spec);

    //         GbaAudioCallback { spec: spec }
    //     })
    //     .unwrap();

    let mut device = audio_subsystem
        .open_queue::<i16, _>(None, &desired_spec)
        .unwrap();

    println!("Found audio device: {:?}", device.spec());

    let freq = device.spec().freq;
    device.resume();
    Sdl2AudioPlayer { device, freq }
}
