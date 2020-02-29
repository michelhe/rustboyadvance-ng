package com.mrmichel.rustdroid_emu.ui.snapshots;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.fragment.app.Fragment;

import android.view.LayoutInflater;
import android.view.View;
import android.view.ViewGroup;
import android.widget.AdapterView;
import android.widget.GridView;

import com.mrmichel.rustdroid_emu.R;
import com.mrmichel.rustdroid_emu.core.Snapshot;
import com.mrmichel.rustdroid_emu.core.SnapshotManager;

import java.util.ArrayList;

public class SnapshotViewerFragment extends Fragment {

    private ArrayList<Snapshot> snapshots;

    private Snapshot chosenSnapshot;

    public void onChosenSnapshot(Snapshot snapshot) {
        Intent intent = new Intent();
        getActivity().setResult(Activity.RESULT_OK, intent);
        chosenSnapshot = snapshot;
        ChosenSnapshot.setSnapshot(snapshot);
    }

    private ISnapshotListener mListener;

    public SnapshotViewerFragment(ISnapshotListener listener) {
        super();
        mListener = listener;
    }

    @Nullable
    @Override
    public View onCreateView(@NonNull LayoutInflater inflater, @Nullable ViewGroup container, @Nullable Bundle savedInstanceState) {
        return inflater.inflate(R.layout.activity_snapshot, container, false);
    }

    @Override
    public void onStart() {
        super.onStart();

        Bundle args = getArguments();

        SnapshotManager manager = SnapshotManager.getInstance(getContext());

        String gameCode = args.getString("gameCode");
        if (gameCode != null) {
            snapshots = manager.getByGameCode(gameCode);
        } else {
            snapshots = manager.getAllSnapshots();
        }

        SnapshotItemAdapter adapter = new SnapshotItemAdapter(getContext(), snapshots);

        GridView view = getActivity().findViewById(R.id.gridview_snapshots);
        view.setAdapter(adapter);
        view.setOnItemClickListener(new AdapterView.OnItemClickListener() {
            @Override
            public void onItemClick(AdapterView<?> parent, View view, int position, long id) {
                final Snapshot snapshot = snapshots.get(position);
                mListener.onSnapshotClicked(snapshot);
//                onChosenSnapshot(snapshot);
            }
        });
    }
}

