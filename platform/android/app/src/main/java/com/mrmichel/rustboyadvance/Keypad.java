package com.mrmichel.rustboyadvance;

public class Keypad {
    private int keyState;

    public Keypad() {
        reset();
    }

    public void reset() {
        this.keyState = 0xffff;
    }

    public void onKeyDown(Key key) {
        this.keyState = this.keyState & ~(1 << key.keyBit);
    }

    public void onKeyUp(Key key) {
        this.keyState = this.keyState | (1 << key.keyBit);
    }

    public int getKeyState() {
        return keyState;
    }

    public enum Key {
        ButtonA(0),
        ButtonB(1),
        Select(2),
        Start(3),
        Right(4),
        Left(5),
        Up(6),
        Down(7),
        ButtonR(8),
        ButtonL(9);

        private final int keyBit;

        Key(int keyBit) {
            this.keyBit = keyBit;
        }
    }
}
