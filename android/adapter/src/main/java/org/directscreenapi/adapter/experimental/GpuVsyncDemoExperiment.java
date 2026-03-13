package org.directscreenapi.adapter;

import android.opengl.EGL14;
import android.opengl.EGLConfig;
import android.opengl.EGLContext;
import android.opengl.EGLDisplay;
import android.opengl.EGLSurface;
import android.opengl.GLES20;
import android.os.Looper;
import android.view.Choreographer;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.FloatBuffer;
import java.util.Locale;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

final class GpuVsyncDemoExperiment {
    private static final long STARTUP_TIMEOUT_MS = 6_000L;
    private static final long STOP_WAIT_MS = 1_600L;

    private final int requestedWidth;
    private final int requestedHeight;
    private final int zLayer;
    private final String layerName;
    private final float runSeconds;
    private final Object lock = new Object();

    private SurfaceLayerSession surfaceSession;
    private RenderThread renderThread;
    private int activeWidth;
    private int activeHeight;
    private float activeRefreshHz;
    private boolean running;

    GpuVsyncDemoExperiment(int requestedWidth, int requestedHeight, int zLayer, String layerName, float runSeconds) {
        this.requestedWidth = requestedWidth;
        this.requestedHeight = requestedHeight;
        this.zLayer = zLayer;
        this.layerName = layerName == null || layerName.trim().isEmpty()
                ? "DirectScreenAPI-GPU"
                : layerName.trim();
        this.runSeconds = runSeconds;
    }

    void runLoop() throws Exception {
        start();
        Throwable runtimeError = null;
        try {
            RenderThread thread;
            synchronized (lock) {
                thread = renderThread;
            }
            if (thread != null) {
                thread.awaitFinished();
                runtimeError = thread.runtimeError();
            }
        } finally {
            stop();
        }
        if (runtimeError != null) {
            throw new RuntimeException(runtimeError);
        }
    }

    void start() throws Exception {
        synchronized (lock) {
            if (running && renderThread != null && renderThread.isRenderActive()) {
                return;
            }

            DisplayAdapter.DisplaySnapshot snapshot = new AndroidDisplayAdapter().queryDisplaySnapshot();
            int displayW = Math.max(1, snapshot.width);
            int displayH = Math.max(1, snapshot.height);
            int targetW = requestedWidth > 0 ? Math.min(requestedWidth, displayW) : displayW;
            int targetH = requestedHeight > 0 ? Math.min(requestedHeight, displayH) : displayH;
            float targetFrameRate = snapshot.refreshHz > 1.0f ? snapshot.refreshHz : 60.0f;

            SurfaceLayerSession createdSurface = null;
            RenderThread createdThread = null;
            try {
                createdSurface = SurfaceLayerSession.create(
                        targetW,
                        targetH,
                        zLayer,
                        layerName,
                        true,
                        0,
                        targetFrameRate
                );
                createdThread = new RenderThread(
                        createdSurface.surfaceObject(),
                        targetW,
                        targetH,
                        runSeconds
                );
                createdThread.start();
                createdThread.awaitStartup();
            } catch (Throwable t) {
                if (createdThread != null) {
                    createdThread.requestStop();
                    createdThread.awaitFinishedQuietly(STOP_WAIT_MS);
                }
                if (createdSurface != null) {
                    createdSurface.closeQuietly();
                }
                throw t;
            }

            surfaceSession = createdSurface;
            renderThread = createdThread;
            activeWidth = targetW;
            activeHeight = targetH;
            activeRefreshHz = targetFrameRate;
            running = true;

            System.out.println(String.format(
                    Locale.US,
                    "gpu_demo_status=running size=%dx%d refresh_hz=%.2f layer=%s run_seconds=%.2f",
                    targetW,
                    targetH,
                    targetFrameRate,
                    layerName,
                    runSeconds
            ));
        }
    }

    void stop() {
        SurfaceLayerSession oldSurface;
        RenderThread oldThread;
        boolean hadRuntime;
        synchronized (lock) {
            oldSurface = surfaceSession;
            oldThread = renderThread;
            hadRuntime = running || oldSurface != null || oldThread != null;
            surfaceSession = null;
            renderThread = null;
            running = false;
            activeWidth = 0;
            activeHeight = 0;
            activeRefreshHz = 0.0f;
        }

        if (oldThread != null) {
            oldThread.requestStop();
            oldThread.awaitFinishedQuietly(STOP_WAIT_MS);
        }
        if (oldSurface != null) {
            oldSurface.closeQuietly();
        }
        if (hadRuntime) {
            System.out.println("gpu_demo_status=stopped");
        }
    }

