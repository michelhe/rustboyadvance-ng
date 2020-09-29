package com.mrmichel.rustdroid_emu.ui;

import android.content.Context;
import android.content.SharedPreferences;
import android.opengl.GLSurfaceView;
import android.util.AttributeSet;

import androidx.preference.PreferenceManager;

import com.mrmichel.rustboyadvance.IFrameRenderer;

public class ScreenView extends GLSurfaceView implements SharedPreferences.OnSharedPreferenceChangeListener, IFrameRenderer {
    private ScreenRenderer mRenderer;

    public ScreenView(Context context) {
        super(context);
        init();
    }

    public ScreenView(Context context, AttributeSet attrs) {
        super(context, attrs);
        init();
    }

    private void init() {
        this.setEGLContextClientVersion(2);
        this.setPreserveEGLContextOnPause(true);

        SharedPreferences sharedPreferences =
                PreferenceManager.getDefaultSharedPreferences(getContext());
        sharedPreferences.registerOnSharedPreferenceChangeListener(this);

        mRenderer = new ScreenRenderer(getContext());
        this.setRenderer(mRenderer);
        this.setRenderMode(RENDERMODE_WHEN_DIRTY);
    }

    public ScreenRenderer getRenderer() {
        return mRenderer;
    }

    @Override
    public void onSharedPreferenceChanged(SharedPreferences sharedPreferences, String key) {
        if (key.equals("color_correction")) {
            boolean colorCorrection = sharedPreferences.getBoolean("color_correction", false);
            mRenderer.setColorCorrection(colorCorrection);
        }
    }

    @Override
    public void renderFrame(int[] frameBuffer) {
        mRenderer.updateTexture(frameBuffer);
        requestRender();
    }
}
