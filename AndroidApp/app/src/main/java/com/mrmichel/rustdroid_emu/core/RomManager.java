package com.mrmichel.rustdroid_emu.core;

import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.sqlite.SQLiteDatabase;
import android.database.sqlite.SQLiteOpenHelper;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.net.Uri;
import android.util.Log;
import android.widget.Toast;

import androidx.annotation.Nullable;
import androidx.documentfile.provider.DocumentFile;

import com.mrmichel.rustboyadvance.RomHelper;
import com.mrmichel.rustdroid_emu.Util;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.sql.Timestamp;
import java.util.ArrayList;

public class RomManager {
    private static final String TAG = "RomManager";
    private static RomManager instance;
    private RomDatabaseHelper dbHelper;
    private Context context;

    public RomManager(Context context) {
        this.context = context;
        this.dbHelper = new RomDatabaseHelper(this.context, 1);
    }

    public static RomManager getInstance(Context context) {
        if (instance == null) {
            instance = new RomManager(context);
        }
        return instance;
    }

    private static String byteArrayToHexString(final byte[] bytes) {
        StringBuilder sb = new StringBuilder();
        for (byte b : bytes) {
            sb.append(String.format("%02x", b & 0xff));
        }
        return sb.toString();
    }

    private static String getHash(final byte[] bytes) {
        MessageDigest md;
        try {
            md = MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException e) {
            // impossible
            Log.e(TAG, "SHA-256 algo not found");
            return null;
        }

        md.update(bytes);
        return byteArrayToHexString(md.digest());
    }

    public ArrayList<RomMetadataEntry> getAllRomMetaData() {
        return this.dbHelper.queryRomMetadata("SELECT * FROM " + RomDatabaseHelper.TABLE_METADATA + " ORDER BY lastPlayed DESC");
    }

    public RomMetadataEntry getRomMetadata(byte[] romData) {
        String romHash = getHash(romData);

        ArrayList<RomMetadataEntry> metadataEntries = dbHelper.queryRomMetadata(
                "SELECT * FROM " + RomDatabaseHelper.TABLE_METADATA + " where hash == '" + romHash + "'");

        if (metadataEntries.size() > 0) {
            return metadataEntries.get(0);
        } else {
            return null;
        }
    }

    public RomMetadataEntry getRomMetadata(int romId) {
        ArrayList<RomMetadataEntry> metadataEntries = dbHelper.queryRomMetadata(
                "SELECT * FROM " + RomDatabaseHelper.TABLE_METADATA + " where id = '" + romId + "'");

        if (metadataEntries.size() > 0) {
            return metadataEntries.get(0);
        } else {
            return null;
        }
    }


    private byte[] readFromUri(Uri uri) throws IOException {
        ContentResolver cr = context.getContentResolver();

        InputStream is = cr.openInputStream(uri);
        byte[] data = new byte[is.available()];
        is.read(data);
        return data;
    }

    public void importRom(DocumentFile documentFile) {

        Uri uri = documentFile.getUri();

        byte[] romData;
        try {
            romData = readFromUri(uri);
        } catch (Exception e) {
            Log.e(TAG, "could not read rom file");
            return;
        }

        if (null != getRomMetadata(romData)) {
            Toast.makeText(context, "This rom is already imported!", Toast.LENGTH_LONG).show();
            return;
        }

        String hash = getHash(romData);
        String gameCode = RomHelper.getGameCode(romData);
        String gameTitle = RomHelper.getGameTitle(romData);

        String romFileName = documentFile.getName();

        // Multiple roms can have the same title+code combo, so we rely on a hash to be a unique identifier.
        File baseDir = new File(context.getFilesDir(), hash);
        baseDir.mkdirs();

        File romFile = new File(baseDir, romFileName);

        // cache the rom
        try {
            FileOutputStream fileOutputStream = new FileOutputStream(romFile);
            fileOutputStream.write(romData);
        } catch (Exception e) {
            Log.e(TAG, "cannot cache rom file");
        }

        File backupFile = new File(baseDir, romFileName + ".sav");

        SQLiteDatabase db = dbHelper.getWritableDatabase();

        ContentValues cv = new ContentValues();

        cv.put("name", romFileName);
        cv.put("gameTitle", gameTitle);
        cv.put("gameCode", gameCode);
        cv.put("hash", hash);
        cv.put("path", romFile.getPath());
        cv.put("backupPath", backupFile.getPath());

        db.insertOrThrow(RomDatabaseHelper.TABLE_METADATA, null, cv);
        db.close();
    }

    public void deleteRomMetadata(RomMetadataEntry romMetadataEntry) {
        SQLiteDatabase db = dbHelper.getWritableDatabase();

        db.delete(RomDatabaseHelper.TABLE_METADATA, "id=" + romMetadataEntry.getId(), null);
    }

