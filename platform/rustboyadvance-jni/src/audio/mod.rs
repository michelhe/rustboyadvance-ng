pub mod connector;
pub mod thread;

pub mod util {

    use jni::objects::{JObject, JValue};
    use jni::signature::{JavaType, Primitive};
    use jni::JNIEnv;

    pub fn get_sample_rate(env: &JNIEnv, audio_player_obj: JObject) -> i32 {
        let audio_player_klass = env.get_object_class(audio_player_obj).unwrap();
        let mid_get_sample_rate = env
            .get_method_id(audio_player_klass, "getSampleRate", "()I")
            .expect("failed to get methodID for getSampleRate");
        let result = env
            .call_method_unchecked(
                audio_player_obj,
                mid_get_sample_rate,
                JavaType::Primitive(Primitive::Int),
                &[],
            )
            .unwrap();
        let sample_rate = match result {
            JValue::Int(sample_rate) => sample_rate as i32,
            _ => panic!("bad return value"),
        };
        return sample_rate;
    }
}
