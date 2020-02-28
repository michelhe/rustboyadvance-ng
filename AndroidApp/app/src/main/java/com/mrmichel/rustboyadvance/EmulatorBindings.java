package com.mrmichel.rustboyadvance;

/**
 * JNI wrapper to the rust core
 */
public class EmulatorBindings {

    public class NativeBindingException extends Exception {
        public NativeBindingException(String errorMessage) {
            super(errorMessage);
        }
    }

    /**
     * Open a new emulator context
     * @param bios bytearray of the GBA bios
     * @param rom bytearray of the rom to run
     * @param frameBuffer frameBuffer render target
     * @param save_name name of the save file TODO remove this
     * @return the emulator context to use pass to other methods in this class
     * @throws NativeBindingException
     */
    public static native long openEmulator(byte[] bios, byte[] rom, int[] frameBuffer, String save_name) throws NativeBindingException;

    /**
     * Make the emulator boot directly into the cartridge
     * @param ctx
     * @throws NativeBindingException
     */
    public static native void skipBios(long ctx) throws NativeBindingException;


    /**
     * Destroys the emulator instance
     * should be put in a finalizer or else the emulator context may leak.
     * @param ctx
     */
    public static native void closeEmulator(long ctx);


    /**
     * Runs the emulation for a single frame.
     * @param ctx
     * @param frame_buffer will be filled with the frame buffer to render
     */
    public static native void runFrame(long ctx, int[] frame_buffer);

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
     * @return non-zero value on failure
     */
    public static native void log(long ctx);
}