    boolean isRunning() {
        synchronized (lock) {
            return running && renderThread != null && renderThread.isRenderActive();
        }
    }

    String statusLine() {
        synchronized (lock) {
            if (!running || renderThread == null || !renderThread.isRenderActive()) {
                return "state=stopped mode=gpu_demo";
            }
            return String.format(
                    Locale.US,
                    "state=running mode=gpu_demo size=%dx%d refresh_hz=%.2f layer=%s",
                    activeWidth,
                    activeHeight,
                    activeRefreshHz,
                    sanitizeToken(layerName)
            );
        }
    }

    String command(String rawCommand) throws Exception {
        String cmd = rawCommand == null ? "" : rawCommand.trim();
        if (cmd.isEmpty()) {
            throw new IllegalArgumentException("gpu_demo_cmd_empty");
        }
        String[] parts = cmd.split("\\s+");
        String op = parts[0].toUpperCase(Locale.US);

        if ("PING".equals(op)) {
            return "pong";
        }
        if ("STATUS".equals(op)) {
            return statusLine();
        }
        if ("STOP".equals(op)) {
            stop();
            return "stopped";
        }

        SurfaceLayerSession session;
        RenderThread thread;
        synchronized (lock) {
            session = surfaceSession;
            thread = renderThread;
        }
        if (session == null || thread == null || !thread.isRenderActive()) {
            throw new IllegalStateException("gpu_demo_not_running");
        }

        if ("SET_POS".equals(op)) {
            if (parts.length < 3) {
                throw new IllegalArgumentException("gpu_demo_set_pos_args_missing");
            }
            int x = parseInt(parts[1], 0);
            int y = parseInt(parts[2], 0);
            session.setPosition((float) x, (float) y);
            return "ok set_pos";
        }

        if ("SET_VIEW".equals(op)) {
            if (parts.length < 3) {
                throw new IllegalArgumentException("gpu_demo_set_view_args_missing");
            }
            int w = parseInt(parts[1], 1);
            int h = parseInt(parts[2], 1);
            int safeW = Math.max(1, w);
            int safeH = Math.max(1, h);
            session.setWindowCrop(0, 0, safeW, safeH);
            thread.setViewport(safeW, safeH);
            synchronized (lock) {
                activeWidth = safeW;
                activeHeight = safeH;
            }
            return "ok set_view";
        }

        if ("SET_COLOR".equals(op)) {
            if (parts.length < 4) {
                throw new IllegalArgumentException("gpu_demo_set_color_args_missing");
            }
            float r = parseColor(parts[1]);
            float g = parseColor(parts[2]);
            float b = parseColor(parts[3]);
            float a = parts.length >= 5 ? parseColor(parts[4]) : 1.0f;
            thread.setTint(r, g, b, a);
            return "ok set_color";
        }

        if ("SET_SPEED".equals(op)) {
            if (parts.length < 2) {
                throw new IllegalArgumentException("gpu_demo_set_speed_args_missing");
            }
            float speed = parseFloat(parts[1], 1.0f);
            if (!Float.isFinite(speed)) {
                speed = 1.0f;
            }
            speed = Math.max(0.0f, Math.min(8.0f, speed));
            thread.setSpeed(speed);
            return "ok set_speed";
        }

        throw new IllegalArgumentException("gpu_demo_cmd_unknown");
    }

