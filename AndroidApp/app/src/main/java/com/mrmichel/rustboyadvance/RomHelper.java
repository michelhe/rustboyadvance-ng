package com.mrmichel.rustboyadvance;

public class RomHelper {

    static {
        System.loadLibrary("rustboyadvance_jni");
    }

    public static native String getGameCode(byte[] romData);

    public static native String getGameTitle(byte[] romData);
}
