# Resources

Reference materials used while implementing the cached interpreter.

## gbatek/
- `gbatek.html` — Martin Korth's GBATEK, the canonical GBA hardware reference. Used for:
  - Memory map and wait-state timing (for cycle accuracy)
  - ARM7TDMI and Thumb instruction encodings (for the decoder-terminator logic)
  - Pipeline semantics (for block boundary reasoning)
  - Scheduler / IRQ behavior

Source: https://problemkaputt.de/gbatek.htm