    private static int parseInt(String text, int fallback) {
        try {
            return Integer.parseInt(text);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static float parseFloat(String text, float fallback) {
        try {
            return Float.parseFloat(text);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static float parseColor(String text) {
        float v = parseFloat(text, 255.0f);
        if (!Float.isFinite(v)) {
            return 1.0f;
        }
        if (v <= 1.0f) {
            return Math.max(0.0f, v);
        }
        return Math.max(0.0f, Math.min(255.0f, v)) / 255.0f;
    }

    private static String sanitizeToken(String raw) {
        if (raw == null || raw.trim().isEmpty()) {
            return "-";
        }
        return raw.trim()
                .replace('\n', '_')
                .replace('\r', '_')
                .replace('\t', '_')
                .replace(' ', '_');
    }

    private static final class RenderThread extends Thread implements Choreographer.FrameCallback {
        private final Object windowSurfaceObject;
        private final int width;
        private final int height;
        private final float runSeconds;
        private final CountDownLatch startupLatch = new CountDownLatch(1);
        private final CountDownLatch finishedLatch = new CountDownLatch(1);
        private final GlesSceneRenderer renderer;

        private volatile Throwable startupError;
        private volatile Throwable runtimeError;
        private volatile boolean running = true;
        private volatile boolean finished;
        private volatile Looper looper;
        private Choreographer choreographer;
        private long startedAtNs = -1L;
        private long lastLogNs = -1L;
        private long framesSinceLog = 0L;

        RenderThread(Object windowSurfaceObject, int width, int height, float runSeconds) {
            super("dsapi-gpu-vsync-demo");
            this.windowSurfaceObject = windowSurfaceObject;
            this.width = width;
            this.height = height;
            this.runSeconds = runSeconds;
            this.renderer = new GlesSceneRenderer(windowSurfaceObject, width, height);
        }

        @Override
        public void run() {
            try {
                Looper.prepare();
                looper = Looper.myLooper();
                choreographer = Choreographer.getInstance();
                renderer.init();
            } catch (Throwable t) {
                startupError = t;
                startupLatch.countDown();
                finished = true;
                finishedLatch.countDown();
                return;
            }

            startupLatch.countDown();
            choreographer.postFrameCallback(this);
            try {
                Looper.loop();
            } catch (Throwable t) {
                runtimeError = t;
            } finally {
                renderer.releaseQuietly();
                finished = true;
                finishedLatch.countDown();
            }
        }

        @Override
        public void doFrame(long frameTimeNanos) {
            if (!running) {
                quitLooper();
                return;
            }
            try {
                if (startedAtNs < 0L) {
                    startedAtNs = frameTimeNanos;
                    lastLogNs = frameTimeNanos;
                }

                float timeSec = (frameTimeNanos - startedAtNs) * 1.0e-9f;
                renderer.draw(timeSec);
                framesSinceLog++;

                long logWindowNs = frameTimeNanos - lastLogNs;
                if (logWindowNs >= 1_000_000_000L) {
                    double fps = (framesSinceLog * 1_000_000_000.0d) / (double) logWindowNs;
                    double frameMs = fps > 0.0001d ? 1000.0d / fps : 0.0d;
                    System.out.println(String.format(
                            Locale.US,
                            "gpu_demo_perf fps=%.1f frame_ms=%.2f",
                            fps,
                            frameMs
                    ));
                    framesSinceLog = 0L;
                    lastLogNs = frameTimeNanos;
                }

                if (runSeconds > 0.0f && timeSec >= runSeconds) {
                    running = false;
                    quitLooper();
                    return;
                }
            } catch (Throwable t) {
                runtimeError = t;
                running = false;
                quitLooper();
                return;
            }
            if (running && choreographer != null) {
                choreographer.postFrameCallback(this);
            }
        }

        void awaitStartup() throws Exception {
            if (!startupLatch.await(STARTUP_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
                throw new IllegalStateException("gpu_demo_start_timeout");
            }
            if (startupError != null) {
                throw new RuntimeException(startupError);
            }
        }

        void awaitFinished() throws InterruptedException {
            finishedLatch.await();
        }

        void awaitFinishedQuietly(long timeoutMs) {
            try {
                finishedLatch.await(timeoutMs, TimeUnit.MILLISECONDS);
            } catch (InterruptedException ignored) {
            }
        }

        Throwable runtimeError() {
            return runtimeError;
        }

        boolean isRenderActive() {
            return !finished && startupError == null;
        }

        void requestStop() {
            running = false;
            quitLooper();
        }

        void setTint(float r, float g, float b, float a) {
            renderer.setTint(r, g, b, a);
        }

        void setSpeed(float speedScale) {
            renderer.setSpeed(speedScale);
        }

        void setViewport(int w, int h) {
            renderer.setViewport(w, h);
        }

        private void quitLooper() {
            Looper l = looper;
            if (l != null) {
                l.quitSafely();
            }
        }
    }

    private static final class GlesSceneRenderer {
        private static final String VS = ""
                + "attribute vec2 aPos;\n"
                + "varying vec2 vUv;\n"
                + "void main() {\n"
                + "  vUv = aPos * 0.5 + 0.5;\n"
                + "  gl_Position = vec4(aPos, 0.0, 1.0);\n"
                + "}\n";
        private static final String FS = ""
                + "precision mediump float;\n"
                + "varying vec2 vUv;\n"
                + "uniform float uTime;\n"
                + "uniform vec4 uTint;\n"
                + "float sdRoundRect(vec2 p, vec2 b, float r) {\n"
                + "  vec2 q = abs(p) - b + vec2(r);\n"
                + "  return length(max(q, 0.0)) - r + min(max(q.x, q.y), 0.0);\n"
                + "}\n"
                + "void main() {\n"
                + "  vec2 p = vUv - 0.5;\n"
                + "  float panelSdf = sdRoundRect(p, vec2(0.46, 0.42), 0.045);\n"
                + "  float panel = 1.0 - smoothstep(0.0, 0.006, panelSdf);\n"
                + "  float shadowSdf = sdRoundRect(p + vec2(0.0, 0.012), vec2(0.46, 0.42), 0.05);\n"
                + "  float shadow = (1.0 - smoothstep(0.0, 0.08, shadowSdf)) * 0.22;\n"
                + "  float titleMask = panel * step(0.74, vUv.y);\n"
                + "  float borderInner = 1.0 - smoothstep(0.0, 0.006, sdRoundRect(p, vec2(0.445, 0.405), 0.038));\n"
                + "  float borderMask = clamp(panel - borderInner, 0.0, 1.0);\n"
                + "  float closeDot = panel * (1.0 - smoothstep(0.022, 0.026, length(vUv - vec2(0.88, 0.84))));\n"
                + "  float actionLine = panel * smoothstep(0.738, 0.742, vUv.y) * (1.0 - step(0.758, vUv.y));\n"
                + "  float highlight = panel * smoothstep(0.80, 0.98, vUv.y) * (0.88 + 0.12 * sin(uTime * 0.9 + vUv.x * 6.28318));\n"
                + "  vec3 bodyTop = vec3(0.97, 0.975, 0.985);\n"
                + "  vec3 bodyBottom = vec3(0.93, 0.94, 0.955);\n"
                + "  float bodyT = clamp((vUv.y - 0.08) / 0.66, 0.0, 1.0);\n"
                + "  vec3 bodyColor = mix(bodyBottom, bodyTop, bodyT);\n"
                + "  vec3 titleColor = vec3(0.15, 0.17, 0.21);\n"
                + "  vec3 borderColor = vec3(0.58, 0.62, 0.70);\n"
                + "  vec3 bgColor = vec3(0.03, 0.04, 0.06);\n"
                + "  vec3 color = bgColor * (1.0 - panel);\n"
                + "  color = mix(color, bodyColor, panel);\n"
                + "  color = mix(color, titleColor, titleMask);\n"
                + "  color += vec3(0.02, 0.03, 0.05) * actionLine;\n"
                + "  color += vec3(0.03, 0.04, 0.06) * highlight;\n"
                + "  color = mix(color, borderColor, borderMask);\n"
                + "  color = mix(color, vec3(0.93, 0.33, 0.34), closeDot);\n"
                + "  vec3 tintMix = mix(vec3(1.0), clamp(uTint.rgb, 0.0, 1.0), 0.20);\n"
                + "  color *= tintMix;\n"
                + "  float alpha = max(shadow, panel * (0.97 * clamp(uTint.a, 0.0, 1.0)));\n"
                + "  gl_FragColor = vec4(color, alpha);\n"
                + "}\n";

        private final Object windowSurfaceObject;
        private final int width;
        private final int height;
        private final FloatBuffer quadBuffer;

        private EGLDisplay eglDisplay = EGL14.EGL_NO_DISPLAY;
        private EGLContext eglContext = EGL14.EGL_NO_CONTEXT;
        private EGLSurface eglSurface = EGL14.EGL_NO_SURFACE;
        private int programId;
        private int attrPos;
        private int uniformTime;
        private int uniformTint;
        private volatile float tintR = 1.0f;
        private volatile float tintG = 1.0f;
        private volatile float tintB = 1.0f;
        private volatile float tintA = 1.0f;
        private volatile float speedScale = 1.0f;
        private volatile int viewportW;
        private volatile int viewportH;

        GlesSceneRenderer(Object windowSurfaceObject, int width, int height) {
            this.windowSurfaceObject = windowSurfaceObject;
            this.width = width;
            this.height = height;
            this.viewportW = width;
            this.viewportH = height;
            this.quadBuffer = ByteBuffer
                    .allocateDirect(4 * 2 * 4)
                    .order(ByteOrder.nativeOrder())
                    .asFloatBuffer();
            this.quadBuffer.put(new float[]{
                    -1.0f, -1.0f,
                    1.0f, -1.0f,
                    -1.0f, 1.0f,
                    1.0f, 1.0f
            });
            this.quadBuffer.position(0);
        }

        void init() {
            eglDisplay = EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY);
            if (eglDisplay == EGL14.EGL_NO_DISPLAY) {
                throw new IllegalStateException("gpu_demo_egl_no_display");
            }
            int[] version = new int[2];
            if (!EGL14.eglInitialize(eglDisplay, version, 0, version, 1)) {
                throw new IllegalStateException("gpu_demo_egl_init_failed err=" + hex(EGL14.eglGetError()));
            }

            int[] configAttrs = new int[]{
                    EGL14.EGL_RENDERABLE_TYPE, EGL14.EGL_OPENGL_ES2_BIT,
                    EGL14.EGL_SURFACE_TYPE, EGL14.EGL_WINDOW_BIT,
                    EGL14.EGL_RED_SIZE, 8,
                    EGL14.EGL_GREEN_SIZE, 8,
                    EGL14.EGL_BLUE_SIZE, 8,
                    EGL14.EGL_ALPHA_SIZE, 8,
                    EGL14.EGL_NONE
            };
            EGLConfig[] configs = new EGLConfig[1];
            int[] numConfig = new int[1];
            if (!EGL14.eglChooseConfig(eglDisplay, configAttrs, 0, configs, 0, configs.length, numConfig, 0)
                    || numConfig[0] <= 0
                    || configs[0] == null) {
                throw new IllegalStateException("gpu_demo_egl_choose_config_failed err=" + hex(EGL14.eglGetError()));
            }

            int[] contextAttrs = new int[]{
                    EGL14.EGL_CONTEXT_CLIENT_VERSION, 2,
                    EGL14.EGL_NONE
            };
            eglContext = EGL14.eglCreateContext(
                    eglDisplay,
                    configs[0],
                    EGL14.EGL_NO_CONTEXT,
                    contextAttrs,
                    0
            );
            if (eglContext == null || eglContext == EGL14.EGL_NO_CONTEXT) {
                throw new IllegalStateException("gpu_demo_egl_create_context_failed err=" + hex(EGL14.eglGetError()));
            }

            int[] surfaceAttrs = new int[]{EGL14.EGL_NONE};
            eglSurface = EGL14.eglCreateWindowSurface(
                    eglDisplay,
                    configs[0],
                    windowSurfaceObject,
                    surfaceAttrs,
                    0
            );
            if (eglSurface == null || eglSurface == EGL14.EGL_NO_SURFACE) {
                throw new IllegalStateException("gpu_demo_egl_create_surface_failed err=" + hex(EGL14.eglGetError()));
            }
            if (!EGL14.eglMakeCurrent(eglDisplay, eglSurface, eglSurface, eglContext)) {
                throw new IllegalStateException("gpu_demo_egl_make_current_failed err=" + hex(EGL14.eglGetError()));
            }

            int vs = compileShader(GLES20.GL_VERTEX_SHADER, VS);
            int fs = compileShader(GLES20.GL_FRAGMENT_SHADER, FS);
            programId = GLES20.glCreateProgram();
            GLES20.glAttachShader(programId, vs);
            GLES20.glAttachShader(programId, fs);
            GLES20.glLinkProgram(programId);
            int[] linked = new int[1];
            GLES20.glGetProgramiv(programId, GLES20.GL_LINK_STATUS, linked, 0);
            GLES20.glDeleteShader(vs);
            GLES20.glDeleteShader(fs);
            if (linked[0] == 0) {
                String log = GLES20.glGetProgramInfoLog(programId);
                GLES20.glDeleteProgram(programId);
                programId = 0;
                throw new IllegalStateException("gpu_demo_gl_link_failed " + log);
            }
            attrPos = GLES20.glGetAttribLocation(programId, "aPos");
            uniformTime = GLES20.glGetUniformLocation(programId, "uTime");
            uniformTint = GLES20.glGetUniformLocation(programId, "uTint");
            if (attrPos < 0 || uniformTime < 0 || uniformTint < 0) {
                throw new IllegalStateException("gpu_demo_gl_uniform_or_attrib_missing");
            }
        }

        void draw(float timeSec) {
            int vw = Math.max(1, Math.min(width, viewportW));
            int vh = Math.max(1, Math.min(height, viewportH));
            float speed = Math.max(0.0f, Math.min(8.0f, speedScale));
            GLES20.glViewport(0, 0, vw, vh);
            GLES20.glClearColor(0.0f, 0.0f, 0.0f, 0.0f);
            GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT);
            GLES20.glUseProgram(programId);
            GLES20.glUniform1f(uniformTime, timeSec * speed);
            GLES20.glUniform4f(uniformTint, tintR, tintG, tintB, tintA);
            quadBuffer.position(0);
            GLES20.glEnableVertexAttribArray(attrPos);
            GLES20.glVertexAttribPointer(attrPos, 2, GLES20.GL_FLOAT, false, 0, quadBuffer);
            GLES20.glDrawArrays(GLES20.GL_TRIANGLE_STRIP, 0, 4);
            GLES20.glDisableVertexAttribArray(attrPos);
            if (!EGL14.eglSwapBuffers(eglDisplay, eglSurface)) {
                throw new IllegalStateException("gpu_demo_egl_swap_failed err=" + hex(EGL14.eglGetError()));
            }
        }

        void setTint(float r, float g, float b, float a) {
            tintR = clamp01(r);
            tintG = clamp01(g);
            tintB = clamp01(b);
            tintA = clamp01(a);
        }

        void setSpeed(float speed) {
            if (!Float.isFinite(speed)) {
                speedScale = 1.0f;
                return;
            }
            speedScale = Math.max(0.0f, Math.min(8.0f, speed));
        }

        void setViewport(int w, int h) {
            viewportW = Math.max(1, w);
            viewportH = Math.max(1, h);
        }

        void releaseQuietly() {
            try {
                release();
            } catch (Throwable ignored) {
            }
        }

        private void release() {
            if (eglDisplay != EGL14.EGL_NO_DISPLAY) {
                EGL14.eglMakeCurrent(
                        eglDisplay,
                        EGL14.EGL_NO_SURFACE,
                        EGL14.EGL_NO_SURFACE,
                        EGL14.EGL_NO_CONTEXT
                );
            }
            if (programId != 0) {
                GLES20.glDeleteProgram(programId);
                programId = 0;
            }
            if (eglDisplay != EGL14.EGL_NO_DISPLAY && eglSurface != EGL14.EGL_NO_SURFACE) {
                EGL14.eglDestroySurface(eglDisplay, eglSurface);
            }
            if (eglDisplay != EGL14.EGL_NO_DISPLAY && eglContext != EGL14.EGL_NO_CONTEXT) {
                EGL14.eglDestroyContext(eglDisplay, eglContext);
            }
            if (eglDisplay != EGL14.EGL_NO_DISPLAY) {
                EGL14.eglTerminate(eglDisplay);
            }
            eglSurface = EGL14.EGL_NO_SURFACE;
            eglContext = EGL14.EGL_NO_CONTEXT;
            eglDisplay = EGL14.EGL_NO_DISPLAY;
        }

        private static int compileShader(int type, String source) {
            int shader = GLES20.glCreateShader(type);
            GLES20.glShaderSource(shader, source);
            GLES20.glCompileShader(shader);
            int[] compiled = new int[1];
            GLES20.glGetShaderiv(shader, GLES20.GL_COMPILE_STATUS, compiled, 0);
            if (compiled[0] == 0) {
                String log = GLES20.glGetShaderInfoLog(shader);
                GLES20.glDeleteShader(shader);
                throw new IllegalStateException("gpu_demo_gl_compile_failed type=" + type + " log=" + log);
            }
            return shader;
        }

        private static float clamp01(float value) {
            if (!Float.isFinite(value)) {
                return 1.0f;
            }
            if (value < 0.0f) {
                return 0.0f;
            }
            if (value > 1.0f) {
                return 1.0f;
            }
            return value;
        }

        private static String hex(int value) {
            return String.format(Locale.US, "0x%04x", value);
        }
    }
}
