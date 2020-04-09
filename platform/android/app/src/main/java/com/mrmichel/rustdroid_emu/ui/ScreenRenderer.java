package com.mrmichel.rustdroid_emu.ui;

import android.content.Context;
import android.content.SharedPreferences;
import android.graphics.Bitmap;
import android.opengl.GLES20;
import android.opengl.GLSurfaceView;
import android.opengl.GLUtils;

import androidx.preference.PreferenceManager;

import com.mrmichel.rustdroid_emu.R;

import org.apache.commons.io.IOUtils;

import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.FloatBuffer;
import java.nio.charset.StandardCharsets;

import javax.microedition.khronos.egl.EGLConfig;
import javax.microedition.khronos.opengles.GL10;

public class ScreenRenderer implements GLSurfaceView.Renderer {

    private ScreenTexture texture;
    private boolean ready = false;
    private Context context;

    public ScreenRenderer(Context context) {
        this.context = context;
    }

    public void updateTexture(int[] frameBuffer) {
        this.texture.update(frameBuffer);
    }

    public void initTextureIfNotInitialized() {
        if (this.texture == null) {
            this.texture = new ScreenTexture(this.context);
        }
    }

    @Override
    public void onSurfaceCreated(GL10 gl, EGLConfig config) {
        initTextureIfNotInitialized();
        ready = true;
    }

    @Override
    public void onSurfaceChanged(GL10 gl, int width, int height) {
        gl.glViewport(0, 0, width, height);
    }

    @Override
    public void onDrawFrame(GL10 gl) {
        this.texture.render();
    }

    public boolean isReady() {
        return ready;
    }

    public void setColorCorrection(boolean colorCorrection) {
        this.texture.setColorCorrection(colorCorrection);
    }

    /**
     * Private class to manage the screen texture rendering
     */
    private class ScreenTexture {
        int normalShaderProgram;
        int colorCorrectionShaderProgram;
        int currentShaderProgram;
        int positionHandle;
        int texCoordHandle;
        int samplerHandle;
        int textureId;

        private FloatBuffer vertexBuffer;
        private FloatBuffer textureBuffer;
        private ByteBuffer indicesBuffer;

        private Bitmap bitmap;
        private Context context;

        // square vertices
        private float[] vertices = {
                -1.0f, 1.0f, 0.0f,      // top left
                -1.0f, -1.0f, 0.0f,    // bottom left
                1.0f, -1.0f, 0.0f,     // bottom right
                1.0f, 1.0f, 0.0f,     // top right
        };

        // texture space vertices
        private float[] textureVertices = {
                0.0f, 0.0f,
                0.0f, 1.0f,
                1.0f, 1.0f,
                1.0f, 0.0f
        };

        // two triangles compose a rect
        private byte[] indicies = {
                0, 1, 2,
                0, 2, 3
        };

        public ScreenTexture(Context context) {
            this.context = context;
            this.bitmap = Bitmap.createBitmap(240, 160, Bitmap.Config.RGB_565);

            GLES20.glEnable(GLES20.GL_TEXTURE_2D);

            // create vertex array
            vertexBuffer = ByteBuffer.allocateDirect(vertices.length * 4).order(ByteOrder.nativeOrder()).asFloatBuffer();
            vertexBuffer.put(vertices);
            vertexBuffer.position(0);

            // create texture coordinate array
            textureBuffer = ByteBuffer.allocateDirect(textureVertices.length * 4).order(ByteOrder.nativeOrder()).asFloatBuffer();
            textureBuffer.put(textureVertices);
            textureBuffer.position(0);

            // create triangle index array
            indicesBuffer = ByteBuffer.allocateDirect(indicies.length).order(ByteOrder.nativeOrder());
            indicesBuffer.put(indicies);
            indicesBuffer.position(0);

            textureId = createTexture();

            String vertexShader = readShaderResource(R.raw.screen_texture_vertex_shader);
            String normalFragmentShader = readShaderResource(R.raw.screen_texture_fragment_shader);
            String colorCorrectionFragmentShader = readShaderResource(R.raw.screen_texture_color_correction_fragment_shader);

            normalShaderProgram = createShaderProgram(vertexShader, normalFragmentShader);
            colorCorrectionShaderProgram = createShaderProgram(vertexShader, colorCorrectionFragmentShader);

            SharedPreferences sharedPreferences = PreferenceManager.getDefaultSharedPreferences(this.context);
            boolean colorCorrection = sharedPreferences.getBoolean("color_correction", false);
            setColorCorrection(colorCorrection);

            GLES20.glClearColor(1.0f, 1.0f, 1.0f, 1.0f);
        }

