package com.mrmichel.rustdroid_emu.ui.snapshots;

import android.os.Bundle;
import android.util.Log;
import android.view.ContextMenu;
import android.view.LayoutInflater;
import android.view.MenuInflater;
import android.view.MenuItem;
import android.view.View;
import android.view.ViewGroup;
import android.widget.AdapterView;
import android.widget.GridView;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.fragment.app.Fragment;

import com.mrmichel.rustdroid_emu.R;
import com.mrmichel.rustdroid_emu.core.Snapshot;
import com.mrmichel.rustdroid_emu.core.SnapshotManager;

import java.util.ArrayList;

public class SnapshotListFragment extends Fragment {

    private static final String TAG = "SnapshotListFragment";

    private GridView mGridView;

    private ArrayList<Snapshot> snapshots;

    private ISnapshotListener mListener;

    public SnapshotListFragment() {
        super();
        mListener = new ISnapshotListener() {
            @Override
            public void onSnapshotClicked(Snapshot snapshot) {
                Log.d(TAG, "stub onSnapshotClicked");
            }
        };
    }

    public SnapshotListFragment(ISnapshotListener listener) {
        super();
        mListener = listener;
    }

    public static SnapshotListFragment newInstance(ISnapshotListener listener) {
        return new SnapshotListFragment(listener);
    }

    @Override
    public void onCreateContextMenu(@NonNull ContextMenu menu, @NonNull View v, @Nullable ContextMenu.ContextMenuInfo menuInfo) {
        super.onCreateContextMenu(menu, v, menuInfo);
        if (v.getId() == R.id.gridview_snapshots) {
            MenuInflater inflater = getActivity().getMenuInflater();
            inflater.inflate(R.menu.menu_context_snapshot, menu);
        }
    }

    @Override
    public boolean onContextItemSelected(@NonNull MenuItem item) {
        AdapterView.AdapterContextMenuInfo menuInfo = (AdapterView.AdapterContextMenuInfo) item.getMenuInfo();

        Snapshot snapshot = snapshots.get(menuInfo.position);
        switch (item.getItemId()) {
            case R.id.action_delete:
                SnapshotManager.getInstance(getContext()).deleteSnapshot(snapshot);
                snapshots.remove(menuInfo.position);

                SnapshotItemAdapter adapter = new SnapshotItemAdapter(getContext(), snapshots);
                mGridView.setAdapter(adapter);
                mGridView.invalidate();

                return true;
            default:
                return super.onContextItemSelected(item);
        }
    }

    @Nullable
    @Override
    public View onCreateView(@NonNull LayoutInflater inflater, @Nullable ViewGroup container, @Nullable Bundle savedInstanceState) {
        return inflater.inflate(R.layout.snapshot_list_fragment, container, false);
    }

    @Override
    public void onStart() {
        super.onStart();

        Bundle args = getArguments();

        SnapshotManager manager = SnapshotManager.getInstance(getContext());

        String gameCode;
        if (args != null && (gameCode = args.getString("gameCode")) != null) {
            snapshots = manager.getByGameCode(gameCode);
        } else {
            snapshots = manager.getAllSnapshots();
        }

        mGridView = getActivity().findViewById(R.id.gridview_snapshots);
        SnapshotItemAdapter adapter = new SnapshotItemAdapter(getContext(), snapshots);
        mGridView.setAdapter(adapter);
        mGridView.setOnItemClickListener(new AdapterView.OnItemClickListener() {
            @Override
            public void onItemClick(AdapterView<?> parent, View view, int position, long id) {
                final Snapshot snapshot = snapshots.get(position);
                mListener.onSnapshotClicked(snapshot);
            }
        });
        registerForContextMenu(mGridView);
    }
}

