package com.mrmichel.rustdroid_emu.ui;

import android.util.Log;

import com.mrmichel.rustdroid_emu.core.Emulator;

public class EmulationThread extends Thread {

    private static final String TAG = "EmulationThread";

    public static final long NANOSECONDS_PER_MILLISECOND = 1000000;
    public static final long FRAME_TIME = 1000000000 / 60;

    private Emulator emulator;
    private ScreenView screenView;

    private boolean running;

    public EmulationThread(Emulator emulator, ScreenView screenView) {
        this.emulator = emulator;
        this.screenView = screenView;
        this.running = false;
    }

    public void pauseEmulation() {
        this.emulator.pause();
    }

    public void resumeEmulation() {
        this.emulator.resume();
    }

    @Override
    public void run() {
        super.run();

        // wait until renderer is ready
        while (!screenView.getRenderer().isReady());

        while (!emulator.isOpen());

        running = true;
        emulator.runMainLoop();
        Log.d(TAG, "Native runMainLoop returned!");
        running = false;
    }


}
