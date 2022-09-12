use rustboyadvance_utils::audio::{AudioRingBuffer, SampleConsumer, SampleProducer};

pub type StereoSample<T> = [T; 2];

pub trait AudioInterface {
    fn get_sample_rate(&self) -> i32 {
        44100
    }

    /// Pushes stereo sample buffer into the audio device
    /// Sample should be normilized to siged 16bit values
    /// Note: It is not guarentied that the sample will be played
    #[allow(unused_variables)]
    fn push_sample(&mut self, sample: &StereoSample<i16>) {}
}

pub struct SimpleAudioInterface {
    producer: SampleProducer,
    sample_rate: i32,
}

impl SimpleAudioInterface {
    pub fn create_channel(
        sample_rate: i32,
        buffer_size: Option<usize>,
    ) -> (Box<Self>, SampleConsumer) {
        let (producer, consumer) =
            AudioRingBuffer::new_with_capacity(buffer_size.unwrap_or(8192)).split();
        (
            Box::new(SimpleAudioInterface {
                producer,
                sample_rate,
            }),
            consumer,
        )
    }
}

impl AudioInterface for SimpleAudioInterface {
    #[inline]
    fn get_sample_rate(&self) -> i32 {
        self.sample_rate
    }

    #[inline]
    fn push_sample(&mut self, sample: &StereoSample<i16>) {
        let _ = self.producer.push(sample[0]);
        let _ = self.producer.push(sample[1]);
    }
}

pub type DynAudioInterface = Box<dyn AudioInterface>;

#[derive(Debug, Default)]
pub struct NullAudio {}

impl AudioInterface for NullAudio {}

impl NullAudio {
    pub fn new() -> Box<NullAudio> {
        Box::new(NullAudio::default())
    }
}
