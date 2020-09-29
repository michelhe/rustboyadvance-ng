package com.mrmichel.rustboyadvance;

public interface IAudioPlayer {
    int audioWrite(short[] buffer, int offsetInShorts, int sizeInShorts);

    void pause();

    void play();

    int getSampleCount();

    int getSampleRate();

    int availableBufferSize();
}
