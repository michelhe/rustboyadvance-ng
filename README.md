# RustBoyAdvance-NG

[![Build Status](https://travis-ci.com/michelhe/rustboyadvance-ng.svg?branch=master)](https://travis-ci.com/michelhe/rustboyadvance-ng)

RustBoyAdvance-NG Nintendo GameBoy Advance emulator and debugger, written in the rust programming language.

Currently passing armwrestler tests, and displays some of TONC's Demos.

# Using the REPL

You need to have rust installed, and somehow legally obtain a gba bios binary.

```bash
$ cargo run -- debug
```

# Why is this project needed ?

It's actually **not**. There are quite a lot of GBA emulators, ~~and even some written in rust~~. Actually, I couldn't find any game capable emulators written in rust.

I'm only doing this as a side project intended for learning rust.

This is my *third* take on this project. My first go at this was about 3 years ago, but I didn't like rust much at the time so it got abandoned.
I tried to resurrect it a year ago but didn't have the time to get invested in a side-project, let alone learning rust.

I've grown to like rust a lot since then, so here we go again.
You know what they say, *third time's a charm*.

# Links

- [ARM7TDMI Technical Reference Manual](http://infocenter.arm.com/help/topic/com.arm.doc.ddi0210c/DDI0210B.pdf)
    Technical Reference Manuals are **fun**.
- [GBATEK](http://problemkaputt.de/gbatek.htm)
    A single webpage written by *no$gba* developer  Martin Korth.
    This page has pretty much everything. Seriously, it's the best.
- [TONC](https://www.coranac.com/tonc/text/)
    A comprehensive GBA dev guide that I used a-lot in order to understand the GBA system.
    Comes with neat demo roms that really helped me during development and debugging.
- [NanoboyAdvance](https://github.com/fleroviux/NanoboyAdvance)
    A GameBoy Advance emulator written in C++17 by a nice person called fleroviux.
    I've used this emulator to search for a tough bug in mine.
- [Eggvance](https://github.com/jsmolka/eggvance/tree/master/tests)
    A GameBoy Advance emulator written in C++, with really useful CPU test roms.