    public void updateLastPlayed(int romId) {
        Timestamp now = new Timestamp(System.currentTimeMillis());

        ContentValues cv = new ContentValues();
        cv.put("lastPlayed", now.toString());

        SQLiteDatabase db = dbHelper.getWritableDatabase();
        db.update(RomDatabaseHelper.TABLE_METADATA, cv, "id=" + romId, null);
    }

    public void updateScreenshot(int romId, Bitmap bitmap) {

        ContentValues cv = new ContentValues();
        cv.put("screenshot", Util.compressBitmapToByteArray(bitmap));

        SQLiteDatabase db = dbHelper.getWritableDatabase();
        db.update(RomDatabaseHelper.TABLE_METADATA, cv, "id=" + romId, null);
    }


    public class RomMetadataEntry {
        int id;
        String name;
        String gameTitle;
        String gameCode;
        File romFile;
        File backupFile;
        Bitmap screenshot;
        Timestamp lastPlayed;

        private RomMetadataEntry(int id, String name, String gameTitle, String gameCode, File romFile, File backupFile, Bitmap screenshot, Timestamp lastPlayed) {
            this.id = id;
            this.name = name;
            this.gameTitle = gameTitle;
            this.gameCode = gameCode;
            this.romFile = romFile;
            this.backupFile = backupFile;
            this.screenshot = screenshot;
            this.lastPlayed = lastPlayed;
        }


        public String getName() {
            return name;
        }

        public int getId() {
            return id;
        }

        public Bitmap getScreenshot() {
            return screenshot;
        }

        public File getBackupFile() {
            return backupFile;
        }

        public File getRomFile() {
            return romFile;
        }

        public String getGameTitle() {
            return gameTitle;
        }

        public String getGameCode() {
            return gameCode;
        }

        public Timestamp getLastPlayed() {
            return lastPlayed;
        }
    }

    private class RomDatabaseHelper extends SQLiteOpenHelper {
        private static final String DATABASE_NAME = "rom_db";

        private static final String TABLE_METADATA = "rom_metadata";


        public RomDatabaseHelper(@Nullable Context context, int version) {
            super(context, DATABASE_NAME, null, version);
        }

        @Override
        public void onCreate(SQLiteDatabase db) {
            db.execSQL("create table " + TABLE_METADATA +
                    " (id INTEGER PRIMARY KEY," +
                    "name TEXT UNIQUE," +
                    "hash TEXT UNIQUE," +
                    "gameTitle TEXT," +
                    "gameCode TEXT," +
                    "screenshot BLOB," +
                    "lastPlayed TIMESTAMP," +
                    "path TEXT UNIQUE," +
                    "backupPath TEXT UNIQUE" +
                    ")");
        }

        public ArrayList<RomMetadataEntry> queryRomMetadata(String query) {
            ArrayList<RomMetadataEntry> arrayList = new ArrayList<>();

            SQLiteDatabase db = this.getReadableDatabase();
            Cursor cursor = db.rawQuery(query, null);

            if (cursor.moveToFirst()) {
                do {

                    String name = cursor.getString(cursor.getColumnIndex("name"));

                    File romFile = new File(cursor.getString(cursor.getColumnIndex("path")));
                    File backupFile = new File(cursor.getString(cursor.getColumnIndex("backupPath")));

                    byte[] screenshotBlob = cursor.getBlob(cursor.getColumnIndex("screenshot"));
                    Bitmap screenshot;
                    if (null != screenshotBlob) {
                        screenshot = BitmapFactory.decodeByteArray(screenshotBlob, 0, screenshotBlob.length);
                    } else {
                        screenshot = null;
                    }

                    String gameTitle = cursor.getString(cursor.getColumnIndex("gameTitle"));
                    String gameCode = cursor.getString(cursor.getColumnIndex("gameCode"));

                    int id = cursor.getInt(cursor.getColumnIndex("id"));

                    String lastPlayedString = cursor.getString(cursor.getColumnIndex("lastPlayed"));
                    Timestamp lastPlayed;
                    if (lastPlayedString != null) {
                        lastPlayed = Timestamp.valueOf(lastPlayedString);
                    } else {
                        lastPlayed = null;
                    }

                    arrayList.add(new RomMetadataEntry(id, name, gameTitle, gameCode, romFile, backupFile, screenshot, lastPlayed));

                } while (cursor.moveToNext());
            }


            cursor.close();
            db.close();

            return arrayList;
        }

        @Override
        public void onUpgrade(SQLiteDatabase db, int oldVersion, int newVersion) {
            db.execSQL("DROP TABLE IF EXISTS " + TABLE_METADATA);
            onCreate(db);
        }
    }
}
