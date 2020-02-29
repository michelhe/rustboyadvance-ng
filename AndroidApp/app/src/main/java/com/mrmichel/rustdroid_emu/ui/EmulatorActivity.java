package com.mrmichel.rustdroid_emu.ui;

import android.content.DialogInterface;
import android.content.Intent;
import android.graphics.Bitmap;
import android.media.AudioFormat;
import android.media.AudioManager;
import android.media.AudioTrack;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;
import androidx.appcompat.app.AlertDialog;
import androidx.appcompat.app.AppCompatActivity;
import androidx.fragment.app.Fragment;
import androidx.fragment.app.FragmentTransaction;

import android.util.Log;
import android.view.KeyEvent;
import android.view.Menu;
import android.view.MenuItem;
import android.view.MotionEvent;
import android.view.View;
import android.view.WindowManager;
import android.widget.CompoundButton;
import android.widget.Toast;

import com.mrmichel.rustboyadvance.EmulatorBindings;
import com.mrmichel.rustdroid_emu.core.AudioThread;
import com.mrmichel.rustdroid_emu.core.Emulator;
import com.mrmichel.rustdroid_emu.core.Keypad;
import com.mrmichel.rustdroid_emu.R;
import com.mrmichel.rustdroid_emu.core.Snapshot;
import com.mrmichel.rustdroid_emu.core.SnapshotManager;
import com.mrmichel.rustdroid_emu.ui.snapshots.ChosenSnapshot;
import com.mrmichel.rustdroid_emu.ui.snapshots.ISnapshotListener;
import com.mrmichel.rustdroid_emu.ui.snapshots.SnapshotViewerFragment;

import java.io.File;
import java.io.InputStream;

public class EmulatorActivity extends AppCompatActivity implements View.OnClickListener, View.OnTouchListener, ISnapshotListener {

    private static final String TAG = "EmulatorActivty";

    private static final int LOAD_ROM_REQUESTCODE = 123;
    private static final int LOAD_SNAPSHOT_REQUESTCODE = 124;

    private static int SAMPLE_RATE_HZ = 44100;

    private Menu menu;

    private byte[] bios;
    private Emulator emulator = null;
    private EmulationRunnable runnable;
    private Thread emulationThread;
    private AudioThread audioThread;
    private AudioTrack audioTrack;
    private byte[] on_resume_saved_state = null;
    private boolean turboMode = false;
    private GbaScreenView gbaScreenView;

    @Override
    public void onClick(View v) {
        if (v.getId() == R.id.tbTurbo) {
            this.turboMode = ((CompoundButton)findViewById(R.id.tbTurbo)).isChecked();
        }
    }

    @Override
    public boolean onTouch(View v, MotionEvent event) {
        Keypad.Key key = null;
        switch (v.getId()) {
            case R.id.bDpadUp:
                key = Keypad.Key.Up;
                break;
            case R.id.bDpadDown:
                key = Keypad.Key.Down;
                break;
            case R.id.bDpadLeft:
                key = Keypad.Key.Left;
                break;
            case R.id.bDpadRight:
                key = Keypad.Key.Right;
                break;
            case R.id.buttonA:
                key = Keypad.Key.ButtonA;
                break;
            case R.id.buttonB:
                key = Keypad.Key.ButtonB;
                break;
            case R.id.buttonL:
                key = Keypad.Key.ButtonL;
                break;
            case R.id.buttonR:
                key = Keypad.Key.ButtonR;
                break;
            case R.id.bStart:
                key = Keypad.Key.Start;
                break;
            case R.id.bSelect:
                key = Keypad.Key.Select;
                break;
        }
        ;
        int action = event.getAction();
        if (key != null) {
            if (action == MotionEvent.ACTION_DOWN) {
                v.setPressed(true);
                this.emulator.keypad.onKeyDown(key);
            } else if (action == MotionEvent.ACTION_UP) {
                v.setPressed(false);
                this.emulator.keypad.onKeyUp(key);
            }
        }
        return action == MotionEvent.ACTION_DOWN;
    }

