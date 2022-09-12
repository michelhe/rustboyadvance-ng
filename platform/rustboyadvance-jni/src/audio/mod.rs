pub mod connector;
pub mod thread;

pub mod util {

    use jni::objects::{JObject, JValue};
    use jni::signature::{JavaType, Primitive};
    use jni::JNIEnv;

    macro_rules! call_audio_player_method {
        ($env:ident, $audio_player_obj:ident, $method_name:literal, "()I") => {{
            let audio_player_klass = $env
                .get_object_class($audio_player_obj)
                .map_err(|e| format!("failed to get class: {:?}", e))?;
            let mid_get_sample_rate = $env
                .get_method_id(audio_player_klass, $method_name, "()I")
                .map_err(|e| format!("failed to get methodID for {}: {:?}", $method_name, e))?;
            let result = $env
                .call_method_unchecked(
                    $audio_player_obj,
                    mid_get_sample_rate,
                    JavaType::Primitive(Primitive::Int),
                    &[],
                )
                .map_err(|e| format!("getSampleRate() failed: {:?}", e))?;
            match result {
                JValue::Int(sample_rate) => Ok(sample_rate),
                value => panic!("bad return value {:?}", value),
            }
        }};
    }

    pub fn get_sample_rate(env: &JNIEnv, audio_player_obj: JObject) -> Result<i32, String> {
        call_audio_player_method!(env, audio_player_obj, "getSampleRate", "()I")
    }

    pub fn get_sample_count(env: &JNIEnv, audio_player_obj: JObject) -> Result<i32, String> {
        call_audio_player_method!(env, audio_player_obj, "getSampleCount", "()I")
    }
}
