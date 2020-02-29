package com.mrmichel.rustdroid_emu.ui.snapshots;

import androidx.appcompat.app.AppCompatActivity;

import android.content.Intent;
import android.os.Bundle;

import com.mrmichel.rustdroid_emu.R;
import com.mrmichel.rustdroid_emu.core.Snapshot;

public class SnapshotPickerActivity extends AppCompatActivity implements ISnapshotListener {

    static Snapshot pickedSnapshot;

    public static Snapshot obtainPickedSnapshot() {
        Snapshot ret = pickedSnapshot;
        pickedSnapshot = null;
        return ret;
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.snapshot_picker_activity);
        if (savedInstanceState == null) {
            getSupportFragmentManager().beginTransaction()
                    .replace(R.id.container, SnapshotListFragment.newInstance(this))
                    .commitNow();
        }
    }

    @Override
    public void onSnapshotClicked(Snapshot snapshot) {
        Intent data = new Intent();
        pickedSnapshot = snapshot;
        setResult(RESULT_OK, data);
        finish();
    }
}
