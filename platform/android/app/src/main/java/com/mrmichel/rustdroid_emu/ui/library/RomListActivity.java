package com.mrmichel.rustdroid_emu.ui.library;

import android.content.ContentResolver;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.net.Uri;
import android.os.Bundle;
import android.provider.MediaStore;
import android.util.Log;
import android.view.ContextMenu;
import android.view.Menu;
import android.view.MenuInflater;
import android.view.MenuItem;
import android.view.View;
import android.widget.AdapterView;
import android.widget.GridView;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.appcompat.app.AppCompatActivity;
import androidx.documentfile.provider.DocumentFile;

import com.mrmichel.rustdroid_emu.R;
import com.mrmichel.rustdroid_emu.Util;
import com.mrmichel.rustdroid_emu.core.RomManager;
import com.mrmichel.rustdroid_emu.ui.SettingsActivity;
import com.mrmichel.rustdroid_emu.ui.SettingsFragment;

import java.io.File;
import java.util.ArrayList;
import java.util.Arrays;

public class RomListActivity extends AppCompatActivity {

    private static final String TAG = "RomListActivity";

    private static final int REQUEST_IMPORT_ROM = 100;
    private static final int REQUEST_IMPORT_DIR = 101;
    private static final int REQUEST_SET_IMAGE = 102;

    private static String[] ALLOWED_EXTENSIONS = {"gba", "zip", "bin"};

    private GridView mGridView;
    private RomListItemAdapter itemAdapter;

    private RomManager.RomMetadataEntry selectedEntry;

    private byte[] bios;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_rom_list);

        this.bios = getIntent().getByteArrayExtra("bios");

        mGridView = findViewById(R.id.gridview_rom_list);

        final RomManager romManager = RomManager.getInstance(this);

        ArrayList<RomManager.RomMetadataEntry> entries = romManager.getAllRomMetaData();

        itemAdapter = new RomListItemAdapter(this, entries);
        mGridView.setAdapter(itemAdapter);

        final Context context = this;
        mGridView.setOnItemClickListener(new AdapterView.OnItemClickListener() {
            @Override
            public void onItemClick(AdapterView<?> parent, View view, int position, long id) {
                RomManager.RomMetadataEntry entry = itemAdapter.getItem(position);
                romManager.updateLastPlayed(entry.getId());
                Util.startEmulator(context, bios, entry.getId());
            }
        });

        registerForContextMenu(mGridView);
    }


    @Override
    public void onCreateContextMenu(ContextMenu menu, View v, ContextMenu.ContextMenuInfo menuInfo) {
        super.onCreateContextMenu(menu, v, menuInfo);
        if (v.getId() == R.id.gridview_rom_list) {
            MenuInflater inflater = getMenuInflater();
            inflater.inflate(R.menu.menu_context_rom, menu);
        }
    }


    @Override
    public boolean onContextItemSelected(@NonNull MenuItem item) {
        AdapterView.AdapterContextMenuInfo menuInfo = (AdapterView.AdapterContextMenuInfo)item.getMenuInfo();

        RomManager romManager = RomManager.getInstance(this);

        RomManager.RomMetadataEntry entry = itemAdapter.getItem(menuInfo.position);

        selectedEntry = entry;

        switch (item.getItemId()) {
            case R.id.action_play:
                romManager.updateLastPlayed(entry.getId());
                Util.startEmulator(this, this.bios, entry.getId());
                return true;
            case R.id.action_delete:
                romManager.deleteRomMetadata(itemAdapter.getItem(menuInfo.position));
                return true;
            case R.id.action_set_screenshot:
                Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                intent.setType("image/*");
                intent.putExtra("romId", entry.getId());
                startActivityForResult(intent, REQUEST_SET_IMAGE);
            default:
                return super.onContextItemSelected(item);
        }
    }

    @Override
    public boolean onCreateOptionsMenu(Menu menu) {
        super.onCreateOptionsMenu(menu);
        getMenuInflater().inflate(R.menu.menu_rom_list, menu);
        return true;
    }

    @Override
    public boolean onOptionsItemSelected(@NonNull MenuItem item) {
        switch (item.getItemId()) {
            case R.id.action_import_rom:
                doImportRom();
            case R.id.action_import_directory:
                doImportDirectory();
                return true;
            case R.id.action_settings:
                Intent intent = new Intent(this, SettingsActivity.class);
                startActivity(intent);
            default:
                return super.onOptionsItemSelected(item);
        }
    }

    String getFileExtension(String name) {
        if (name == null) {
            return "";
        }
        int i = name.lastIndexOf('.');
        String ext = i > 0 ? name.substring(i + 1) : "";
        return ext;
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, @Nullable Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (resultCode == RESULT_OK) {
            ContentResolver contentResolver = getContentResolver();
            RomManager romManager = RomManager.getInstance(this);
            switch (requestCode) {
                case REQUEST_IMPORT_ROM:
                    Uri uri = data.getData();
                    contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);

                    romManager.importRom(DocumentFile.fromSingleUri(this, uri));

                    break;
                case REQUEST_IMPORT_DIR:

                    Uri treeUri = data.getData();

                    contentResolver.takePersistableUriPermission(treeUri, Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);

                    DocumentFile pickedDir = DocumentFile.fromTreeUri(this, treeUri);

                    for (DocumentFile file : pickedDir.listFiles()) {

                        String extension = getFileExtension(file.getName());
                        if (Arrays.asList(ALLOWED_EXTENSIONS).contains(extension)) {
                            Log.d(TAG, "Importing ROM " + file.getName() + " with size " + file.length() + " and type: " + extension);
                            romManager.importRom(file);
                        }
                    }

                    break;
                case REQUEST_SET_IMAGE:
                    int romId = selectedEntry.getId();

                    Bitmap bitmap;
                    try {
                        bitmap = MediaStore.Images.Media.getBitmap(this.getContentResolver(), data.getData());

                    }
                    catch (Exception e) {
                        Util.showAlertDiaglogAndExit(this, e);
                        return;
                    }

                    Log.d(TAG, "found bitmap");
                    romManager.updateScreenshot(romId, bitmap);

            }

            mGridView.setAdapter(new RomListItemAdapter(this, romManager.getAllRomMetaData()));
            mGridView.invalidate();

        } else {
            Log.e(TAG, "got error for request code " + requestCode);
        }
    }

    void doImportRom() {
        Intent intent = new Intent(Intent.ACTION_GET_CONTENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        Log.d(TAG, "pressed import rom");
        Intent chooser = Intent.createChooser(intent, "choose GBA rom file to import");
        startActivityForResult(chooser, REQUEST_IMPORT_ROM);
    }

    void doImportDirectory() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        startActivityForResult(intent, REQUEST_IMPORT_DIR);
    }
}
