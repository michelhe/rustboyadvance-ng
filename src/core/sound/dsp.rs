pub type Sample = (i16, i16);
const PI: f32 = std::f32::consts::PI;

pub trait Resampler {
    fn push_sample(&mut self, s: Sample, output: &mut Vec<i16>);
}

pub struct CosineResampler {
    last_in_sample: Sample,
    phase: f32,
    pub in_freq: f32,
    out_freq: f32,
}

fn cosine_interpolation(y1: Sample, y2: Sample, phase: f32) -> Sample {
    let y1_left = y1.0 as f32;
    let y1_right = y1.1 as f32;
    let y2_left = y2.0 as f32;
    let y2_right = y2.1 as f32;

    let mu2 = (1.0 - (PI * phase).cos()) / 2.0;

    (
        (y2_left * (1.0 - mu2) + y1_left * mu2) as i16,
        (y2_right * (1.0 - mu2) + y1_right * mu2) as i16,
    )
}

impl Resampler for CosineResampler {
    fn push_sample(&mut self, s: Sample, output: &mut Vec<i16>) {
        while self.phase < 1.0 {
            let x = cosine_interpolation(self.last_in_sample, s, self.phase);
            output.push(x.0);
            output.push(x.1);
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
