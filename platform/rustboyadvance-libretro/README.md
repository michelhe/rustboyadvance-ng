# Experimental RustBoyAdvance-NG retroarch core

## Desktop Build

To build for your host system, run
```sh
cargo build --release
```

For Linux, the output is `repo/target/release/librustboyadvance_libretro.so`
For windows, the output is `repo/target/release/rustboyadvance_libretro.dll`

## Android Build

Assuming you have NDK toolchain installed and configured, this crate can be built for android targets.

For example, for armv7-linux-androideabi
```sh
cargo build --release --target=armv7-linux-androideabi
```

The output will be in `/repo/target/armv7-linux-androideabi/release/librustboyadvance_libretro.so`