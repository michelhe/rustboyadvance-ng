package com.mrmichel.rustdroid_emu.ui;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.appcompat.app.AppCompatActivity;
import androidx.core.app.ActivityCompat;
import androidx.core.content.ContextCompat;

import android.Manifest;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Bundle;
import android.util.Log;
import android.widget.Toast;

import com.mrmichel.rustdroid_emu.R;

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

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.main_activity);

        if (ContextCompat.checkSelfPermission(this,
                Manifest.permission.WRITE_EXTERNAL_STORAGE)
                != PackageManager.PERMISSION_GRANTED) {

            // No explanation needed; request the permission
            ActivityCompat.requestPermissions(this
                    ,
                    new String[]{Manifest.permission.WRITE_EXTERNAL_STORAGE},
                    REQUEST_PERMISSION_CODE);
        } else {
            // Permission has already been granted
            initCacheBios();

        }
    }

    private void cacheBiosInAppFiles(byte[] bios) throws FileNotFoundException, IOException {
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

                    initEmulator(bios);
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
            initEmulator(bios);
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

    private void initEmulator(byte[] bios) {
        Intent intent = new Intent(this, EmulatorActivity.class);
        intent.putExtra("bios", bios);
        startActivity(intent);
    }
}
