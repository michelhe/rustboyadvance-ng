package com.mrmichel.rustdroid_emu.ui.snapshots;

import android.app.Activity;
import android.content.DialogInterface;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;

import com.google.android.material.floatingactionbutton.FloatingActionButton;
import com.google.android.material.snackbar.Snackbar;

import androidx.appcompat.app.AlertDialog;
import androidx.appcompat.app.AppCompatActivity;
import androidx.appcompat.widget.Toolbar;

import android.view.View;
import android.widget.AdapterView;
import android.widget.GridView;
import android.widget.ListView;
import android.widget.Toast;

import com.mrmichel.rustdroid_emu.R;
import com.mrmichel.rustdroid_emu.core.Snapshot;
import com.mrmichel.rustdroid_emu.core.SnapshotManager;

import java.util.ArrayList;

public class SnapshotActivity extends AppCompatActivity {

    private ArrayList<Snapshot> snapshots;
    public static final String EXTRA_GAME_CODE = "GAME_CODE";

    public void onChosenSnapshot(Snapshot snapshot) {
        Toast.makeText(this, "loading snapshot", Toast.LENGTH_SHORT).show();
        Intent intent = new Intent();
        setResult(RESULT_OK, intent);
        ChosenSnapshot.setSnapshot(snapshot);
        SnapshotActivity.this.finish();
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_snapshot);

        SnapshotManager manager = SnapshotManager.getInstance(this);

        String gameCode = getIntent().getStringExtra(EXTRA_GAME_CODE);

        if (gameCode == null) {
            snapshots = manager.getAllSnapshots();
        } else {
            snapshots = manager.getByGameCode(gameCode);
        }

        SnapshotItemAdapter adapter = new SnapshotItemAdapter(this, snapshots);

        GridView view = findViewById(R.id.gridview_snapshots);
        view.setAdapter(adapter);
        view.setOnItemClickListener(new AdapterView.OnItemClickListener() {
            @Override
            public void onItemClick(AdapterView<?> parent, View view, int position, long id) {
                final Snapshot snapshot = snapshots.get(position);
                onChosenSnapshot(snapshot);
            }
        });
    }
}

