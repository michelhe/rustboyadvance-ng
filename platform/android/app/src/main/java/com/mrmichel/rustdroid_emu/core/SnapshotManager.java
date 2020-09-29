package com.mrmichel.rustdroid_emu.core;

import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.sqlite.SQLiteDatabase;
import android.database.sqlite.SQLiteOpenHelper;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;

import com.mrmichel.rustdroid_emu.Util;

import java.io.File;
import java.sql.Timestamp;
import java.util.ArrayList;

public class SnapshotManager {
    private static final String TAG = "SnapshotManager";

    private static final String SNAPSHOT_ROOT = "snapshots";
    private static final String DB_NAME = "snapshots";

    static SnapshotManager instance;

    private Context context;

    private SnapshotDatabaseHelper dbHelper;

    private SnapshotManager(Context context) {
        this.context = context;
        this.dbHelper = new SnapshotDatabaseHelper(this.context);
//        this.snapshotDB = context.openOrCreateDatabase("snapshots", Context.MODE_PRIVATE, null);
    }

    public static SnapshotManager getInstance(Context context) {
        if (instance == null) {
            instance = new SnapshotManager(context);
        }
        return instance;
    }

    private File getPreviewsDir(String gameCode) {
        File d = new File(context.getFilesDir(), SNAPSHOT_ROOT + "/previews");
        d.mkdirs();
        return d;
    }

    private File getSnapshotDir(String gameCode) {
        File d = new File(context.getFilesDir(), SNAPSHOT_ROOT + "/data");
        d.mkdirs();
        return d;
    }

    public void saveSnapshot(String gameCode, String gameTitle, Bitmap previewImage, byte[] data) {
        byte[] previewImageBytes = Util.compressBitmapToByteArray(previewImage);

        String hash = Util.getHash(data);

        File previewsDir = getPreviewsDir(gameCode);
        File snapshotsDir = getSnapshotDir(gameCode);

        File previewFile = new File(previewsDir, hash);
        Util.writeCompressedFile(previewFile, previewImageBytes);

        File snapshotFile = new File(snapshotsDir, hash);
        Util.writeCompressedFile(snapshotFile, data);

        this.dbHelper.insertSnapshot(gameCode, gameTitle, previewFile, snapshotFile);
    }

    public void deleteSnapshot(Snapshot snapshot) {

        SQLiteDatabase db = dbHelper.getWritableDatabase();

        File file = snapshot.getFile();
        db.delete(SnapshotDatabaseHelper.TABLE_NAME, "dataFile = '" + file.toString() + "'", null);
        file.delete();
    }

    public ArrayList<Snapshot> getAllSnapshots() {
        return this.dbHelper.getEntries();
    }

    public ArrayList<Snapshot> getByGameCode(String gameCode) {
        return this.dbHelper.getEntries(gameCode);
    }

    public class SnapshotDBEntry {
        String gameCode;
        String gameTitle;
        File previewFile;
        File snapshotFile;
        Timestamp timestamp;

        public SnapshotDBEntry(String gameCode, File previewFile, File snapshotFile, Timestamp timestamp) {
            this.gameCode = gameCode;
            this.previewFile = previewFile;
            this.snapshotFile = snapshotFile;
            this.timestamp = timestamp;
        }
    }

    public class SnapshotDatabaseHelper extends SQLiteOpenHelper {
        public static final String TABLE_NAME = "snapshot_table";
        private Context context;

        public SnapshotDatabaseHelper(Context context) {
            super(context, DB_NAME, null, 1);
            this.context = context;
        }

        @Override
        public void onCreate(SQLiteDatabase db) {
            db.execSQL("create table " + TABLE_NAME +
                    " (id INTEGER PRIMARY KEY, gameCode TEXT, gameTitle TEXT, timestamp DATETIME DEFAULT CURRENT_TIMESTAMP, previewImageFile TEXT, dataFile TEXT)"
            );
        }

        public void insertSnapshot(String gameCode, String gameTitle, File previewCacheFile, File snapshotDataFile) {
            SQLiteDatabase db = this.getWritableDatabase();
            ContentValues values = new ContentValues();
            values.put("gameCode", gameCode);
            values.put("gameTitle", gameTitle);
            values.put("previewImageFile", previewCacheFile.getPath());
            values.put("dataFile", snapshotDataFile.getPath());
            db.insertOrThrow(TABLE_NAME, null, values);
            db.close();
        }

        public ArrayList<Snapshot> getEntriesByQuery(String query) {
            ArrayList<Snapshot> arrayList = new ArrayList<>();

            SQLiteDatabase db = this.getWritableDatabase();
            Cursor cursor = db.rawQuery(query, null);

            if (cursor.moveToFirst()) {
                do {
                    String gameCode = cursor.getString(1);
                    String gameTitle = cursor.getString(2);
                    Timestamp timestamp = Timestamp.valueOf(cursor.getString(3));
                    File previewImageFile = new File(cursor.getString(4));
                    File dataFile = new File(cursor.getString(5));

                    byte[] previewData = Util.readCompressedFile(previewImageFile);
                    Bitmap previewBitmap = BitmapFactory.decodeByteArray(previewData, 0, previewData.length);

                    arrayList.add(new Snapshot(dataFile, gameCode, gameTitle, previewBitmap, timestamp.getTime()));
                } while (cursor.moveToNext());
            }

            cursor.close();
            db.close();
            return arrayList;
        }

        public ArrayList<Snapshot> getEntries() {
            return getEntriesByQuery("SELECT * FROM " + TABLE_NAME + " ORDER BY timestamp DESC ");
        }

        public ArrayList<Snapshot> getEntries(String gameCode) {
            return getEntriesByQuery("SELECT * FROM " + TABLE_NAME + " where gameCode = '" + gameCode + "' ORDER BY timestamp DESC ");
        }

        @Override
        public void onUpgrade(SQLiteDatabase db, int oldVersion, int newVersion) {

        }
    }
}