        private void setColorCorrection(boolean colorCorrection) {

            if (colorCorrection) {
                currentShaderProgram = colorCorrectionShaderProgram;
            } else {
                currentShaderProgram = normalShaderProgram;
            }

            // use the program
            GLES20.glUseProgram(currentShaderProgram);

            positionHandle = GLES20.glGetAttribLocation(currentShaderProgram, "a_position");

            texCoordHandle = GLES20.glGetAttribLocation(currentShaderProgram, "a_texCoord");

            samplerHandle = GLES20.glGetUniformLocation(currentShaderProgram, "s_texture");


            // load the vertex position
            GLES20.glVertexAttribPointer(positionHandle, 3, GLES20.GL_FLOAT, false, 0, vertexBuffer);
            GLES20.glEnableVertexAttribArray(positionHandle);
            // load texture coordinate
            GLES20.glVertexAttribPointer(texCoordHandle, 2, GLES20.GL_FLOAT, false, 0, textureBuffer);
            GLES20.glEnableVertexAttribArray(texCoordHandle);


            GLES20.glVertexAttribPointer(positionHandle, 3, GLES20.GL_FLOAT, false, 0, vertexBuffer);
            GLES20.glEnableVertexAttribArray(positionHandle);
            // load texture coordinate
            GLES20.glVertexAttribPointer(texCoordHandle, 2, GLES20.GL_FLOAT, false, 0, textureBuffer);
            GLES20.glEnableVertexAttribArray(texCoordHandle);
        }

        private int compileShader(int type, String code) {
            int shader = GLES20.glCreateShader(type);
            GLES20.glShaderSource(shader, code);
            GLES20.glCompileShader(shader);
            return shader;
        }

        private void update(int[] frameBuffer) {
            bitmap.setPixels(frameBuffer, 0, 240, 0, 0, 240, 160);
        }

        private int createShaderProgram(String vertexShaderCode, String fragmentShaderCode) {
            int vertexShader = compileShader(GLES20.GL_VERTEX_SHADER, vertexShaderCode);
            int fragmentShader = compileShader(GLES20.GL_FRAGMENT_SHADER, fragmentShaderCode);

            int program = GLES20.glCreateProgram();
            GLES20.glAttachShader(program, vertexShader);
            GLES20.glAttachShader(program, fragmentShader);

            GLES20.glLinkProgram(program);

            return program;
        }

        private int createTexture() {
            int[] texturesIds = new int[1];

            GLES20.glGenTextures(1, texturesIds, 0);
            if (texturesIds[0] == GLES20.GL_FALSE) {
                throw new RuntimeException("Error loading texture");
            }
            // bind the texture
            GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, texturesIds[0]);

            GLUtils.texImage2D(GLES20.GL_TEXTURE_2D, 0, bitmap, 0);

            // set the parameters
            GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MIN_FILTER, GLES20.GL_LINEAR);
            GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MAG_FILTER, GLES20.GL_LINEAR);
            GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_S, GLES20.GL_CLAMP_TO_EDGE);
            GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_T, GLES20.GL_CLAMP_TO_EDGE);

            GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, 0);

            return texturesIds[0];
        }

        private String readShaderResource(int resourceId) {
            InputStream in = context.getResources().openRawResource(resourceId);
            String code;
            try {
                code = IOUtils.toString(in, StandardCharsets.UTF_8);
            } catch (IOException e) {
                code = "";
            }
            return code;
        }

        protected void destroy() {
            GLES20.glDeleteProgram(normalShaderProgram);
            GLES20.glDeleteProgram(colorCorrectionShaderProgram);
            int[] textures = {textureId};
            GLES20.glDeleteTextures(1, textures, 0);
        }

        public void render() {
            // use the shader program
            GLES20.glUseProgram(currentShaderProgram);

            // clear the color buffer
            GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT);

            // bind the texture
            GLES20.glActiveTexture(GLES20.GL_TEXTURE0);
            GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, textureId);

            GLUtils.texImage2D(GLES20.GL_TEXTURE_2D, 0, bitmap, 0);

            // Set the sampler texture unit to 0
            GLES20.glUniform1i(samplerHandle, 0);

            GLES20.glDrawElements(GLES20.GL_TRIANGLE_STRIP, 6, GLES20.GL_UNSIGNED_BYTE, indicesBuffer);
        }
    }
}
