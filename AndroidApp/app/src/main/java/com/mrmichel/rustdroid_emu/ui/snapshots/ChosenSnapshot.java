package com.mrmichel.rustdroid_emu.ui.snapshots;

import com.mrmichel.rustdroid_emu.core.Snapshot;

/**
 * static class to transfer big byte arrays between activities
 */
public class ChosenSnapshot {

    static Snapshot snapshot;

    public static void setSnapshot(Snapshot snapshot) {
        ChosenSnapshot.snapshot = snapshot;
    }

    public static Snapshot takeSnapshot() {
        Snapshot result = ChosenSnapshot.snapshot;
        ChosenSnapshot.snapshot = null;
        return result;
    }
}
