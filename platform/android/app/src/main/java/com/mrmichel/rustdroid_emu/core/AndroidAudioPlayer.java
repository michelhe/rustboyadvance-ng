package com.mrmichel.rustdroid_emu.core;

import android.media.AudioFormat;
import android.media.AudioManager;
import android.media.AudioTrack;
import android.os.Build;
import android.util.Log;

import com.mrmichel.rustboyadvance.IAudioPlayer;


/**
 * Simple wrapper around the android AudioTrack class that implements IAudioPlayer
 */
public class AndroidAudioPlayer implements IAudioPlayer {
    private static final String TAG = "AndroidAudioPlayer";

    private static final int BUFFER_SIZE_IN_BYTES = 8192;
    private static int SAMPLE_RATE_HZ = 44100;

    private AudioTrack audioTrack;

    public AndroidAudioPlayer() {
        if (Build.VERSION.SDK_INT >= 23) {
            AudioTrack.Builder audioTrackBuilder = new AudioTrack.Builder()
                    .setAudioFormat(new AudioFormat.Builder()
                            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                            .setSampleRate(SAMPLE_RATE_HZ)
                            .setChannelMask(AudioFormat.CHANNEL_IN_STEREO | AudioFormat.CHANNEL_OUT_STEREO)
                            .build()
                    )
                    .setBufferSizeInBytes(AndroidAudioPlayer.BUFFER_SIZE_IN_BYTES)
                    .setTransferMode(AudioTrack.MODE_STREAM);
            if (Build.VERSION.SDK_INT >= 26) {
                audioTrackBuilder.setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY);
            }
            this.audioTrack = audioTrackBuilder.build();
        } else {
            this.audioTrack = new AudioTrack(
                    AudioManager.STREAM_MUSIC,
                    SAMPLE_RATE_HZ,
                    AudioFormat.CHANNEL_IN_STEREO | AudioFormat.CHANNEL_OUT_STEREO,
                    AudioFormat.ENCODING_PCM_16BIT,
                    AndroidAudioPlayer.BUFFER_SIZE_IN_BYTES,
                    AudioTrack.MODE_STREAM);
        }
        Log.d(TAG, "sampleCount = " + this.getSampleCount());
    }

    @Override
    public int audioWrite(short[] buffer, int offsetInShorts, int sizeInShorts) {
        if (Build.VERSION.SDK_INT >= 23) {
            return this.audioTrack.write(buffer, offsetInShorts, sizeInShorts, AudioTrack.WRITE_NON_BLOCKING);
        } else {
            // Native bindings will do its best to make sure this doesn't block anyway
            return this.audioTrack.write(buffer, offsetInShorts, sizeInShorts);
        }
    }

    @Override
    public void pause() {
        this.audioTrack.pause();
    }

    @Override
    public void play() {
        this.audioTrack.play();
    }

    @Override
    public int getSampleCount() {
        if (Build.VERSION.SDK_INT >= 23) {
            return this.audioTrack.getBufferSizeInFrames();
        } else {
            return BUFFER_SIZE_IN_BYTES / 2;
        }
    }

    @Override
    public int getSampleRate() {
        return this.audioTrack.getSampleRate();
    }

    @Override
    public int availableBufferSize() {
        return 2;
    }

}
