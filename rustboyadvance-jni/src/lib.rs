/// JNI Bindings to rustboyadvance
/// For use with the following example java class
///
/// package com.mrmichel.rustboyadvance;
////
/// public class EmulatorBindings {
///
///     public static native int openEmulator(String biosPath, String romPath, boolean skipBiosAnimation);
///
///     public static native void closeEmulator();
///
///     public static native int runFrame(int[] frame_buffer);
///
///     public static native int log();
///
///     static {
///         System.loadLibrary("rustboyadvance_jni");
///     }
/// }
///
use std::cell::RefCell;
use std::os::raw::c_void;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use jni;

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jintArray, JNI_VERSION_1_6};
use jni::{JNIEnv, JavaVM};

#[macro_use]
extern crate log;

#[cfg(target_os = "android")]
use android_log;
#[cfg(not(target_os = "android"))]
use env_logger;

use rustboyadvance_ng::prelude::*;

struct Hardware {
    // frame_buffer: [u32; DISPLAY_WIDTH * DISPLAY_HEIGHT],
    key_state: u16,
}

impl VideoInterface for Hardware {}
impl AudioInterface for Hardware {}
impl InputInterface for Hardware {
    fn poll(&mut self) -> u16 {
        self.key_state
    }
}

struct Emulator {
    hwif: Rc<RefCell<Hardware>>,
    gba: GameBoyAdvance,
}

static mut JVM_PTR: Option<Arc<Mutex<*mut JavaVM>>> = None;
static mut EMULATOR: Option<Arc<Mutex<Emulator>>> = None;
static mut DID_LOAD: bool = false;

macro_rules! get_static_global {
    ($GLBL:ident: &mut $v:ident => $ok:block else $err:block) => {
        if let Some(lock) = &mut $GLBL {
            let mut $v = lock.lock().unwrap();

            $ok
        } else {
            error!("{} not initialized", stringify!($GLBL));
            $err
        }
    };
    ($GLBL:ident: &$v:ident => $ok:block else $err:block) => {
        if let Some(lock) = &mut $GLBL {
            let $v = lock.lock().unwrap();

            $ok
        } else {
            error!("{} not initialized", stringify!($GLBL));
            $err
        }
    };
}

#[allow(non_snake_case)]
pub mod bindings {
    use super::*;

    use std::path::Path;

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

        // save JVM_PTR
        JVM_PTR = Some(Arc::new(Mutex::new(vm)));

        DID_LOAD = true;

        JNI_VERSION_1_6
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_openEmulator(
        env: JNIEnv,
        _: JClass,
        bios_path: JString,
        rom_path: JString,
        skip_bios: jboolean,
    ) -> jint {
        let bios_path: String = env
            .get_string(bios_path)
            .expect("invalid bios path object")
            .into();

        let bios_rom = read_bin_file(&Path::new(&bios_path)).expect("failed to load bios file");

        let rom_path: String = env
            .get_string(rom_path)
            .expect("invalid rom path object")
            .into();

        debug!("trying to load {}", rom_path);

        let gamepak = match GamepakBuilder::new().file(&Path::new(&rom_path)).build() {
            Ok(gamepak) => gamepak,
            Err(err) => {
                error!("failed to load rom, error: {:?}", err);
                return -1;
            }
        };

        info!("Loaded ROM file {:?}", gamepak.header);

        let hw = Hardware { key_state: 0xffff };
        let hw = Rc::new(RefCell::new(hw));

        let mut gba = GameBoyAdvance::new(bios_rom.into_boxed_slice(), gamepak, hw.clone(), hw.clone(), hw.clone());
        if skip_bios != 0 {
            gba.skip_bios();
        }

        EMULATOR = Some(Arc::new(Mutex::new(Emulator {
            hwif: hw.clone(),
            gba,
        })));

        return 0;
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_closeEmulator(
        _env: JNIEnv,
        _: JClass,
    ) {
        EMULATOR = None;
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_runFrame(
        env: JNIEnv,
        _: JClass,
        frame_buffer: jintArray,
    ) -> jint {
        get_static_global!(EMULATOR: &mut e => {
                e.gba.frame();
                // let our_buffer = std::mem::transmute::<&[u32], &[i32]>(&e.hwif.borrow().frame_buffer as &[u32]);
                env.set_int_array_region(frame_buffer, 0, std::mem::transmute::<&[u32], &[i32]>(&e.gba.get_frame_buffer() as &[u32]))
                    .expect("failed to copy frame buffer to java");

                return 0;
            } else {
                return -1;
            }
        );
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_setKeyState(
        env: JNIEnv,
        _: JClass,
        key_state: jint,
    ) -> jint {
        get_static_global!(EMULATOR: &mut e => {
                e.hwif.borrow_mut().key_state = key_state as u16;
                return 0;
            } else {
                return -1;
            }
        );
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorBindings_log(
        _env: JNIEnv,
        _: JClass,
    ) -> jint {
        get_static_global!(EMULATOR: &e => {
            info!("CPU LOG: {:#x?}", e.gba.cpu);
            return 0;
        } else {
            return -1;
        });
    }
}
