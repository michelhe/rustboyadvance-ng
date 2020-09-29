use jni::objects::{GlobalRef, JMethodID, JObject, JValue};
use jni::signature::{JavaType, Primitive};
use jni::sys::{jlong, jmethodID};
use jni::JNIEnv;

pub struct AudioJNIConnector {
    pub audio_player_ref: GlobalRef,
    pub audio_buffer_ref: GlobalRef,

    /// jmethodID is safe to pass between threads but the jni-sys crate marked them as !Send
    /// TODO send patch to jni-sys
    mid_audio_write: jlong,
    mid_audio_play: jlong,
    mid_audio_pause: jlong,

    pub sample_rate: i32,
    pub sample_count: usize,
}

impl AudioJNIConnector {
    pub fn new(env: &JNIEnv, audio_player: JObject) -> AudioJNIConnector {
        let audio_player_ref = env.new_global_ref(audio_player).unwrap();
        let audio_player_klass = env.get_object_class(audio_player_ref.as_obj()).unwrap();

        let mid_audio_write = env
            .get_method_id(audio_player_klass, "audioWrite", "([SII)I")
            .expect("failed to get methodID for audioWrite")
            .into_inner() as jlong;
        let mid_audio_play = env
            .get_method_id(audio_player_klass, "play", "()V")
            .expect("failed to get methodID for audioPlay")
            .into_inner() as jlong;
        let mid_audio_pause = env
            .get_method_id(audio_player_klass, "pause", "()V")
            .expect("failed to get methodID for audioPause")
            .into_inner() as jlong;

        let mid_get_sample_rate = env
            .get_method_id(audio_player_klass, "getSampleRate", "()I")
            .expect("failed to get methodID for getSampleRate");
        let mid_get_sample_count = env
            .get_method_id(audio_player_klass, "getSampleCount", "()I")
            .expect("failed to get methodID for getSampleCount");

        let result = env
            .call_method_unchecked(
                audio_player_ref.as_obj(),
                mid_get_sample_count,
                JavaType::Primitive(Primitive::Int),
                &[],
            )
            .unwrap();

        let sample_count = match result {
            JValue::Int(sample_count) => sample_count as usize,
            _ => panic!("bad return value"),
        };

        let result = env
            .call_method_unchecked(
                audio_player_ref.as_obj(),
                mid_get_sample_rate,
                JavaType::Primitive(Primitive::Int),
                &[],
            )
            .unwrap();
        let sample_rate = match result {
            JValue::Int(sample_rate) => sample_rate as i32,
            _ => panic!("bad return value"),
        };

        let audio_buffer = env
            .new_short_array(sample_count as i32)
            .expect("failed to create sound buffer");
        let audio_buffer_ref = env.new_global_ref(audio_buffer).unwrap();

        // Don't need this ref anymore
        drop(audio_player_klass);

        AudioJNIConnector {
            audio_player_ref,
            audio_buffer_ref,
            mid_audio_pause,
            mid_audio_play,
            mid_audio_write,
            sample_rate,
            sample_count,
        }
    }

    #[inline]
    pub fn pause(&self, env: &JNIEnv) {
        // TODO handle errors
        let _ = env.call_method_unchecked(
            self.audio_player_ref.as_obj(),
            JMethodID::from(self.mid_audio_pause as jmethodID),
            JavaType::Primitive(Primitive::Void),
            &[],
        );
    }

    #[inline]
    pub fn play(&self, env: &JNIEnv) {
        // TODO handle errors
        let _ = env.call_method_unchecked(
            self.audio_player_ref.as_obj(),
            JMethodID::from(self.mid_audio_play as jmethodID),
            JavaType::Primitive(Primitive::Void),
            &[],
        );
    }

    #[inline]
    pub fn write_audio_samples(&self, env: &JNIEnv, samples: &[i16]) {
        // TODO handle errors
        env.set_short_array_region(self.audio_buffer_ref.as_obj().into_inner(), 0, &samples)
            .unwrap();
        let _ = env.call_method_unchecked(
            self.audio_player_ref.as_obj(),
            JMethodID::from(self.mid_audio_write as jmethodID),
            JavaType::Primitive(Primitive::Int),
            &[
                JValue::from(self.audio_buffer_ref.as_obj()),
                JValue::Int(0),                    // offset_in_shorts
                JValue::Int(samples.len() as i32), // size_in_shorts
            ],
        );
    }
}
