package com.mrmichel.rustdroid_emu.core;

import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.sqlite.SQLiteDatabase;
import android.database.sqlite.SQLiteOpenHelper;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.util.Log;

import java.io.BufferedInputStream;
import java.io.BufferedReader;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.sql.Timestamp;
import java.util.ArrayList;
import java.util.zip.GZIPInputStream;
import java.util.zip.GZIPOutputStream;

public class SnapshotManager {
    private static final String TAG = "SnapshotManager";

    private static final String SNAPSHOT_ROOT = "snapshots";
    private static final String DB_NAME = "snapshots";

    static SnapshotManager instance;

    private Context context;

    private SnapshotDatabaseHelper dbHelper;

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

                    byte[] previewData = readCompressedFile(previewImageFile);
                    Bitmap previewBitmap = BitmapFactory.decodeByteArray(previewData, 0, previewData.length);

                    arrayList.add(new Snapshot(dataFile, gameCode, gameTitle, previewBitmap, timestamp.getTime()));
                } while (cursor.moveToNext());
            }

            cursor.close();
            db.close();
            return arrayList;
        }

        public ArrayList<Snapshot> getAllEntries() {
            return getEntriesByQuery("SELECT * FROM " + TABLE_NAME + " ORDER BY timestamp DESC ");
        }

        public ArrayList<Snapshot> getAllEntries(String gameCode) {
            return getEntriesByQuery("SELECT * FROM " + TABLE_NAME + "where gameCode = " + gameCode + " ORDER BY timestamp DESC ");
        }

        @Override
        public void onUpgrade(SQLiteDatabase db, int oldVersion, int newVersion) {

        }
    }

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
            Log.e("SnapshotManager", "SHA-256 algo not found");
            return null;
        }

        md.update(bytes);
        return byteArrayToHexString(md.digest());
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

    public static void writeCompressedFile(File file, byte[] bytes) {
        try {
            FileOutputStream fos = new FileOutputStream(file);
            GZIPOutputStream gos = new GZIPOutputStream(fos);

            gos.write(bytes);
            gos.close();
            fos.close();
        } catch (Exception e) {
            Log.e(TAG, "failed to write compressed file " + file.toString() + " error: " + e.getMessage());
        }
    }

    public static byte[] readCompressedFile(File file) {
        try {
            byte[] buffer = new byte[8192];
            ByteArrayOutputStream outputStream = new ByteArrayOutputStream();
            GZIPInputStream gis = new GZIPInputStream(new FileInputStream(file));

            int len;

            while ( (len = gis.read(buffer, 0, 8192)) != -1) {
                outputStream.write(buffer, 0, len);
            }
            gis.close();
            return outputStream.toByteArray();
        } catch (Exception e) {
            Log.e(TAG, "failed to read compressed file " + file.toString() + " error: " + e.getMessage());
            return null;
        }
    }

    public static byte[] compressBitmapToByteArray(Bitmap bitmap) {
        ByteArrayOutputStream byteArrayOutputStream = new ByteArrayOutputStream();
        bitmap.compress(Bitmap.CompressFormat.PNG, 10, byteArrayOutputStream);
        return byteArrayOutputStream.toByteArray();
    }

    public void saveSnapshot(String gameCode, String gameTitle, Bitmap previewImage, byte[] data) {
        byte[] previewImageBytes = compressBitmapToByteArray(previewImage);

        String hash = getHash(data);

        File previewsDir = getPreviewsDir(gameCode);
        File snapshotsDir = getSnapshotDir(gameCode);

        File previewFile = new File(previewsDir, hash);
        writeCompressedFile(previewFile, previewImageBytes);

        File snapshotFile = new File(snapshotsDir, hash);
        writeCompressedFile(snapshotFile, data);

        this.dbHelper.insertSnapshot(gameCode, gameTitle, previewFile, snapshotFile);
    }

    public ArrayList<Snapshot> getAllSnapshots() {
        return this.dbHelper.getAllEntries();
    }

    public ArrayList<Snapshot> getByGameCode(String gameCode) {
        return this.dbHelper.getAllEntries(gameCode);
    }
}
