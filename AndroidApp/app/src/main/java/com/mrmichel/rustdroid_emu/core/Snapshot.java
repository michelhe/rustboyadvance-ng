package com.mrmichel.rustdroid_emu.core;

import android.graphics.Bitmap;

import java.io.File;

public class Snapshot {
    private Bitmap preview;
    private String gameCode;
    private String gameTitle;
    private long timestamp;
    private File file;

    public Snapshot(File file, String gameCode, String gameTitle, Bitmap preview) {
        this.file = file;
        this.gameCode = gameCode;
        this.gameTitle = gameTitle;
        this.preview = preview;
        this.timestamp = System.currentTimeMillis();
    }

    public Snapshot(File file, String gameCode, String gameTitle, Bitmap preview, long timestamp) {
        this.file = file;
        this.gameCode = gameCode;
        this.gameTitle = gameTitle;
        this.preview = preview;
        this.timestamp = timestamp;
    }

    public String getGameCode() {
        return gameCode;
    }

    public String getGameTitle() {
        return gameTitle;
    }

    public long getTimestamp() {
        return timestamp;
    }

    public Bitmap getPreview() {
        return preview;
    }

    public byte[] load() {
        return SnapshotManager.readCompressedFile(this.file);
    }
}
