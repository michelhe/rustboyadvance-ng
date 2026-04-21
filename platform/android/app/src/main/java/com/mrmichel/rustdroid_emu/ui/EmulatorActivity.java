package com.mrmichel.rustdroid_emu.ui;

import android.content.DialogInterface;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Bitmap;
import android.os.Bundle;
import android.util.Log;
import android.view.KeyEvent;
import android.view.Menu;
import android.view.MenuItem;
import android.view.MotionEvent;
import android.view.View;
import android.view.WindowManager;
import android.widget.CompoundButton;
import android.widget.Toast;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.appcompat.app.AlertDialog;
import androidx.appcompat.app.AppCompatActivity;
import androidx.preference.PreferenceManager;

import com.mrmichel.rustboyadvance.EmulatorBindings;
import com.mrmichel.rustboyadvance.Keypad;
import com.mrmichel.rustdroid_emu.R;
import com.mrmichel.rustdroid_emu.Util;
import com.mrmichel.rustdroid_emu.core.AndroidAudioPlayer;
import com.mrmichel.rustdroid_emu.core.Emulator;
import com.mrmichel.rustdroid_emu.core.RomManager;
import com.mrmichel.rustdroid_emu.core.Snapshot;
import com.mrmichel.rustdroid_emu.core.SnapshotManager;
import com.mrmichel.rustdroid_emu.ui.snapshots.SnapshotPickerActivity;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;

public class EmulatorActivity extends AppCompatActivity implements View.OnClickListener, View.OnTouchListener {

    private static final String TAG = "EmulatorActivty";

    private static final String TAG_EMULATOR_STATE = "EmulatorStateFragment";

    private static final int LOAD_ROM_REQUESTCODE = 123;
    private static final int LOAD_SNAPSHOT_REQUESTCODE = 124;

    private Menu menu;

    private RomManager.RomMetadataEntry romMetadata;
    private byte[] bios;
    private EmulationThread emulationThread;
    private AndroidAudioPlayer audioPlayer;
    private byte[] on_resume_saved_state = null;

    /// Fixed recording file path. Lives under the app's external Downloads
    /// so it's easy to pull with adb or share. Users who want multiple
    /// recordings rename the file between sessions.
    private static final String REC_FILE_PATH = "/sdcard/Download/rbarec.rec";
    private boolean isRecording = false;
    private boolean isReplaying = false;

    private Emulator emulator;
    private ScreenView screenView;
    private CompoundButton turboButton;

    private boolean isEmulatorRunning() {
        return emulator.isOpen() && emulationThread != null;
    }

    @Override
    public void onClick(View v) {
        if (v.getId() == R.id.tbTurbo) {
            if (!isEmulatorRunning()) {
                return;
            }
            emulator.setTurbo(((CompoundButton) findViewById(R.id.tbTurbo)).isChecked());
        }
    }

    @Override
    public boolean onTouch(View v, MotionEvent event) {
        Keypad.Key key = null;
        int vid = v.getId();
        if (vid == R.id.bDpadUp) {
            key = Keypad.Key.Up;
        } else if (vid == R.id.bDpadDown) {
            key = Keypad.Key.Down;
        } else if (vid == R.id.bDpadLeft) {
            key = Keypad.Key.Left;
        } else if (vid == R.id.bDpadRight) {
            key = Keypad.Key.Right;
        } else if (vid == R.id.buttonA) {
            key = Keypad.Key.ButtonA;
        } else if (vid == R.id.buttonB) {
            key = Keypad.Key.ButtonB;
        } else if (vid == R.id.buttonL) {
            key = Keypad.Key.ButtonL;
        } else if (vid == R.id.buttonR) {
            key = Keypad.Key.ButtonR;
        } else if (vid == R.id.bStart) {
            key = Keypad.Key.Start;
        } else if (vid == R.id.bSelect) {
            key = Keypad.Key.Select;
        }
        int action = event.getAction();
        if (key != null) {
            if (action == MotionEvent.ACTION_DOWN) {
                v.setPressed(true);
                emulator.keypad.onKeyDown(key);
            } else if (action == MotionEvent.ACTION_UP) {
                v.setPressed(false);
                emulator.keypad.onKeyUp(key);
            } else if (action == MotionEvent.ACTION_OUTSIDE) {
                v.setPressed(false);
                emulator.keypad.onKeyUp(key);
            }
        }

        return true;
    }

