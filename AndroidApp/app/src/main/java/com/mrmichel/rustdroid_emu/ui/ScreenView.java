package com.mrmichel.rustdroid_emu.ui;

import android.content.Context;
import android.opengl.GLSurfaceView;
import android.util.AttributeSet;

public class ScreenView extends GLSurfaceView {
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

        mRenderer = new ScreenRenderer();
        this.setRenderer(mRenderer);
        this.setRenderMode(RENDERMODE_WHEN_DIRTY);
    }

    public void updateFrame(int[] frameBuffer) {
        mRenderer.updateTexture(frameBuffer);
        requestRender();
    }

    public ScreenRenderer getRenderer() {
        return mRenderer;
    }
}
