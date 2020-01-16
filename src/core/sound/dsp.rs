use crate::{AudioInterface, StereoSample};

use serde::{Deserialize, Serialize};

const PI: f32 = std::f32::consts::PI;

pub trait Resampler {
    fn push_sample(&mut self, s: StereoSample, audio: &mut dyn AudioInterface);
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CosineResampler {
    last_in_sample: StereoSample,
    phase: f32,
    pub in_freq: f32,
    out_freq: f32,
}

fn cosine_interpolation(y1: i16, y2: i16, phase: f32) -> i16 {
    let y1 = y1 as i32 as f32;
    let y2 = y2 as i32 as f32;

    let mu2 = (1.0 - (PI * phase).cos()) / 2.0;

    (y2 * (1.0 - mu2) + y1 * mu2) as i16
}

impl Resampler for CosineResampler {
    fn push_sample(&mut self, s: StereoSample, audio: &mut dyn AudioInterface) {
        while self.phase < 1.0 {
            let left = cosine_interpolation(self.last_in_sample.0, s.0, self.phase);
            let right = cosine_interpolation(self.last_in_sample.1, s.1, self.phase);
            audio.push_sample((left, right));
            self.phase += self.in_freq / self.out_freq;
        }
        self.phase = self.phase - 1.0;
        self.last_in_sample = s;
    }
}

impl CosineResampler {
    pub fn new(in_freq: f32, out_freq: f32) -> CosineResampler {
        CosineResampler {
            last_in_sample: Default::default(),
            phase: 0.0,
            in_freq: in_freq,
            out_freq: out_freq,
        }
    }
}