    public Keypad.Key keyCodeToGbaKey(int keyCode) {
        switch (keyCode) {
            case KeyEvent.KEYCODE_DPAD_UP:
                return Keypad.Key.Up;
            case KeyEvent.KEYCODE_DPAD_DOWN:
                return Keypad.Key.Down;
            case KeyEvent.KEYCODE_DPAD_LEFT:
                return Keypad.Key.Left;
            case KeyEvent.KEYCODE_DPAD_RIGHT:
                return Keypad.Key.Right;
            case KeyEvent.KEYCODE_Z:
                return Keypad.Key.ButtonB;
            case KeyEvent.KEYCODE_X:
                return Keypad.Key.ButtonA;
            case KeyEvent.KEYCODE_A:
                return Keypad.Key.ButtonL;
            case KeyEvent.KEYCODE_S:
                return Keypad.Key.ButtonR;
            case KeyEvent.KEYCODE_DEL:
                return Keypad.Key.Select;
            case KeyEvent.KEYCODE_COMMA:
                return Keypad.Key.Start;
        }
        return null;
    }

    @Override
    public boolean onKeyLongPress(int keyCode, KeyEvent event) {
        if (!isEmulatorRunning()) {
            return false;
        }
        Keypad.Key key = keyCodeToGbaKey(keyCode);
        Log.d(TAG, "onKeyLongPress(: keyCode = " + keyCode + " GBAKey:" + key);
        if (null != key) {
            this.emulator.keypad.onKeyDown(key);
            return false;
        } else {
            return super.onKeyDown(keyCode, event);
        }
    }

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        if (!isEmulatorRunning()) {
            return false;
        }
        Keypad.Key key = keyCodeToGbaKey(keyCode);
        Log.d(TAG, "onKeyDown: keyCode = " + keyCode + " GBAKey:" + key);
        if (null != key) {
            switch (event.getAction()) {
                case KeyEvent.ACTION_DOWN:
                    this.emulator.keypad.onKeyDown(key);
                    break;
                case KeyEvent.ACTION_UP:
                    this.emulator.keypad.onKeyUp(key);
                    break;
            }
            return event.getAction() == KeyEvent.ACTION_DOWN;
        } else {
            return super.onKeyDown(keyCode, event);
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, @Nullable Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (resultCode == RESULT_OK) {
//            if (requestCode == LOAD_ROM_REQUESTCODE) {
//                Uri uri = data.getData();
//                try {
//                    InputStream inputStream = getContentResolver().openInputStream(uri);
//                    byte[] rom = new byte[inputStream.available()];
//                    inputStream.read(rom);
//                    inputStream.close();
//
//                    String filename = new File(uri.getPath()).getName();
//
//                    File saveRoot = getFilesDir();
//                    String savePath = saveRoot.getAbsolutePath() + "/" + filename + ".sav";
//                    onRomLoaded(rom, savePath);
//                } catch (Exception e) {
//                    Log.e(TAG, "got error while reading rom file");
//                    Util.showAlertDialogAndExit(this, e);
//                }
//          }
            if (requestCode == LOAD_SNAPSHOT_REQUESTCODE) {
                Snapshot pickedSnapshot = SnapshotPickerActivity.obtainPickedSnapshot();

                Toast.makeText(this, "Loading snapshot from " + pickedSnapshot.getTimestamp(), Toast.LENGTH_LONG).show();

                boolean emulatorWasRunning = isEmulatorRunning();

                pauseEmulation();
                try {
                    emulator.loadState(pickedSnapshot.load());
                } catch (Exception e) {
                    Util.showAlertDialogAndExit(this, e);
                }
                resumeEmulation();

                if (!emulatorWasRunning) {
                    createThreads();
                }
            }
        } else {
            Log.e(TAG, "got error for request code " + requestCode);
        }
    }

