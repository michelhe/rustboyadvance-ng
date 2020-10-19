package com.mrmichel.rustdroid_emu.core;

import com.mrmichel.rustboyadvance.EmulatorBindings;
import com.mrmichel.rustboyadvance.IFrameRenderer;
import com.mrmichel.rustboyadvance.Keypad;

public class Emulator {

    public Keypad keypad;
    /// context received by the native binding
    private long ctx = -1;

    private AndroidAudioPlayer audioPlayer;
    private IFrameRenderer frameRenderer;
    public Emulator(IFrameRenderer frameRenderer, AndroidAudioPlayer audioPlayer) {
        this.keypad = new Keypad();
        this.frameRenderer = frameRenderer;
        this.audioPlayer = audioPlayer;
    }

    public Emulator(long ctx, IFrameRenderer frameRenderer, AndroidAudioPlayer audioPlayer) {
        this.ctx = ctx;
        this.frameRenderer = frameRenderer;
        this.audioPlayer = audioPlayer;
        this.keypad = new Keypad();

    }

    /**
     * Get the native emulator handle for caching
     */
    public long getCtx() {
        return ctx;
    }

    public void runMainLoop() {
        EmulatorBindings.runMainLoop(this.ctx);
    }

    public void pause() {
        EmulatorBindings.pause(this.ctx);
        this.audioPlayer.pause();
    }

    public void resume() {
        EmulatorBindings.resume(this.ctx);
        this.audioPlayer.play();
    }

    public void setTurbo(boolean turbo) {
        EmulatorBindings.setTurbo(ctx, turbo);
    }

    public void stop() {
        EmulatorBindings.stop(this.ctx);
        this.audioPlayer.pause();

    }

    public int[] getFrameBuffer() {
        return EmulatorBindings.getFrameBuffer(this.ctx);
    }

    public synchronized byte[] saveState() throws EmulatorBindings.NativeBindingException {
        return EmulatorBindings.saveState(this.ctx);
    }

    public synchronized void loadState(byte[] state) throws EmulatorBindings.NativeBindingException, EmulatorException {
        if (ctx != -1) {
            EmulatorBindings.loadState(this.ctx, state);
        } else {
            throw new EmulatorException("Call open() first");
        }
    }

    public synchronized void open(byte[] bios, byte[] rom, String saveName, boolean skipBios) throws EmulatorBindings.NativeBindingException {
        this.ctx = EmulatorBindings.openEmulator(bios, rom, this.frameRenderer, this.audioPlayer, this.keypad, saveName, skipBios);
    }

    public synchronized void openSavedState(byte[] bios, byte[] rom, byte[] savedState) throws EmulatorBindings.NativeBindingException {
        this.ctx = EmulatorBindings.openSavedState(bios, rom, savedState, this.frameRenderer, this.audioPlayer, this.keypad);
    }

    public synchronized void close() {
        if (this.ctx != -1) {
            EmulatorBindings.closeEmulator(this.ctx);
            this.ctx = -1;

        }
    }

    public String getGameCode() {
        if (ctx != -1) {
            return EmulatorBindings.getGameCode(ctx);
        } else {
            return null;
        }
    }

    public String getGameTitle() {
        if (ctx != -1) {
            return EmulatorBindings.getGameTitle(ctx);
        } else {
            return null;
        }
    }

    public boolean isOpen() {
        return this.ctx != -1;
    }

    @Override
    protected void finalize() throws Throwable {
        super.finalize();
        close();
    }

    public synchronized void log() {
        EmulatorBindings.log(this.ctx);
    }

    public class EmulatorException extends Exception {
        public EmulatorException(String errorMessage) {
            super(errorMessage);
        }
    }
}
