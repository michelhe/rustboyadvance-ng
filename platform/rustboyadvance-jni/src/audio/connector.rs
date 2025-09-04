use jni::objects::{GlobalRef, JMethodID, JObject, JShortArray, JValue, JValueGen};
use jni::signature::{Primitive, ReturnType};
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

    pub _sample_rate: i32,
    pub _sample_count: usize,
}

impl AudioJNIConnector {
    pub fn new(env: &mut JNIEnv, audio_player: &JObject) -> AudioJNIConnector {
        let audio_player_ref = env.new_global_ref(audio_player).unwrap();
        let audio_player_klass = env.get_object_class(audio_player_ref.as_obj()).unwrap();

        let mid_audio_write = env
            .get_method_id(&audio_player_klass, "audioWrite", "([SII)I")
            .expect("failed to get methodID for audioWrite")
            .into_raw() as jlong;
        let mid_audio_play = env
            .get_method_id(&audio_player_klass, "play", "()V")
            .expect("failed to get methodID for audioPlay")
            .into_raw() as jlong;
        let mid_audio_pause = env
            .get_method_id(&audio_player_klass, "pause", "()V")
            .expect("failed to get methodID for audioPause")
            .into_raw() as jlong;

        let mid_get_sample_rate = env
            .get_method_id(&audio_player_klass, "getSampleRate", "()I")
            .expect("failed to get methodID for getSampleRate");
        let mid_get_sample_count = env
            .get_method_id(&audio_player_klass, "getSampleCount", "()I")
            .expect("failed to get methodID for getSampleCount");

        let result = unsafe { env
            .call_method_unchecked(
                audio_player_ref.as_obj(),
                mid_get_sample_count,
                ReturnType::Primitive(Primitive::Int),
                &[],
            )
            .unwrap() };

        
        let sample_count = match result.i() {
            Ok(sample_count) => sample_count as usize,
            _ => panic!("bad return value"),
        };

        let result = unsafe { env
            .call_method_unchecked(
                audio_player_ref.as_obj(),
                mid_get_sample_rate,
                ReturnType::Primitive(Primitive::Int),
                &[],
            )
            .unwrap() };
        let sample_rate = match result.i() {
            Ok(sample_rate) => sample_rate as i32,
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
            _sample_rate: sample_rate,
            _sample_count: sample_count,
        }
    }

    #[inline]
    pub fn pause(&self, env: &mut JNIEnv) {
        let mid_audio_pause = unsafe {
            JMethodID::from_raw(self.mid_audio_pause as jmethodID)
        };
        // TODO handle errors
        let _ = unsafe { env.call_method_unchecked(
            self.audio_player_ref.as_obj(),
            JMethodID::from(mid_audio_pause),
            ReturnType::Primitive(Primitive::Void),
            &[],
        ) };
    }

    #[inline]
    pub fn play(&self, env: &mut JNIEnv) {
        // TODO handle errors
        let _ = unsafe { env.call_method_unchecked(
            self.audio_player_ref.as_obj(),
            JMethodID::from_raw(self.mid_audio_play as jmethodID),
            ReturnType::Primitive(Primitive::Void),
            &[],
        ) };
    }

    #[inline]
    pub fn write_audio_samples(&self, env: &mut JNIEnv, samples: &[i16]) {
        // TODO handle errors
        let arr: &JShortArray = self.audio_buffer_ref.as_obj().into();
        env.set_short_array_region( arr, 0, &samples)
            .unwrap();
        let _ = unsafe { env.call_method_unchecked(
            self.audio_player_ref.as_obj(),
            JMethodID::from_raw(self.mid_audio_write as jmethodID),
            ReturnType::Primitive(Primitive::Int),
            &[
                JValueGen::Object(self.audio_buffer_ref.as_obj()).as_jni(),
                JValue::Int(0).as_jni(),                    // offset_in_shorts
                JValue::Int(samples.len() as i32).as_jni(), // size_in_shorts
            ],
        ) };
    }
}
