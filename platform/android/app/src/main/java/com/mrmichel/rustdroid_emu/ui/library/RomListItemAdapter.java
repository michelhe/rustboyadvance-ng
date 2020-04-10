package com.mrmichel.rustdroid_emu.ui.library;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.view.LayoutInflater;
import android.view.View;
import android.view.ViewGroup;
import android.widget.ArrayAdapter;
import android.widget.ImageView;
import android.widget.TextView;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import com.mrmichel.rustdroid_emu.R;
import com.mrmichel.rustdroid_emu.core.RomManager.RomMetadataEntry;

import java.util.ArrayList;

public class RomListItemAdapter extends ArrayAdapter<RomMetadataEntry> {

    Context context;
    ArrayList<RomMetadataEntry> items;

    public RomListItemAdapter(Context context, ArrayList<RomMetadataEntry> romEntries) {
        super(context, 0, romEntries);
        this.context = context;
        this.items = romEntries;
    }

    @Override
    public long getItemId(int position) {
        return 0;
    }

    @NonNull
    @Override
    public View getView(int position, @Nullable View convertView, @NonNull ViewGroup parent) {
        RomMetadataEntry item = getItem(position);

        if (convertView == null) {
            convertView = LayoutInflater.from(getContext()).inflate(R.layout.rom_item, parent, false);
        }

        ImageView screenshotImageView = convertView.findViewById(R.id.imageview_screenshot);

        Bitmap screenshot = item.getScreenshot();
        if (screenshot != null) {
            screenshotImageView.setImageBitmap(screenshot);
        } else {
            screenshotImageView.setImageBitmap(BitmapFactory.decodeResource(context.getResources(), R.mipmap.ic_launcher));
        }


        TextView tvTitle = convertView.findViewById(R.id.textview_game_title);
        tvTitle.setText(item.getName());

        return convertView;
    }
}
