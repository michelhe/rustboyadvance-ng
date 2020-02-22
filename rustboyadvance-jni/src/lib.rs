/// JNI Bindings to rustboyadvance
/// For use with the following java class
///
/// package com.mrmichel.rustboyadvance;
////
/// public class EmulatorInterface {
///
///     public static native int loadRom(String romPath);
///
///     public static native int openEmulator(String biosPath);
///
///     public static native void closeEmulator();
///
///     public static native int runFrame(int[] frame_buffer);
///
///     static {
///         System.loadLibrary("rustboyadvance_jni");
///     }
/// }
use std::cell::RefCell;
use std::os::raw::c_void;
use std::rc::Rc;

#[macro_use]
extern crate log;

use env_logger;

use rustboyadvance_ng::prelude::*;

struct Hardware {
    frame_buffer: [u32; DISPLAY_WIDTH * DISPLAY_HEIGHT],
}

impl Hardware {
    fn new() -> Hardware {
        Hardware {
            frame_buffer: [0; DISPLAY_WIDTH * DISPLAY_HEIGHT],
        }
    }
}

impl VideoInterface for Hardware {
    fn render(&mut self, buffer: &[u32]) {
        self.frame_buffer[..].clone_from_slice(buffer);
    }
}

impl AudioInterface for Hardware {}
impl InputInterface for Hardware {}

struct Emulator {
    hwif: Rc<RefCell<Hardware>>,
    gba: GameBoyAdvance,
}

#[allow(non_snake_case)]
pub mod android {
    use super::*;

    use std::path::Path;

    use jni;

    use jni::objects::{JClass, JString};
    use jni::sys::{jint, jintArray, JNI_VERSION_1_6};
    use jni::{JNIEnv, JavaVM};

    static mut EMULATOR: Option<Emulator> = None;
    static mut ROM: Option<Cartridge> = None;

    #[no_mangle]
    pub unsafe extern "C" fn JNI_OnLoad(_vm: *mut JavaVM, _reserved: *mut c_void) -> jint {
        env_logger::init();
        debug!("library loaded!");

        JNI_VERSION_1_6
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorInterface_loadRom(
        env: JNIEnv,
        _: JClass,
        rom_path: JString,
    ) -> jint {
        if EMULATOR.is_some() {
            error!("can't load rom while emulator is running");
            return -1;
        }

        let rom_path: String = env
            .get_string(rom_path)
            .expect("invalid rom path object")
            .into();
        let gamepak = GamepakBuilder::new()
            .file(&Path::new(&rom_path))
            .build()
            .expect("failed to load rom");

        info!("Loaded ROM file {:?}", gamepak.header);
        ROM = Some(gamepak);

        0
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorInterface_openEmulator(
        env: JNIEnv,
        _: JClass,
        bios_path: JString,
    ) -> jint {
        if let Some(cartridge) = ROM.clone() {
            let bios_path: String = env
                .get_string(bios_path)
                .expect("invalid bios path object")
                .into();
            let hw = Rc::new(RefCell::new(Hardware::new()));

            let bios_rom = read_bin_file(&Path::new(&bios_path)).expect("failed to load bios file");

            EMULATOR = Some(Emulator {
                hwif: hw.clone(),
                gba: GameBoyAdvance::new(bios_rom, cartridge, hw.clone(), hw.clone(), hw.clone()),
            });

            return 0;
        } else {
            error!("please call loadRom first");
            return -1;
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorInterface_closeEmulator(
        env: JNIEnv,
        _: JClass,
    ) {
        EMULATOR = None;
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_EmulatorInterface_runFrame(
        env: JNIEnv,
        _: JClass,
        frame_buffer: jintArray,
    ) -> jint {
        if let Some(emu) = &mut EMULATOR {
            emu.gba.frame();
            let our_buffer =
                std::mem::transmute::<&[u32], &[i32]>(&emu.hwif.borrow().frame_buffer as &[u32]);
            env.set_int_array_region(frame_buffer, 0, our_buffer)
                .expect("failed to copy frame buffer to java");

            return 0;
        }
        error!("emulator is not initalized");
        return -1;
    }
}
