use std::thread;

extern crate cpal;

use cpal::traits::{DeviceTrait, EventLoopTrait, HostTrait};
use cpal::EventLoop;

pub struct CpalSoundBackend {
    event_loop: EventLoop,
    sample_rate: usize,
    channels: usize,
}

impl CpalSoundBackend {
    pub fn new() -> CpalSoundBackend {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("failed to find a default output device");
        let format = device.default_output_format()?;
        let event_loop = host.event_loop();
        let stream_id = event_loop.build_output_stream(&device, &format)?;

        event_loop.play_stream(stream_id.clone())?;

        CpalSoundBackend {
            event_loop: event_loop,
            sample_rate: format.sample_rate.0,
            channels: format.channels,
        }
    }

    pub fn start(&self) {
        thread::spawn(move || {
            self.event_loop.run(move |id, result| {
                let data = match result {
                    Ok(data) => data,
                    Err(err) => {
                        println!("an error occurred on stream {:?}: {}", id, err);
                        return;
                    }
                };

                match data {
                    cpal::StreamData::Output { buffer: cpal::UnknownTypeOutputBuffer::F32(mut buffer) } => {
                        for sample in buffer.chunks_mut(format.channels as usize) {
                            // TODO get samples from SoundController
                            sample[0] = 0.0;
                            sample[1] = 0.0;
                        }
                    },
                    _ => {
                        panic!("expected F32");
                    },
                }
            });
        });
    }
}
