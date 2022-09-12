use sdl2;
use sdl2::audio::{AudioCallback, AudioDevice, AudioFormat, AudioSpec, AudioSpecDesired};

use rustboyadvance_core::prelude::SimpleAudioInterface;
use rustboyadvance_utils::audio::SampleConsumer;

pub struct GbaAudioCallback {
    consumer: SampleConsumer,
    #[allow(unused)]
    spec: AudioSpec,
}

impl AudioCallback for GbaAudioCallback {
    type Channel = i16;

    fn callback(&mut self, out_samples: &mut [i16]) {
        let written = self.consumer.pop_slice(out_samples);
        for s in out_samples.iter_mut().skip(written) {
            *s = self.spec.silence as i16;
        }
    }
}

pub fn create_audio_player(
    sdl: &sdl2::Sdl,
) -> Result<(Box<SimpleAudioInterface>, AudioDevice<GbaAudioCallback>), String> {
    let desired_spec = AudioSpecDesired {
        freq: Some(44_100),
        channels: Some(2), // stereo
        samples: None,
    };

    let audio_subsystem = sdl.audio()?;

    let mut freq = 0;

    let mut gba_audio = None;

    let device = audio_subsystem.open_playback(None, &desired_spec, |spec| {
        info!("Found audio device: {:?}", spec);
        freq = spec.freq;

        if spec.format != AudioFormat::S16LSB {
            panic!("Unsupported audio format {:?}", spec.format);
        }

        // Create a thread-safe SPSC fifo
        let ringbuf_samples_per_channel = (spec.samples as usize) * 2; // we want the ringbuf to hold 2 frames worth of samples
        let ringbuf_size = (spec.channels as usize) * ringbuf_samples_per_channel;
        info!("ringbuffer size = {}", ringbuf_size);

        let (audio_device, consumer) =
            SimpleAudioInterface::create_channel(freq, Some(ringbuf_size));
        // Move the audio to outer scope
        gba_audio = Some(audio_device);

        GbaAudioCallback { consumer, spec }
    })?;

    device.resume();

    Ok((gba_audio.take().unwrap(), device))
}
