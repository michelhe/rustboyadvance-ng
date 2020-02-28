package com.mrmichel.rustdroid_emu.ui.snapshots;

import android.content.Context;
import android.view.LayoutInflater;
import android.view.View;
import android.view.ViewGroup;
import android.widget.ArrayAdapter;
import android.widget.ImageView;
import android.widget.TextView;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import com.mrmichel.rustdroid_emu.R;
import com.mrmichel.rustdroid_emu.core.Snapshot;

import java.sql.Timestamp;
import java.util.ArrayList;

public class SnapshotItemAdapter extends ArrayAdapter<Snapshot> {

    Context context;
    ArrayList<Snapshot> items;

    public SnapshotItemAdapter(Context context, ArrayList<Snapshot> items) {
        super(context, 0, items);
        this.context = context;
        this.items = items;
    }

    @Override
    public int getCount() {
        return items.size();
    }

    @Override
    public long getItemId(int position) {
        return 0;
    }

    @NonNull
    @Override
    public View getView(int position, @Nullable View convertView, @NonNull ViewGroup parent) {
        Snapshot snapshot = getItem(position);

        if (convertView == null) {
            convertView = LayoutInflater.from(getContext()).inflate(R.layout.snapshot_item, parent, false);
        }

        ImageView preview = (ImageView) convertView.findViewById(R.id.imageview_snapshot_preview);
        preview.setImageBitmap(snapshot.getPreview());


        TextView tvTitle = (TextView) convertView.findViewById(R.id.textview_snapshot_title);
        tvTitle.setText(snapshot.getGameTitle());

        TextView tvTimestamp = (TextView) convertView.findViewById(R.id.textview_snapshot_timestmap);
        Timestamp timestamp = new Timestamp(snapshot.getTimestamp());
        tvTimestamp.setText(timestamp.toString());

        return convertView;
    }
}
