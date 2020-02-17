# RustBoyAdvance-NG

![license](https://img.shields.io/github/license/michelhe/rustboyadvance-ng) [![Build Status](https://travis-ci.com/michelhe/rustboyadvance-ng.svg?branch=master)](https://travis-ci.com/michelhe/rustboyadvance-ng)

![icon ](assets/icon.png)

Nintendo GameBoy Advance ™ emulator and debugger, written in rust.

# Progress

![Pokemon Emerald](media/screenshot1.png)

## Supported features:
* Display modes 0,1,2,3,4
* PCM Audio channels
* Snapshots (AKA savestates)
* Cartridge backup saves 

## Todo:
* CGB audio (4 wave generator channels)
* web.asm frontend
* Andriod & IOS applications
* color correction
* Configurable keybindings
* Controller support


# Build and usage

The following instructions build the `rba-sdl2` target which is currently the main platform. (`rba-minifb` is also available without audio support)

To get started, you need to get a [stable rust toolchain](https://rustup.rs).

## Linux build dependencies
Install SDL2 dependencies

```bash
sudo apt-get -y install libsdl2-dev libsdl2-image-dev
```

## Windows build dependencies
Download SDL2 runtime binaries for windows (either 32bit or 64bit depending on the target machine)
https://www.libsdl.org/download-2.0.php
https://www.libsdl.org/projects/SDL_image/

Extract all the DLLs into the project root (Yeah, its dirty and a build script will be written in the future to automate this)

## Build & Usage
You need to obtain a gba bios binary.
An [open source GBA bios](https://github.com/Nebuleon/ReGBA/blob/master/bios/gba_bios.bin) is also available and supported.


Place the bios file in the repository root and name it `gba_bios.bin` (or alternatively use the `-b` command line option) 


Build and run in release mode (performance is terrible in the `dev` profile)
```bash
$ cargo run --release -- path/to/rom
```


You can also drag&drop rom files or any zip files containing `.gba` files inside into the emulator window and a new rom will be loaded.

# Key bindings

> Currently the key bindings are not configureable.

GBA key bindings:

| Keyboard  	| GBA      	|
|-----------	|----------	|
| Up        	| Up       	|
| Down      	| Down     	|
| Left      	| Right    	|
| Right     	| Right    	|
| Z         	| B Button 	|
| X         	| A Button 	|
| Return    	| Start    	|
| Backspace 	| Select   	|
| A         	| L        	|
| S         	| R        	|

Special key bindings
| Key          	| Function          	|
|--------------	|--------------------	|
| Space (hold) 	| Disable 60fps cap  	|
| F5           	| Save snapshot file 	|
| F9           	| Load snapshot file 	|

# Why is this project needed ?

It's actually **not**. There are quite a lot of GBA emulators, ~~and even some written in rust~~. Actually, I couldn't find any game capable emulators written in rust.

I'm only doing this as a side project intended for learning rust.

This is my *third* take on this project. My first go at this was about 3 years ago, but I didn't like rust much at the time so it got abandoned.
I tried to resurrect it a year ago but didn't have the time to get invested in a side-project, let alone learning rust.

I've grown to like rust a lot since then, so here we go again.
You know what they say, *third time's a charm*.

## More Screenshots
 ![Kirby - Nightmare in Dreamland](media/screenshot2.png) ![Dragon Ball - Legacy of Goku 2](media/screenshot3.png)

# Links and attribution

- [ARM7TDMI Technical Reference Manual](http://infocenter.arm.com/help/topic/com.arm.doc.ddi0210c/DDI0210B.pdf)
    Technical Reference Manuals are **fun**.
- [GBATEK](http://problemkaputt.de/gbatek.htm)
    A single webpage written by *no$gba* developer Martin Korth.
    This page has pretty much everything. Seriously, it's the best.
- [TONC](https://www.coranac.com/tonc/text/)
    A comprehensive GBA dev guide that I used a-lot in order to understand the GBA system.
    Comes with neat demo roms that really helped me during development and debugging.
- [NanoboyAdvance](https://github.com/fleroviux/NanoboyAdvance)
    A GameBoy Advance emulator written in C++17 by a nice person called fleroviux.
    I've used this for debugging.
- [Eggvance gba-suite](https://github.com/jsmolka/gba-suite)
    Incredible test suite for the arm7tdmi interpreter that I'm using, written by Julian Smolka.
