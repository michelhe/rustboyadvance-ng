package com.mrmichel.rustdroid_emu.ui;

import com.mrmichel.rustdroid_emu.core.Emulator;

public class EmulationThread extends Thread {

    public static final long NANOSECONDS_PER_MILLISECOND = 1000000;
    public static final long FRAME_TIME = 1000000000 / 60;

    private Emulator emulator;
    private ScreenView screenView;

    private boolean turbo;
    private boolean running;
    private boolean stopping;

    public EmulationThread(Emulator emulator, ScreenView screenView) {
        this.emulator = emulator;
        this.screenView = screenView;
        this.running = true;
    }

    public void setStopping(boolean stopping) {
        this.stopping = stopping;
    }

    public void pauseEmulation() {
        running = false;
    }

    public void resumeEmulation() {
        running = true;
    }

    public void setTurbo(boolean turbo) {
        this.turbo = turbo;
    }

    public boolean isTurbo() { return turbo; }

    @Override
    public void run() {
        super.run();

        // wait until renderer is ready
        while (!screenView.getRenderer().isReady());

        while (!stopping) {
            if (running) {
                long startTimer = System.nanoTime();
                emulator.runFrame();
                if (!turbo) {
                    long currentTime = System.nanoTime();
                    long timePassed = currentTime - startTimer;

                    long delay = FRAME_TIME - timePassed;
                    if (delay > 0) {
                        try {
                            Thread.sleep(delay / NANOSECONDS_PER_MILLISECOND);
                        } catch (Exception e) {

                        }
                    }
                }

                screenView.updateFrame(emulator.getFrameBuffer());
            }
        }
    }


}
