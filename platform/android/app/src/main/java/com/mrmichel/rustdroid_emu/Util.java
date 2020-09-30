package com.mrmichel.rustdroid_emu;

import android.app.Activity;
import android.content.Context;
import android.content.DialogInterface;
import android.content.Intent;
import android.graphics.Bitmap;
import android.net.Uri;
import android.os.Build;
import android.util.Log;

import androidx.appcompat.app.AlertDialog;
import androidx.core.content.FileProvider;

import com.mrmichel.rustdroid_emu.ui.EmulatorActivity;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileNotFoundException;
import java.io.FileOutputStream;
import java.io.IOException;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.zip.GZIPInputStream;
import java.util.zip.GZIPOutputStream;

public class Util {

    private static final String TAG = "Util";


    public static void startEmulator(Context context, byte[] bios, int romId) {
        Intent intent = new Intent(context, EmulatorActivity.class);
        intent.putExtra("bios", bios);
        intent.putExtra("romId", romId);
        context.startActivity(intent);
    }


    public static void showAlertDialogAndExit(final Activity activity, Exception e) {
        new AlertDialog.Builder(activity)
                .setTitle(e.toString())
                .setMessage(e.getMessage())
                // Specifying a listener allows you to take an action before dismissing the dialog.
                // The dialog is automatically dismissed when a dialog button is clicked.
                .setPositiveButton(android.R.string.yes, new DialogInterface.OnClickListener() {
                    public void onClick(DialogInterface dialog, int which) {
                        activity.finishAffinity();
                    }
                })
                .setIcon(android.R.drawable.ic_dialog_alert)
                .show();
    }

    public static void showAlertDialog(final Activity activity, Exception e) {
        new AlertDialog.Builder(activity)
                .setTitle(e.toString())
                .setMessage(e.getMessage())
                .setIcon(android.R.drawable.ic_dialog_alert)
                .show();
    }


    public static byte[] compressBitmapToByteArray(Bitmap bitmap) {
        ByteArrayOutputStream byteArrayOutputStream = new ByteArrayOutputStream();
        bitmap.compress(Bitmap.CompressFormat.PNG, 10, byteArrayOutputStream);
        return byteArrayOutputStream.toByteArray();
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

            while ((len = gis.read(buffer, 0, 8192)) != -1) {
                outputStream.write(buffer, 0, len);
            }
            gis.close();
            return outputStream.toByteArray();
        } catch (Exception e) {
            Log.e(TAG, "failed to read compressed file " + file.toString() + " error: " + e.getMessage());
            return null;
        }
    }

    public static byte[] readFile(File file) throws IOException {
        byte[] buffer = new byte[8192];
        ByteArrayOutputStream outputStream = new ByteArrayOutputStream();
        FileInputStream fis = new FileInputStream(file);

        int len;

        while ((len = fis.read(buffer, 0, 8192)) != -1) {
            outputStream.write(buffer, 0, len);
        }
        fis.close();
        return outputStream.toByteArray();
    }

    public static String byteArrayToHexString(final byte[] bytes) {
        StringBuilder sb = new StringBuilder();
        for (byte b : bytes) {
            sb.append(String.format("%02x", b & 0xff));
        }
        return sb.toString();
    }

    public static String getHash(final byte[] bytes) {
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

    public static void shareFile(Context context, File file, String message) throws FileNotFoundException {
        if (!file.exists()) {
            throw new FileNotFoundException("file does not exist");
        }

        final Uri uri;
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.LOLLIPOP) {
            uri = Uri.fromFile(file);
        } else {
            uri = FileProvider.getUriForFile(context, context.getPackageName() + ".provider", file);
        }

        if (uri == null) {
            throw new FileNotFoundException("could not find file to share");
        }


        Intent intentShareFile = new Intent(Intent.ACTION_SEND);
        intentShareFile.setType("*/*");
        intentShareFile.setFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        intentShareFile.putExtra(Intent.EXTRA_STREAM, uri);
        intentShareFile.putExtra(Intent.EXTRA_TEXT, message);

        context.startActivity(Intent.createChooser(intentShareFile, message));
    }
}
