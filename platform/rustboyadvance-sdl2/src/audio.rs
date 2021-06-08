use sdl2;
use sdl2::audio::{AudioCallback, AudioDevice, AudioSpec, AudioSpecDesired};

use rustboyadvance_core::{AudioInterface, StereoSample};

use ringbuf;
use ringbuf::{Consumer, Producer, RingBuffer};

struct GbaAudioCallback {
    consumer: Consumer<StereoSample<i16>>,
    spec: AudioSpec,
}

pub struct DummyAudioPlayer {}

impl AudioInterface for DummyAudioPlayer {}

pub struct Sdl2AudioPlayer {
    _device: AudioDevice<GbaAudioCallback>,
    producer: Producer<StereoSample<i16>>,
    freq: i32,
}

impl AudioCallback for GbaAudioCallback {
    type Channel = i16;

    fn callback(&mut self, out_samples: &mut [i16]) {
        let sample_count = out_samples.len() / 2;

        for i in 0..sample_count {
            if let Some((left, right)) = self.consumer.pop() {
                out_samples[2 * i] = left;
                out_samples[2 * i + 1] = right;
            } else {
                out_samples[2 * i] = self.spec.silence as i16;
                out_samples[2 * i + 1] = self.spec.silence as i16;
            }
        }
    }
}

impl AudioInterface for Sdl2AudioPlayer {
    fn get_sample_rate(&self) -> i32 {
        self.freq
    }

    fn push_sample(&mut self, sample: &[i16]) {
        #![allow(unused_must_use)]
        self.producer.push((sample[0], sample[1]));
    }
}

pub fn create_audio_player(sdl: &sdl2::Sdl) -> Sdl2AudioPlayer {
    let desired_spec = AudioSpecDesired {
        freq: Some(44_100),
        channels: Some(2), // stereo
        samples: None,
    };

    let audio_subsystem = sdl.audio().unwrap();

    let mut freq = 0;

    let mut producer: Option<Producer<StereoSample<i16>>> = None;

    let device = audio_subsystem
        .open_playback(None, &desired_spec, |spec| {
            info!("Found audio device: {:?}", spec);
            freq = spec.freq;

            // Create a thread-safe SPSC fifo
            let ringbuf_size = (spec.samples as usize) * 2;
            let rb = RingBuffer::<StereoSample<i16>>::new(ringbuf_size);
            let (prod, cons) = rb.split();

            // move producer to the outer scope
            producer = Some(prod);

            GbaAudioCallback {
                consumer: cons,
                spec,
            }
        })
        .unwrap();

    device.resume();

    Sdl2AudioPlayer {
        _device: device,
        freq,
        producer: producer.unwrap(),
    }
}

pub fn create_dummy_player() -> DummyAudioPlayer {
    info!("Dummy audio device");
    DummyAudioPlayer {}
}
