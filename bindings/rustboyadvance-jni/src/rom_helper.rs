use jni::objects::*;
use jni::sys::*;
use jni::JNIEnv;

use rustboyadvance_core::cartridge;

fn parse_rom_header(env: &JNIEnv, barr: jbyteArray) -> cartridge::header::CartridgeHeader {
    let rom_data = env.convert_byte_array(barr).unwrap();
    cartridge::header::parse(&rom_data).unwrap()
}

mod bindings {
    use super::*;

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_RomHelper_getGameCode(
        env: JNIEnv,
        _obj: JClass,
        rom_data: jbyteArray,
    ) -> jstring {
        let header = parse_rom_header(&env, rom_data);
        env.new_string(header.game_code).unwrap().into_inner()
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_mrmichel_rustboyadvance_RomHelper_getGameTitle(
        env: JNIEnv,
        _obj: JClass,
        rom_data: jbyteArray,
    ) -> jstring {
        let header = parse_rom_header(&env, rom_data);
        env.new_string(header.game_title).unwrap().into_inner()
    }
}
