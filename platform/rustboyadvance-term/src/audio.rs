use rustboyadvance_core::prelude::SimpleAudioInterface;

use rustboyadvance_utils::Consumer;
use tinyaudio::prelude::*;

pub fn create_audio_player() -> (Box<SimpleAudioInterface>, OutputDevice) {
    let desired_spec = OutputDeviceParameters {
        sample_rate: 44_100,
        channels_count: 2, // stereo
        channel_sample_count: 44_100 / 30,
    };

    let freq = desired_spec.sample_rate as i32;
    let ringbuf_size = desired_spec.channel_sample_count * 2;

    let (audio_device, mut consumer) =
        SimpleAudioInterface::create_channel(freq, Some(ringbuf_size));

    let r = run_output_device(desired_spec, move |data| {
        'outer: for samples in data.chunks_mut(desired_spec.channels_count) {
            for sample in samples {
                if let Some(v) = consumer.try_pop() {
                    *sample = v as f32 / i16::MAX as f32;
                } else {
                    break 'outer;
                }
            }
        }
    })
    .unwrap();

    (audio_device, r)
}