    private void killThreads() {
        if (emulationThread != null) {
            try {
                emulator.stop();
                emulationThread.join();
            } catch (InterruptedException e) {
                Log.e(TAG, "emulation thread join interrupted");
            }
            emulationThread = null;
        }
    }

    private void createThreads() {
        emulationThread = new EmulationThread(emulator, screenView);
        emulator.setTurbo(turboButton.isChecked());
        emulationThread.start();
    }

    public void onRomLoaded(byte[] rom, String savePath) {
//        killThreads();
//
//        try {
//            emulator.open(bios, rom, savePath);
//        } catch (EmulatorBindings.NativeBindingException e) {
//            Util.showAlertDialogAndExit(this, e);
//        }
//
//        createThreads();
    }

    public void doLoadRom() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.setType("*/*");
        intent.putExtra("android.content.extra.SHOW_ADVANCED", true);
        startActivityForResult(intent, LOAD_ROM_REQUESTCODE);
    }

    @Override
    protected void onSaveInstanceState(@NonNull Bundle outState) {
        super.onSaveInstanceState(outState);

        if (!isEmulatorRunning()) {
            return;
        }
        // save the emulator state
        try {
            byte[] savedState = emulator.saveState();

            File saveFile = new File(getCacheDir(), "saved_state");
            FileOutputStream fis = new FileOutputStream(saveFile);

            fis.write(savedState);

            fis.close();

            outState.putString("saveFile", saveFile.getPath());
            outState.putInt("romId", this.romMetadata.getId());

            outState.putBoolean("turbo", false);

        } catch (Exception e) {
            Util.showAlertDialogAndExit(this, e);
        }
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_emulator);

        this.getWindow().setFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN, WindowManager.LayoutParams.FLAG_FULLSCREEN);
        getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_HIDE_NAVIGATION);

        this.audioPlayer = new AndroidAudioPlayer();

        findViewById(R.id.bStart).setOnTouchListener(this);
        findViewById(R.id.bSelect).setOnTouchListener(this);
        findViewById(R.id.buttonA).setOnTouchListener(this);
        findViewById(R.id.buttonB).setOnTouchListener(this);
        findViewById(R.id.buttonL).setOnTouchListener(this);
        findViewById(R.id.buttonR).setOnTouchListener(this);
        findViewById(R.id.bDpadUp).setOnTouchListener(this);
        findViewById(R.id.bDpadDown).setOnTouchListener(this);
        findViewById(R.id.bDpadLeft).setOnTouchListener(this);
        findViewById(R.id.bDpadRight).setOnTouchListener(this);

        turboButton = findViewById(R.id.tbTurbo);
        turboButton.setOnClickListener(this);

        this.bios = getIntent().getByteArrayExtra("bios");

        this.screenView = findViewById(R.id.gba_view);
        this.emulator = new Emulator(this.screenView, this.audioPlayer);

        final String saveFilePath;

        SharedPreferences sharedPreferences =
                PreferenceManager.getDefaultSharedPreferences(this /* Activity context */);
        boolean skipBios = sharedPreferences.getBoolean("skip_bios", false);

        if (null != savedInstanceState && (saveFilePath = savedInstanceState.getString("saveFile")) != null) {
            final EmulatorActivity thisActivity = this;
            int romId = getIntent().getIntExtra("romId", -1);

            // busy wait until surface view is ready
            try {
                ByteArrayOutputStream outputStream = new ByteArrayOutputStream();

                byte[] buffer = new byte[4096];
                File saveFile = new File(saveFilePath);
                FileInputStream fis = new FileInputStream(saveFile);

                int read = 0;
                while ((read = fis.read(buffer)) != -1) {
                    outputStream.write(buffer);
                }

                fis.close();

                saveFile.delete();

                byte[] savedState = outputStream.toByteArray();
                RomManager romManager = RomManager.getInstance(this);
                romManager.updateLastPlayed(romId);
                this.romMetadata = romManager.getRomMetadata(romId);

                byte[] romData;
                try {
                    romData = Util.readFile(romMetadata.getRomFile());
                } catch (Exception e) {
                    Util.showAlertDialogAndExit(this, e);
                    return;
                }
                emulator.openSavedState(this.bios, romData, savedState);

                createThreads();

                boolean turbo = savedInstanceState.getBoolean("turbo");

                turboButton.setPressed(turbo);
                emulator.setTurbo(turbo);

            } catch (Exception e) {
                Util.showAlertDialogAndExit(thisActivity, e);
            }

        } else {
            int romId = getIntent().getIntExtra("romId", -1);
            if (-1 != romId) {
                RomManager romManager = RomManager.getInstance(this);
                romManager.updateLastPlayed(romId);
                this.romMetadata = romManager.getRomMetadata(romId);

                byte[] romData;
                try {
                    romData = Util.readFile(romMetadata.getRomFile());
                    this.emulator.open(bios, romData, romMetadata.getBackupFile().getAbsolutePath(), skipBios);
                } catch (Exception e) {
                    Util.showAlertDialogAndExit(this, e);
                    return;
                }

                createThreads();
            }
        }
    }

    @Override
    public boolean onCreateOptionsMenu(Menu menu) {
        super.onCreateOptionsMenu(menu);
        this.menu = menu;
        getMenuInflater().inflate(R.menu.menu_emulator, menu);
        return true;
    }

    @Override
    public boolean onOptionsItemSelected(@NonNull MenuItem item) {
        int id = item.getItemId();
        if (id == R.id.action_load_rom) {
            doLoadRom();
            return true;
        } else if (id == R.id.action_view_snapshots) {
            doViewSnapshots();
            return true;
        } else if (id == R.id.action_save_snapshot) {
            doSaveSnapshot();
            return true;
        } else if (id == R.id.action_set_library_image) {
            doSaveScreenshotToLibrary();
            return true;
        } else if (id == R.id.action_record_toggle) {
            toggleRecording(item);
            return true;
        } else if (id == R.id.action_replay_toggle) {
            toggleReplay(item);
            return true;
        } else if (id == R.id.action_settings) {
            Intent intent = new Intent(this, SettingsActivity.class);
            startActivity(intent);
            return true;
        } else {
            return super.onOptionsItemSelected(item);
        }
    }


    @Override
    public boolean onPrepareOptionsMenu(Menu menu) {
        menu.findItem(R.id.action_save_snapshot).setEnabled(isEmulatorRunning());
        return super.onPrepareOptionsMenu(menu);
    }

    private void pauseEmulation() {
        if (null != emulationThread) {
            emulationThread.pauseEmulation();
        }
    }

    private void resumeEmulation() {
        if (null != emulationThread) {
            emulationThread.resumeEmulation();
        }
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        pauseEmulation();

        if (this.romMetadata != null) {
            if (this.romMetadata.getScreenshot() == null) {
                // Save current screenshot
                Bitmap screenshot = Bitmap.createBitmap(
                        emulator.getFrameBuffer(),
                        240,
                        160,
                        Bitmap.Config.RGB_565);

                RomManager.getInstance(this).updateScreenshot(this.romMetadata.getId(), screenshot);

            }
        }
        killThreads();
    }

    @Override
    protected void onPause() {
        super.onPause();
        pauseEmulation();
        screenView.onPause();
    }

    @Override
    protected void onResume() {
        super.onResume();
        screenView.onResume();
        resumeEmulation();
        audioPlayer.play();
    }

    private void toggleRecording(MenuItem item) {
        if (!isEmulatorRunning()) {
            Toast.makeText(this, "No game is running!", Toast.LENGTH_SHORT).show();
            return;
        }
        long ctx = emulator.getCtx();
        if (!isRecording) {
            if (isReplaying) {
                EmulatorBindings.stopReplay(ctx);
                isReplaying = false;
                MenuItem replayItem = menu.findItem(R.id.action_replay_toggle);
                if (replayItem != null) replayItem.setTitle(R.string.action_replay_start);
            }
            EmulatorBindings.startRecording(ctx, REC_FILE_PATH);
            isRecording = true;
            item.setTitle(R.string.action_record_stop);
            Toast.makeText(this, "Recording to " + REC_FILE_PATH, Toast.LENGTH_SHORT).show();
        } else {
            EmulatorBindings.stopRecording(ctx);
            isRecording = false;
            item.setTitle(R.string.action_record_start);
            Toast.makeText(this, "Recording saved", Toast.LENGTH_SHORT).show();
        }
    }

    private void toggleReplay(MenuItem item) {
        if (!isEmulatorRunning()) {
            Toast.makeText(this, "No game is running!", Toast.LENGTH_SHORT).show();
            return;
        }
        long ctx = emulator.getCtx();
        if (!isReplaying) {
            if (isRecording) {
                EmulatorBindings.stopRecording(ctx);
                isRecording = false;
                MenuItem recItem = menu.findItem(R.id.action_record_toggle);
                if (recItem != null) recItem.setTitle(R.string.action_record_start);
            }
            EmulatorBindings.startReplay(ctx, REC_FILE_PATH);
            isReplaying = true;
            item.setTitle(R.string.action_replay_stop);
            Toast.makeText(this, "Replaying " + REC_FILE_PATH, Toast.LENGTH_SHORT).show();
        } else {
            EmulatorBindings.stopReplay(ctx);
            isReplaying = false;
            item.setTitle(R.string.action_replay_start);
            Toast.makeText(this, "Replay stopped", Toast.LENGTH_SHORT).show();
        }
    }

    public void doSaveScreenshotToLibrary() {
        if (!isEmulatorRunning() || null == this.romMetadata) {
            Toast.makeText(this, "No game is running!", Toast.LENGTH_LONG).show();
            return;
        }

        pauseEmulation();

        Bitmap screenshot = Bitmap.createBitmap(
                emulator.getFrameBuffer(),
                240,
                160,
                Bitmap.Config.RGB_565);

        RomManager.getInstance(this).updateScreenshot(this.romMetadata.getId(), screenshot);


        resumeEmulation();
    }

    public void doSaveSnapshot() {
        if (!isEmulatorRunning()) {
            Toast.makeText(this, "No game is running!", Toast.LENGTH_LONG).show();
            return;
        }

        SnapshotManager snapshotManager = SnapshotManager.getInstance(this);

        pauseEmulation();
        try {
            String gameCode = emulator.getGameCode();
            String gameTitle = emulator.getGameTitle();
            byte[] saveState = emulator.saveState();
            Bitmap preview = Bitmap.createBitmap(emulator.getFrameBuffer(), 240, 160, Bitmap.Config.RGB_565);

            snapshotManager.saveSnapshot(gameCode, gameTitle, preview, saveState);
            Toast.makeText(this, "Snapshot saved", Toast.LENGTH_LONG).show();

        } catch (EmulatorBindings.NativeBindingException e) {
            Log.e(TAG, e.toString());
            Util.showAlertDialogAndExit(this, e);
        } finally {
            resumeEmulation();
        }
    }

    public void doViewSnapshots() {
        Intent intent = new Intent(this, SnapshotPickerActivity.class);
        if (emulator.isOpen()) {
            intent.putExtra("gameCode", emulator.getGameCode());
        }
        startActivityForResult(intent, LOAD_SNAPSHOT_REQUESTCODE);
    }

    @Override
    public void onBackPressed() {
        boolean emulatorIsRunning = isEmulatorRunning();

        if (!emulatorIsRunning) {
            super.onBackPressed();
            return;
        }

        new AlertDialog.Builder(this)
                .setIcon(android.R.drawable.ic_dialog_alert)
                .setTitle("Closing Emulator")
                .setCancelable(false)
                .setMessage("Are you sure you want to close the emulator?")
                .setPositiveButton(android.R.string.yes, new DialogInterface.OnClickListener() {
                    @Override
                    public void onClick(DialogInterface dialog, int which) {
                        EmulatorActivity.super.onBackPressed();
                    }
                })
                .setNeutralButton("Yes - but save snapshot", new DialogInterface.OnClickListener() {
                    @Override
                    public void onClick(DialogInterface dialog, int which) {
                        doSaveSnapshot();
                        EmulatorActivity.super.onBackPressed();
                    }
                })
                .setNegativeButton(android.R.string.no, null)
                .show();
    }
}
