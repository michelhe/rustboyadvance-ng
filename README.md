# RustBoyAdvance-NG

![license](https://img.shields.io/github/license/michelhe/rustboyadvance-ng) [![Build Status](https://travis-ci.com/michelhe/rustboyadvance-ng.svg?branch=master)](https://travis-ci.com/michelhe/rustboyadvance-ng)

![icon ](assets/icon.png)

Nintendo GameBoy Advance ™ emulator and debugger, written in rust.

# Build and usage

1. set-up rust *nightly*
2. Obtain a gba bios binary. you can get an [open source GBA bios](https://github.com/Nebuleon/ReGBA/blob/master/bios/gba_bios.bin)
3. Place the bios file in the repository root and name it `gba_bios.bin`

4. Build and run in release mode (performance is terrible in the `dev` profile)
    ```bash
    $ cargo run --release -- path/to/rom
    ```

# Why is this project needed ?

It's actually **not**. There are quite a lot of GBA emulators, ~~and even some written in rust~~. Actually, I couldn't find any game capable emulators written in rust.

I'm only doing this as a side project intended for learning rust.

This is my *third* take on this project. My first go at this was about 3 years ago, but I didn't like rust much at the time so it got abandoned.
I tried to resurrect it a year ago but didn't have the time to get invested in a side-project, let alone learning rust.

I've grown to like rust a lot since then, so here we go again.
You know what they say, *third time's a charm*.

# Progress

## Supported features:
* Display modes 0,4,5
* PCM Audio channels

## Todo:
* Display modes 2,3 (affine backgrounds)
* Flash(backup) support
* CGB audio (4 wave generator channels)
* web.asm frontend
* color correction

## Tested games status

### Kirby - Nightmare in Dreamland*
No issues so far

### Pokemon - Emerald
~~Won't boot unless binary patched to remove a loop querying the flash chip~~

### Dragon Ball - Legacy of Goku 2
~~crashes when entering in-game menu, other than that works fine.~~

## Screenshots

![Pokemon Emerald](media/screenshot1.png) ![Kirby - Nightmare in Dreamland](media/screenshot2.png) ![Dragon Ball - Legacy of Goku 2](media/screenshot3.png)

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