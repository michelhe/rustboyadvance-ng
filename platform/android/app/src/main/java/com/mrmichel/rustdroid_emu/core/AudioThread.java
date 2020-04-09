package com.mrmichel.rustdroid_emu.core;

import android.media.AudioTrack;

public class AudioThread extends Thread {

    AudioTrack audioTrack;
    Emulator emulator;
    boolean enabled;
    boolean stopping;

    public AudioThread(AudioTrack audioTrack, Emulator emulator) {
        super();
        this.audioTrack = audioTrack;
        this.emulator = emulator;
        this.enabled = true;
        this.stopping = false;
    }

    public void setStopping(boolean stopping) {
        this.stopping = stopping;
    }

    public void setEnabled(boolean enabled) {
        this.enabled = enabled;
    }

    public boolean isStopping() {
        return stopping;
    }

    public boolean isEnabled() {
        return enabled;
    }

    @Override
    public void run() {
        super.run();

        while (!stopping) {
            if (enabled) {
                short[] samples = emulator.collectAudioSamples();
                audioTrack.write(samples, 0, samples.length);
            }
        }
    }
}