    private void showAlertDiaglogAndExit(Exception e) {
        new AlertDialog.Builder(this)
                .setTitle("Exception")
                .setMessage(e.getMessage())
                // Specifying a listener allows you to take an action before dismissing the dialog.
                // The dialog is automatically dismissed when a dialog button is clicked.
                .setPositiveButton(android.R.string.yes, new DialogInterface.OnClickListener() {
                    public void onClick(DialogInterface dialog, int which) {
                        finishAffinity();
                    }
                })
                .setIcon(android.R.drawable.ic_dialog_alert)
                .show();
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, @Nullable Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (resultCode == RESULT_OK) {
            if (requestCode == LOAD_ROM_REQUESTCODE) {
                Uri uri = data.getData();
                try {
                    InputStream inputStream = getContentResolver().openInputStream(uri);
                    byte[] rom = new byte[inputStream.available()];
                    inputStream.read(rom);
                    inputStream.close();

                    String filename = new File(uri.getPath()).getName();

                    File saveRoot = getFilesDir();
                    String savePath = saveRoot.getAbsolutePath() + "/" + filename + ".sav";
                    onRomLoaded(rom, savePath);
                } catch (Exception e) {
                    Log.e(TAG, "got error while reading rom file");
                    showAlertDiaglogAndExit(e);
                }
            }
        } else {
            Log.e(TAG, "got error for request code " + requestCode);
        }
    }

    public void onRomLoaded(byte[] rom, String savePath) {
        if (emulationThread != null) {
            runnable.stop();
            try {
                emulationThread.join();
            } catch (InterruptedException e) {
                Log.e(TAG, "emulation thread join interrupted");
            }
            emulationThread = null;
        }
        if (audioThread != null) {
            audioThread.setStopping(true);
            try {
                audioThread.join();
            } catch (InterruptedException e) {
                Log.e(TAG, "audio thread join interrupted");
            };
            audioThread = null;
        }

        if (emulator.isOpen()) {
            emulator.close();
        }

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
        findViewById(R.id.tbTurbo).setOnClickListener(this);

        menu.findItem(R.id.action_save_snapshot).setEnabled(true);

        try {
            emulator.open(this.bios, rom, savePath);
        } catch (EmulatorBindings.NativeBindingException e) {
            showAlertDiaglogAndExit(e);
        }
        runnable = new EmulationRunnable(this.emulator, this);
        emulationThread = new Thread(runnable);
        emulationThread.start();

        audioThread = new AudioThread(audioTrack, emulator);
        audioThread.start();
    }

    public void doLoadRom() {
        if (runnable != null) {
            runnable.pauseEmulation();
        }
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.setType("*/*");
        intent.putExtra("android.content.extra.SHOW_ADVANCED", true);
        startActivityForResult(intent, LOAD_ROM_REQUESTCODE);
    }

    @Override
    protected void onSaveInstanceState(@NonNull Bundle outState) {
        super.onSaveInstanceState(outState);
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_emulator);

