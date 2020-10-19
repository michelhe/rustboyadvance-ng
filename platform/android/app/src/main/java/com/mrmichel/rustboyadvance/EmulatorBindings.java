package com.mrmichel.rustboyadvance;

/**
 * JNI wrapper to the rust core
 */
public class EmulatorBindings {

    static {
        System.loadLibrary("rustboyadvance_jni");
    }

    /**
     * Open a new emulator context
     *
     * @param bios        bytearray of the GBA bios
     * @param rom         bytearray of the rom to run
     * @param renderer    renderer instance
     * @param audioPlayer audio player instance
     * @param keypad      Keypad instance
     * @param save_name   name of the save file TODO remove this
     * @param skipBios    skip bios
     * @return the emulator context to use pass to other methods in this class
     * @throws NativeBindingException
     */
    public static native long openEmulator(byte[] bios, byte[] rom, IFrameRenderer renderer, IAudioPlayer audioPlayer, Keypad keypad, String save_name, boolean skipBios) throws NativeBindingException;

    /**
     * Open a new emulator context from a saved state buffer
     *
     * @param bios        bytearray of the GBA bios
     * @param rom         bytearray of the rom to run
     * @param savedState  saved state buffer
     * @param renderer    renderer instance
     * @param audioPlayer audio player instance
     * @param keypad      Keypad instance
     * @return
     * @throws NativeBindingException
     */
    public static native long openSavedState(byte[] bios, byte[] rom, byte[] savedState, IFrameRenderer renderer, IAudioPlayer audioPlayer, Keypad keypad) throws NativeBindingException;

    /**
     * Destroys the emulator instance
     * should be put in a finalizer or else the emulator context may leak.
     *
     * @param ctx
     */
    public static native void closeEmulator(long ctx);

    /**
     * Run the emulation thread
     *
     * @param ctx
     */
    public static native void runMainLoop(long ctx);

    public static native void pause(long ctx);

    public static native void resume(long ctx);

    public static native void setTurbo(long ctx, boolean turbo);

    public static native void stop(long ctx);


    public static native int[] getFrameBuffer(long ctx);

//    /**
//     * Runs the emulation for a single frame.
//     * @param ctx
//     * @param frame_buffer will be filled with the frame buffer to render
//     */
//    public static native void runFrame(long ctx, int[] frame_buffer);

    /**
     * @param ctx
     * @return The loaded ROM title
     */
    public static native String getGameTitle(long ctx);

    /**
     * @param ctx
     * @return The loaded ROM game code
     */
    public static native String getGameCode(long ctx);

    /**
     * Sets the keystate
     *
     * @param keyState
     */
    public static native void setKeyState(long ctx, int keyState);

    /**
     * Saves the state
     *
     * @param ctx
     * @return save state buffer
     * @throws NativeBindingException
     */
    public static native byte[] saveState(long ctx) throws NativeBindingException;

    /**
     * Loads a save state
     *
     * @param ctx
     * @param state save state buffer
     * @throws NativeBindingException
     */
    public static native void loadState(long ctx, byte[] state) throws NativeBindingException;

    /**
     * Logs the emulator state
     *
     * @return non-zero value on failure
     */
    public static native void log(long ctx);

    public class NativeBindingException extends Exception {
        public NativeBindingException(String errorMessage) {
            super(errorMessage);
        }
    }
}
