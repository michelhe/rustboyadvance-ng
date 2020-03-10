package com.mrmichel.rustdroid_emu.ui;

import android.Manifest;
import android.app.ActivityManager;
import android.content.Context;
import android.content.DialogInterface;
import android.content.Intent;
import android.content.pm.ConfigurationInfo;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Bundle;
import android.util.Log;
import android.widget.Toast;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.appcompat.app.AlertDialog;
import androidx.appcompat.app.AppCompatActivity;
import androidx.core.app.ActivityCompat;
import androidx.core.content.ContextCompat;

import com.mrmichel.rustdroid_emu.R;
import com.mrmichel.rustdroid_emu.ui.library.RomListActivity;

import java.io.FileInputStream;
import java.io.FileNotFoundException;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;

public class SplashActivity extends AppCompatActivity {

    private static final String TAG = "SplashActivity";
    private static final int REQUEST_PERMISSION_CODE = 55;
    private static final int BIOS_REQUEST_CODE = 66;

    @Override
    public void onRequestPermissionsResult(int requestCode, @NonNull String[] permissions, @NonNull int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode == REQUEST_PERMISSION_CODE) {
            if (permissions.length == 1 && permissions[0].equals(Manifest.permission.WRITE_EXTERNAL_STORAGE)) {
                if (grantResults[0] == PackageManager.PERMISSION_GRANTED) {
                    initCacheBios();
                } else {
                    Toast.makeText(this, "WRITE_EXTERNAL_STORAGE not granted, need to quit", Toast.LENGTH_LONG).show();
                    this.finishAffinity();
                }
            }
        }
    }

    private void checkOpenGLES20() {
        ActivityManager am = (ActivityManager) getSystemService(Context.ACTIVITY_SERVICE);
        ConfigurationInfo configurationInfo = am.getDeviceConfigurationInfo();
        if (configurationInfo.reqGlEsVersion >= 0x20000) {
            // Supported
        } else {
            new AlertDialog.Builder(this)
                    .setTitle("OpenGLES 2")
                    .setMessage("Your device doesn't support GLES20. reqGLEsVersion = " + configurationInfo.reqGlEsVersion)
                    .setPositiveButton(android.R.string.yes, new DialogInterface.OnClickListener() {
                        public void onClick(DialogInterface dialog, int which) {
                            finishAffinity();
                        }
                    })
                    .setIcon(android.R.drawable.ic_dialog_alert)
                    .show();
        }
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.splash_activity);

        checkOpenGLES20();

        if (ContextCompat.checkSelfPermission(this,
                Manifest.permission.WRITE_EXTERNAL_STORAGE)
                != PackageManager.PERMISSION_GRANTED) {

            // No explanation needed; request the permission
            ActivityCompat.requestPermissions(this
                    ,
                    new String[]{Manifest.permission.WRITE_EXTERNAL_STORAGE, Manifest.permission.READ_EXTERNAL_STORAGE},
                    REQUEST_PERMISSION_CODE);
        } else {
            // Permission has already been granted
            initCacheBios();

        }
    }

    private void cacheBiosInAppFiles(byte[] bios) throws IOException {
        FileOutputStream fos = openFileOutput("gba_bios.bin", MODE_PRIVATE);
        fos.write(bios);
        fos.close();
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, @Nullable Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (resultCode == RESULT_OK) {
            if (requestCode == BIOS_REQUEST_CODE) {
                Uri uri = data.getData();
                try {
                    InputStream inputStream = getContentResolver().openInputStream(uri);
                    byte[] bios = new byte[inputStream.available()];
                    inputStream.read(bios);
                    inputStream.close();

                    cacheBiosInAppFiles(bios);

                    startLibraryActivity(bios);
                } catch (Exception e) {
                    Log.e(TAG, "can't open bios file");
                    this.finishAffinity();
                }
            }
        } else {
            Log.e(TAG, "get error for request code " + requestCode);
        }
    }

    private void initCacheBios() {
        try {
            FileInputStream fis = openFileInput("gba_bios.bin");
            byte[] bios = new byte[fis.available()];
            fis.read(bios);
            startLibraryActivity(bios);
        } catch (FileNotFoundException e) {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.setType("*/*");
            intent.putExtra("android.content.extra.SHOW_ADVANCED", true);
            intent.putExtra(Intent.EXTRA_TITLE, "Please load the gba_bios.bin file");
            startActivityForResult(intent, BIOS_REQUEST_CODE);
        } catch (IOException e) {
            Log.e(TAG, "Got IOException while reading from bios");
        }
    }

    private void startLibraryActivity(byte[] bios) {
        Intent intent = new Intent(this, RomListActivity.class);
        intent.putExtra("bios", bios);
        startActivity(intent);
        finish();
    }
}