        this.getWindow().setFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN, WindowManager.LayoutParams.FLAG_FULLSCREEN);
        getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_HIDE_NAVIGATION);

        this.bios = getIntent().getByteArrayExtra("bios");
        if (this.emulator == null) {
            this.emulator = new Emulator();
        } else {
            Log.d(TAG, "Orientation was changed");
        }

        if (Build.VERSION.SDK_INT >= 23) {
            AudioTrack.Builder audioTrackBuilder = new AudioTrack.Builder()
                    .setAudioFormat(new AudioFormat.Builder()
                            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                            .setSampleRate(SAMPLE_RATE_HZ)
                            .setChannelMask(AudioFormat.CHANNEL_IN_STEREO | AudioFormat.CHANNEL_OUT_STEREO)
                            .build()
                    )
                    .setBufferSizeInBytes(4096)
                    .setTransferMode(AudioTrack.MODE_STREAM);
            if (Build.VERSION.SDK_INT >= 26) {
                audioTrackBuilder.setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY);
            }
            this.audioTrack = audioTrackBuilder.build();
        } else {
            this.audioTrack = new AudioTrack(
                    AudioManager.STREAM_MUSIC,
                    SAMPLE_RATE_HZ,
                    AudioFormat.CHANNEL_IN_STEREO | AudioFormat.CHANNEL_OUT_STEREO,
                    AudioFormat.ENCODING_PCM_16BIT,
                    4096,
                    AudioTrack.MODE_STREAM);
        }
        this.audioTrack.play();

        this.gbaScreenView = findViewById(R.id.gba_view);
    }

    @Override
    public boolean onCreateOptionsMenu(Menu menu) {
        super.onCreateOptionsMenu(menu);
        getMenuInflater().inflate(R.menu.menu_emulator, menu);
        this.menu = menu;
        return true;
    }

    @Override
    public boolean onOptionsItemSelected(@NonNull MenuItem item) {
        switch (item.getItemId()) {
            case R.id.action_load_rom:
                doLoadRom();
                return true;
            case R.id.action_view_snapshots:
                doViewSnapshots();
                return true;
            case R.id.action_save_snapshot:
                doSaveSnapshot();
                return true;
            default:
                return super.onOptionsItemSelected(item);
        }
    }

    @Override
    protected void onPause() {
        super.onPause();
        audioTrack.stop();
        if (emulator.isOpen()) {
            if (runnable != null) {
                runnable.pauseEmulation();
            }
            Log.d(TAG, "onPause - saving emulator state");
//            try {
//                on_resume_saved_state = emulator.saveState();
//            } catch (EmulatorBindings.NativeBindingException e) {
//                showAlertDiaglogAndExit(e);
//            }
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (emulator.isOpen()) {
            Log.d(TAG, "onResume - loading emulator state");
//            try {
//                emulator.loadState(on_resume_saved_state);
//            } catch (EmulatorBindings.NativeBindingException e) {
//                showAlertDiaglogAndExit(e);
//            }
//            on_resume_saved_state = null;
            if (runnable != null) {
                runnable.resumeEmulation();
            }
        }
        audioTrack.play();
    }

    public void doSaveSnapshot() {
        if (!emulator.isOpen()) {
            Toast.makeText(this, "No game is running!", Toast.LENGTH_LONG).show();
            return;
        }

        SnapshotManager snapshotManager = SnapshotManager.getInstance(this);

        runnable.pauseEmulation();
        try {
            String gameCode = emulator.getGameCode();
            String gameTitle = emulator.getGameTitle();
            byte[] saveState = emulator.saveState();
            Bitmap preview = Bitmap.createBitmap(emulator.getFrameBuffer(), 240, 160, Bitmap.Config.RGB_565);

            snapshotManager.saveSnapshot(gameCode, gameTitle, preview, saveState);
            Toast.makeText(this, "Snapshot saved", Toast.LENGTH_LONG).show();

        } catch (EmulatorBindings.NativeBindingException e) {
            Log.e(TAG, e.toString());
            showAlertDiaglogAndExit(e);
        } finally {
            runnable.resumeEmulation();
        }
    }

    public void doViewSnapshots() {
//        Intent intent = new Intent(this, SnapshotViewerFragment.class);
//        startActivityForResult(intent, LOAD_SNAPSHOT_REQUESTCODE);

        SnapshotViewerFragment fragment = new SnapshotViewerFragment(this);

        Bundle args = new Bundle();
        if (emulator.isOpen()) {
            args.putString("gameCode", emulator.getGameCode());
            this.runnable.pauseEmulation();
        }
        fragment.setArguments(args);

        FragmentTransaction transaction = getSupportFragmentManager().beginTransaction();

        transaction.add(R.id.fragment_container, fragment, "fragment_snapshot_viewer");
        transaction.addToBackStack(null);

        transaction.commit();

    }

    @Override
    public void onSnapshotClicked(Snapshot snapshot) {
        Fragment f = getSupportFragmentManager().findFragmentByTag("fragment_snapshot_viewer");
        FragmentTransaction transaction = getSupportFragmentManager().beginTransaction();
        transaction.remove(f);
        transaction.commit();

        byte[] state = snapshot.load();
        if (emulator.isOpen()) {
            try {
                emulator.loadState(state);
                this.runnable.resumeEmulation();
            } catch (EmulatorBindings.NativeBindingException e) {
                showAlertDiaglogAndExit(e);
            }
        }
    }

    private class EmulationRunnable implements Runnable {

        public static final long NANOSECONDS_PER_MILLISECOND = 1000000;
        public static final long FRAME_TIME = 1000000000 / 60;

        EmulatorActivity emulatorActivity;
        Emulator emulator;
        boolean running;
        boolean stopping;

        public EmulationRunnable(Emulator emulator, EmulatorActivity emulatorActivity) {
            this.emulator = emulator;
            this.emulatorActivity = emulatorActivity;
            resumeEmulation();
        }

        private void emulate() {
            long startTimer = System.nanoTime();
            emulator.runFrame();
            if (!emulatorActivity.turboMode) {
                long currentTime = System.nanoTime();
                long timePassed = currentTime - startTimer;

                long delay = FRAME_TIME - timePassed;
                if (delay > 0) {
                    try {
                        Thread.sleep(delay / NANOSECONDS_PER_MILLISECOND);
                    } catch (Exception e) {

                    }
                }
            }

            emulatorActivity.gbaScreenView.updateFrame(emulator.getFrameBuffer());
        }

        public void pauseEmulation() {
            running = false;
        }

        public void resumeEmulation() {
            running = true;
        }

        public void stop() {
            stopping = true;
        }

        @Override
        public void run() {
            while (!stopping) {
                if (running) {
                    emulate();
                }
            }
        }
    }
}
