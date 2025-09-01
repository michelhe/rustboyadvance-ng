mod audio;
mod emulator;
/// JNI Bindings for rustboyadvance
///
mod rom_helper;

use emulator::EmulatorContext;

use std::os::raw::c_void;

use jni::objects::*;
use jni::sys::*;
use jni::{JNIEnv, JavaVM};

#[macro_use]
extern crate log;

#[cfg(target_os = "android")]
use android_log;
#[cfg(not(target_os = "android"))]
use env_logger;

use rustboyadvance_core::prelude::*;

static mut DID_LOAD: bool = false;

const NATIVE_EXCEPTION_CLASS: &'static str =
    "com/mrmichel/rustboyadvance/EmulatorBindings/NativeBindingException";

fn save_state(env: &JNIEnv, gba: &mut GameBoyAdvance) -> Result<jbyteArray, String> {
    let saved_state = gba
        .save_state()
        .map_err(|e| format!("failed to serielize state, error: {:?}", e))?;
    let byte_array = env
        .byte_array_from_slice(&saved_state)
        .map_err(|e| format!("failed to create byte array, error: {:?}", e))?;
    Ok(**byte_array)
}

fn load_state(env: &JNIEnv, gba: &mut GameBoyAdvance, state: jbyteArray) -> Result<(), String> {
    let state = unsafe {
        JByteArray::from_raw(state)
    };
    let state = env
        .convert_byte_array(state)
        .map_err(|e| format!("failed to convert byte array, error: {:?}", e))?;
    gba.restore_state(&state)
        .map_err(|e| format!("failed to restore state, error: {:?}", e))
}

#[allow(non_snake_case)]
pub mod bindings {
    use super::*;

    #[inline(always)]
    unsafe fn cast_ctx<'a>(ctx: jlong) -> &'a mut EmulatorContext {
        unsafe { &mut (*(ctx as *mut EmulatorContext)) }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn JNI_OnLoad(vm: *mut JavaVM, _reserved: *mut c_void) -> jint {
        if unsafe { DID_LOAD } {
            return JNI_VERSION_1_6;
        }
        #[cfg(target_os = "android")]
        android_log::init("EmulatorBindings").unwrap();
        #[cfg(not(target_os = "android"))]
        env_logger::init();

        debug!("library loaded and logger initialized!");
        debug!("JVM: {:?}", vm);

        unsafe { DID_LOAD = true };

        JNI_VERSION_1_6
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_openEmulator(
        mut env: JNIEnv,
        _obj: JClass,
        bios: jbyteArray,
        rom: jbyteArray,
        renderer_obj: JObject,
        audio_player_obj: JObject,
        keypad_obj: JObject,
        save_file: JString,
        skip_bios: jboolean,
    ) -> jlong {
        match EmulatorContext::native_open_context(
            &mut env,
            bios,
            rom,
            renderer_obj,
            audio_player_obj,
            keypad_obj,
            save_file,
            skip_bios,
        ) {
            Ok(ctx) => Box::into_raw(Box::new(ctx)) as jlong,
            Err(msg) => {
                env.throw_new(NATIVE_EXCEPTION_CLASS, msg).unwrap();
                -1
            }
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_openSavedState(
        mut env: JNIEnv,
        _obj: JClass,
        bios: jbyteArray,
        rom: jbyteArray,
        savestate: jbyteArray,
        renderer_obj: JObject,
        audio_player_obj: JObject,
        keypad_obj: JObject,
    ) -> jlong {
        match EmulatorContext::native_open_saved_state(
            &mut env,
            bios,
            rom,
            savestate,
            renderer_obj,
            audio_player_obj,
            keypad_obj,
        ) {
            Ok(ctx) => Box::into_raw(Box::new(ctx)) as jlong,
            Err(msg) => {
                env.throw_new(NATIVE_EXCEPTION_CLASS, msg).unwrap();
                -1
            }
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_closeEmulator(
        _env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) {
        info!("waiting for emulation thread to stop");

        {
            let ctx = unsafe { cast_ctx(ctx) };
            ctx.request_stop();
            while !ctx.is_stopped() {}
        }

        info!("destroying context {:#x}", ctx);
        // consume the wrapped content
        let _ = unsafe { Box::from_raw(ctx as *mut EmulatorContext) };
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_runMainLoop(
        mut env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) {
        let ctx = unsafe { cast_ctx(ctx) };
        match ctx.native_run(&mut env) {
            Ok(_) => {}
            Err(err) => {
                env.throw_new(NATIVE_EXCEPTION_CLASS, format!("Error: {:?}", err))
                    .unwrap();
            }
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_pause(
        _env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) {
        let ctx = unsafe { cast_ctx(ctx) };
        ctx.pause();
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_resume(
        _env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) {
        let ctx = unsafe { cast_ctx(ctx) };
        ctx.resume();
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_setTurbo(
        _env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
        turbo: jboolean,
    ) {
        info!("setTurbo called!");
        let ctx = unsafe { cast_ctx(ctx) };
        ctx.set_turbo(turbo != 0);
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_stop(
        _env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) {
        let ctx = unsafe { cast_ctx(ctx) };
        ctx.request_stop();
        while !ctx.is_stopped() {}
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_getFrameBuffer(
        env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) -> jintArray {
        let ctx = unsafe { cast_ctx(ctx) };
        ctx.native_get_framebuffer(&env)
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_saveState(
        mut env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) -> jbyteArray {
        let ctx = unsafe { cast_ctx(ctx) };
        ctx.pause();
        let (_lock, gba) = ctx.lock_and_get_gba();
        match save_state(&env, gba) {
            Ok(result) => {
                drop(_lock);
                ctx.resume();
                return result;
            }
            Err(msg) => {
                env.throw_new(NATIVE_EXCEPTION_CLASS, msg).unwrap();
                return JObject::null().into_raw();
            }
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_loadState(
        mut env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
        state: jbyteArray,
    ) {
        let ctx = unsafe { cast_ctx(ctx) };
        ctx.pause();
        let (_lock, gba) = ctx.lock_and_get_gba();
        match load_state(&env, gba, state) {
            Ok(_) => {
                drop(_lock);
                ctx.resume();
            }
            Err(msg) => env.throw_new(NATIVE_EXCEPTION_CLASS, msg).unwrap(),
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_getGameTitle(
        env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) -> jstring {
        let ctx = unsafe { cast_ctx(ctx) };
        env.new_string(ctx.gba.get_game_title())
            .unwrap()
            .into_raw()
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_getGameCode(
        env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) -> jstring {
        let ctx = unsafe { cast_ctx(ctx) };
        env.new_string(ctx.gba.get_game_code())
            .unwrap()
            .into_raw()
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_log(
        _env: JNIEnv,
        _obj: JClass,
        _ctx: jlong,
    ) {
        info!("unimplemented")
    }
}
