/// JNI Bindings for rustboyadvance
///
use std::cell::RefCell;
use std::os::raw::c_void;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Mutex, MutexGuard};

use jni::objects::*;
use jni::sys::*;
use jni::{JNIEnv, JavaVM};

#[macro_use]
extern crate log;

#[cfg(target_os = "android")]
use android_log;
#[cfg(not(target_os = "android"))]
use env_logger;

use rustboyadvance_ng::prelude::*;

struct Hardware {
    jvm: JavaVM,
    frame_buffer_global_ref: GlobalRef,
    // frame_buffer: [u32; DISPLAY_WIDTH * DISPLAY_HEIGHT],
    key_state: u16,
}

impl VideoInterface for Hardware {
    fn render(&mut self, buffer: &[u32]) {
        let env = self.jvm.get_env().unwrap();
        unsafe {
            env.set_int_array_region(
                self.frame_buffer_global_ref.as_obj().into_inner(),
                0,
                std::mem::transmute::<&[u32], &[i32]>(buffer),
            )
            .unwrap();
        }
    }
}
impl AudioInterface for Hardware {}
impl InputInterface for Hardware {
    fn poll(&mut self) -> u16 {
        self.key_state
    }
}

struct Context {
    hwif: Rc<RefCell<Hardware>>,
    gba: GameBoyAdvance,
}

static mut DID_LOAD: bool = false;

const NATIVE_EXCEPTION_CLASS: &'static str =
    "com/mrmichel/rustboyadvance/EmulatorBindings/NativeBindingException";

unsafe fn internal_open_context(
    env: &JNIEnv,
    bios: jbyteArray,
    rom: jbyteArray,
    frame_buffer: jintArray,
    save_file: JString,
) -> Result<Context, String> {
    let bios = env
        .convert_byte_array(bios)
        .map_err(|e| format!("could not get bios buffer, error {}", e))?
        .into_boxed_slice();
    let rom = env
        .convert_byte_array(rom)
        .map_err(|e| format!("could not get rom buffer, error {}", e))?
        .into_boxed_slice();
    let save_file: String = env
        .get_string(save_file)
        .map_err(|_| String::from("could not get save path"))?
        .into();

    let gamepak = GamepakBuilder::new()
        .take_buffer(rom)
        .save_path(&Path::new(&save_file))
        .build()
        .map_err(|e| format!("failed to load rom, gba result: {:?}", e))?;

    info!("Loaded ROM file {:?}", gamepak.header);

    let frame_buffer_global_ref = env
        .new_global_ref(JObject::from(frame_buffer))
        .map_err(|e| format!("failed to add new global ref, error: {:?}", e))?;

    let hw = Hardware {
        jvm: env.get_java_vm().unwrap(),
        frame_buffer_global_ref: frame_buffer_global_ref,
        key_state: 0xffff,
    };
    let hw = Rc::new(RefCell::new(hw));

    let gba = GameBoyAdvance::new(bios, gamepak, hw.clone(), hw.clone(), hw.clone());

    debug!("creating context");
    let context = Context {
        gba: gba,
        hwif: hw.clone(),
    };

    Ok(context)
}

fn save_state(env: &JNIEnv, gba: &mut GameBoyAdvance) -> Result<jbyteArray, String> {
    let saved_state = gba
        .save_state()
        .map_err(|e| format!("failed to serielize state, error: {:?}", e))?;
    let byte_array = env
        .byte_array_from_slice(&saved_state)
        .map_err(|e| format!("failed to create byte array, error: {:?}", e))?;
    Ok(byte_array)
}

fn load_state(env: &JNIEnv, gba: &mut GameBoyAdvance, state: jbyteArray) -> Result<(), String> {
    let state = env
        .convert_byte_array(state)
        .map_err(|e| format!("failed to convert byte array, error: {:?}", e))?;
    gba.restore_state(&state)
        .map_err(|e| format!("failed to restore state, error: {:?}", e))
}

#[allow(non_snake_case)]
pub mod bindings {
    use super::*;

    unsafe fn lock_ctx<'a>(ctx: jlong) -> MutexGuard<'a, Context> {
        (*(ctx as *mut Mutex<Context>)).lock().unwrap()
    }

    #[no_mangle]
    pub unsafe extern "C" fn JNI_OnLoad(vm: *mut JavaVM, _reserved: *mut c_void) -> jint {
        if DID_LOAD {
            return JNI_VERSION_1_6;
        }
        #[cfg(target_os = "android")]
        android_log::init("EmulatorBindings").unwrap();
        #[cfg(not(target_os = "android"))]
        env_logger::init();

        debug!("library loaded and logger initialized!");
        debug!("JVM: {:?}", vm);

        DID_LOAD = true;

        JNI_VERSION_1_6
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_openEmulator(
        env: JNIEnv,
        _obj: JClass,
        bios: jbyteArray,
        rom: jbyteArray,
        frame_buffer: jintArray,
        save_file: JString,
    ) -> jlong {
        match internal_open_context(&env, bios, rom, frame_buffer, save_file) {
            Ok(ctx) => Box::into_raw(Box::new(Mutex::new(ctx))) as jlong,
            Err(msg) => {
                env.throw_new(NATIVE_EXCEPTION_CLASS, msg).unwrap();
                0
            }
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_closeEmulator(
        env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) {
        info!("destroying context {:#x}", ctx);
        // consume the wrapped content
        let _ = Box::from_raw(ctx as *mut Mutex<Context>);
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_runFrame(
        env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
        frame_buffer: jintArray,
    ) {
        let mut ctx = lock_ctx(ctx);

        ctx.gba.frame();
        // let gpu_buffer =
        //     std::mem::transmute::<&[u32], &[i32]>(&ctx.gba.get_frame_buffer() as &[u32]);
        // let result = env.set_int_array_region(frame_buffer, 0, gpu_buffer);
        // if let Err(e) = result {
        //     env.throw_new(
        //         NATIVE_EXCEPTION_CLASS,
        //         format!("failed to copy framebuffer into Java, error: {}", e),
        //     )
        //     .unwrap();
        // }
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_setKeyState(
        _env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
        key_state: jint,
    ) {
        let mut ctx = lock_ctx(ctx);
        ctx.hwif.borrow_mut().key_state = key_state as u16;
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_saveState(
        env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) -> jbyteArray {
        let mut ctx = lock_ctx(ctx);
        match save_state(&env, &mut ctx.gba) {
            Ok(result) => {
                return result;
            }
            Err(msg) => {
                env.throw_new(NATIVE_EXCEPTION_CLASS, msg).unwrap();
                return JObject::null().into_inner();
            }
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_loadState(
        env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
        state: jbyteArray,
    ) {
        let mut ctx = lock_ctx(ctx);
        match load_state(&env, &mut ctx.gba, state) {
            Ok(_) => {}
            Err(msg) => env.throw_new(NATIVE_EXCEPTION_CLASS, msg).unwrap(),
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_getGameTitle(
        env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) -> jstring {
        let ctx = lock_ctx(ctx);
        env.new_string(ctx.gba.get_game_title()).unwrap().into_inner()
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_getGameCode(
        env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) -> jstring {
        let ctx = lock_ctx(ctx);
        env.new_string(ctx.gba.get_game_code()).unwrap().into_inner()
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_log(
        _env: JNIEnv,
        _obj: JClass,
        ctx: jlong,
    ) {
        let ctx = lock_ctx(ctx);
        info!("CPU LOG: {:#x?}", ctx.gba.cpu);
    }
}
