package org.directscreenapi.adapter;

import android.opengl.EGL14;
import android.opengl.EGLConfig;
import android.opengl.EGLContext;
import android.opengl.EGLDisplay;
import android.opengl.EGLSurface;
import android.opengl.GLES20;
import android.os.Looper;
import android.view.Choreographer;

import java.io.BufferedReader;
import java.io.FileInputStream;
import java.io.InputStreamReader;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.FloatBuffer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;

final class TouchUiGpuPresenter {
    private static final long STARTUP_TIMEOUT_MS = 5_000L;
    private static final int DEFAULT_BUFFER_WIDTH = 1280;
    private static final int DEFAULT_BUFFER_HEIGHT = 720;
    private static final int DEFAULT_WINDOW_WIDTH = 560;
    private static final int DEFAULT_WINDOW_HEIGHT = 320;

    private final String statePipePath;
    private final int zLayer;
    private final String layerName;
    private final int blurRadius;
    private final float frameRateHz;
    private final Object stateLock = new Object();
    private final UiState state = new UiState();
    private final AtomicBoolean shutdownOnce = new AtomicBoolean(false);

    private volatile boolean running = true;
    private volatile Throwable pipeError;
    private volatile SurfaceLayerSession surfaceSession;
    private volatile RenderThread renderThread;
    private volatile Thread pipeThread;
    private int bufferWidth = DEFAULT_BUFFER_WIDTH;
    private int bufferHeight = DEFAULT_BUFFER_HEIGHT;
    private int displayDensityDpi = 0;
    private float displayRefreshHz = 60.0f;

    private static final class UiState {
        int x = 0;
        int y = 0;
        int w = DEFAULT_WINDOW_WIDTH;
        int h = DEFAULT_WINDOW_HEIGHT;
        String inputText = "";
        String mode = "idle";
        String lastSubmit = "";
        float panelAlpha = 0.92f;
        float fps = 0.0f;
        long blocked = 0L;
        long passed = 0L;
        long uiEvents = 0L;
        boolean focused = false;
        boolean imeVisible = false;
        boolean cursorVisible = false;
        boolean closePressed = false;
        boolean submitPressed = false;
        boolean closeFlash = false;
        boolean submitFlash = false;
        boolean visible = true;

        UiState() {
        }

        UiState(UiState other) {
            this.x = other.x;
            this.y = other.y;
            this.w = other.w;
            this.h = other.h;
            this.inputText = other.inputText;
            this.mode = other.mode;
            this.lastSubmit = other.lastSubmit;
            this.panelAlpha = other.panelAlpha;
            this.fps = other.fps;
            this.blocked = other.blocked;
            this.passed = other.passed;
            this.uiEvents = other.uiEvents;
            this.focused = other.focused;
            this.imeVisible = other.imeVisible;
            this.cursorVisible = other.cursorVisible;
            this.closePressed = other.closePressed;
            this.submitPressed = other.submitPressed;
            this.closeFlash = other.closeFlash;
            this.submitFlash = other.submitFlash;
            this.visible = other.visible;
        }
    }

    private static final class KvEntry {
        final String key;
        final String value;

        KvEntry(String key, String value) {
            this.key = key;
            this.value = value;
        }
    }

    TouchUiGpuPresenter(
            String statePipePath,
            int zLayer,
            String layerName,
            int blurRadius,
            float frameRateHz
    ) {
        this.statePipePath = statePipePath == null ? "" : statePipePath.trim();
        this.zLayer = zLayer;
        this.layerName = (layerName == null || layerName.trim().isEmpty())
                ? "DirectScreenAPI-TouchUI"
                : layerName.trim();
        this.blurRadius = Math.max(0, blurRadius);
        // <=0 表示跟随系统当前刷新率。
        this.frameRateHz = (Float.isFinite(frameRateHz) && frameRateHz > 0.0f) ? frameRateHz : 0.0f;
    }

    void runLoop() throws Exception {
        if (statePipePath.isEmpty()) {
            throw new IllegalArgumentException("touch_ui_pipe_path_empty");
        }

        Runtime.getRuntime().addShutdownHook(new Thread(this::shutdown, "dsapi-touch-ui-shutdown"));
        try {
            resolveBufferSize();
            applyInitialWindowState();
            float targetHz = frameRateHz;
            if (!(Float.isFinite(targetHz) && targetHz > 0.0f)) {
                targetHz = displayRefreshHz > 1.0f ? displayRefreshHz : 60.0f;
            }
            surfaceSession = SurfaceLayerSession.create(
                    bufferWidth,
                    bufferHeight,
                    zLayer,
                    layerName,
                    true,
                    blurRadius,
                    targetHz
            );

            RenderThread rt = new RenderThread(surfaceSession, bufferWidth, bufferHeight, targetHz);
            renderThread = rt;
            rt.start();
            rt.awaitStartup();

            Thread pt = new Thread(this::readPipeLoop, "dsapi-touch-ui-pipe");
            pipeThread = pt;
            pt.start();

            log("touch_ui_status=started pipe="
                    + sanitizeToken(statePipePath)
                    + " z_layer=" + zLayer
                    + " layer=" + sanitizeToken(layerName)
                    + " blur_radius=" + blurRadius
                    + " frame_rate="
                    + String.format(Locale.US, "%.2f", targetHz)
                    + " renderer=gles");

            pt.join();

            rt.requestStop();
            rt.awaitFinished();
            if (rt.runtimeError() != null) {
                throw asRuntime(rt.runtimeError());
            }
            if (pipeError != null) {
                throw asRuntime(pipeError);
            }
        } finally {
            shutdown();
        }
    }

    private void resolveBufferSize() {
        int width = DEFAULT_BUFFER_WIDTH;
        int height = DEFAULT_BUFFER_HEIGHT;
        int densityDpi = 0;
        float refreshHz = 60.0f;
        try {
            DisplayAdapter.DisplaySnapshot snapshot = new AndroidDisplayAdapter().queryDisplaySnapshot();
            if (snapshot != null) {
                // 旋转后 width/height 会交换；这里用 max 生成方形缓冲区，避免横屏时宽度受限。
                int maxDim = Math.max(snapshot.width, snapshot.height);
                width = Math.max(DEFAULT_WINDOW_WIDTH, maxDim);
                height = Math.max(DEFAULT_WINDOW_HEIGHT, maxDim);
                densityDpi = snapshot.densityDpi;
                refreshHz = snapshot.refreshHz;
            }
        } catch (Throwable ignored) {
        }
        bufferWidth = Math.max(1, width);
        bufferHeight = Math.max(1, height);
        displayDensityDpi = Math.max(0, densityDpi);
        if (Float.isFinite(refreshHz) && refreshHz > 1.0f) {
            displayRefreshHz = refreshHz;
        }
    }

    private void applyInitialWindowState() {
        synchronized (stateLock) {
            state.w = clampInt(state.w, 1, bufferWidth);
            state.h = clampInt(state.h, 1, bufferHeight);
            state.x = Math.max(0, (bufferWidth - state.w) / 2);
            state.y = Math.max(0, (bufferHeight - state.h) / 3);
        }
    }

    private void readPipeLoop() {
        try (
                FileInputStream fis = new FileInputStream(statePipePath);
                InputStreamReader isr = new InputStreamReader(fis, StandardCharsets.UTF_8);
                BufferedReader reader = new BufferedReader(isr)
        ) {
            log("touch_ui_status=pipe_open path=" + sanitizeToken(statePipePath));
            String line;
            while (running && (line = reader.readLine()) != null) {
                applyStateLine(line);
            }
            log("touch_ui_status=pipe_closed");
        } catch (Throwable t) {
            if (running) {
                pipeError = t;
                log("touch_ui_status=pipe_error err=" + describeThrowable(t));
            }
        } finally {
            running = false;
            RenderThread rt = renderThread;
            if (rt != null) {
                rt.requestStop();
            }
        }
    }

    private void applyStateLine(String line) {
        String raw = line == null ? "" : line.trim();
        if (raw.isEmpty() || raw.startsWith("#")) {
            return;
        }
        List<KvEntry> entries = parseKvLine(raw);
        if (entries.isEmpty()) {
            return;
        }

        boolean requestStop = false;
        synchronized (stateLock) {
            for (KvEntry entry : entries) {
                if (entry.key == null || entry.key.isEmpty()) {
                    continue;
                }
                String key = entry.key.toLowerCase(Locale.US);
                String value = entry.value == null ? "" : entry.value;
                switch (key) {
                    case "x":
                        state.x = parseIntBounded(value, state.x, -bufferWidth * 4, bufferWidth * 4);
                        break;
                    case "y":
                        state.y = parseIntBounded(value, state.y, -bufferHeight * 4, bufferHeight * 4);
                        break;
                    case "w":
                        state.w = parseIntBounded(value, state.w, 1, bufferWidth);
                        break;
                    case "h":
                        state.h = parseIntBounded(value, state.h, 1, bufferHeight);
                        break;
                    case "input":
                        state.inputText = sanitizeText(value, 160);
                        break;
                    case "input_text":
                        state.inputText = decodeHexText(value, 160);
                        break;
                    case "mode":
                        state.mode = sanitizeText(value, 32);
                        break;
                    case "last_submit":
                        state.lastSubmit = decodeHexText(value, 160);
                        break;
                    case "focus":
                        state.focused = parseBoolean(value);
                        break;
                    case "ime":
                        state.imeVisible = parseBoolean(value);
                        break;
                    case "cursor":
                        state.cursorVisible = parseBoolean(value);
                        break;
                    case "press_close":
                        state.closePressed = parseBoolean(value);
                        break;
                    case "press_submit":
                        state.submitPressed = parseBoolean(value);
                        break;
                    case "flash_close":
                        state.closeFlash = parseBoolean(value);
                        break;
                    case "flash_submit":
                        state.submitFlash = parseBoolean(value);
                        break;
                    case "fps":
                        state.fps = parseFloatBounded(value, state.fps, 0.0f, 1000.0f);
                        break;
                    case "blocked":
                        state.blocked = parseLongBounded(value, state.blocked, 0L, Long.MAX_VALUE);
                        break;
                    case "passed":
                        state.passed = parseLongBounded(value, state.passed, 0L, Long.MAX_VALUE);
                        break;
                    case "ui_events":
                        state.uiEvents = parseLongBounded(value, state.uiEvents, 0L, Long.MAX_VALUE);
                        break;
                    case "panel_alpha":
                        state.panelAlpha = parseFloatBounded(value, state.panelAlpha, 0.0f, 1.0f);
                        break;
                    case "visible":
                        state.visible = parseBoolean(value);
                        break;
                    case "quit":
                    case "stop":
                        requestStop = parseBoolean(value);
                        break;
                    default:
                        break;
                }
            }
            state.w = clampInt(state.w, 1, bufferWidth);
            state.h = clampInt(state.h, 1, bufferHeight);
        }

        if (requestStop) {
            running = false;
            RenderThread rt = renderThread;
            if (rt != null) {
                rt.requestStop();
            }
        }
    }

    private UiState snapshotState() {
        synchronized (stateLock) {
            return new UiState(state);
        }
    }

    private void shutdown() {
        if (!shutdownOnce.compareAndSet(false, true)) {
            return;
        }
        running = false;

        Thread pt = pipeThread;
        if (pt != null && pt != Thread.currentThread()) {
            pt.interrupt();
        }

        RenderThread rt = renderThread;
        if (rt != null) {
            rt.requestStop();
            rt.awaitFinishedQuietly(1_500L);
            renderThread = null;
        }

        SurfaceLayerSession session = surfaceSession;
        if (session != null) {
            session.closeQuietly();
            surfaceSession = null;
        }

        log("touch_ui_status=stopped");
    }

    private static List<KvEntry> parseKvLine(String line) {
        List<KvEntry> out = new ArrayList<>();
        int n = line.length();
        int i = 0;
        while (i < n) {
            while (i < n && Character.isWhitespace(line.charAt(i))) {
                i++;
            }
            if (i >= n) {
                break;
            }

            int keyStart = i;
            while (i < n && !Character.isWhitespace(line.charAt(i)) && line.charAt(i) != '=') {
                i++;
            }
            if (i >= n || line.charAt(i) != '=') {
                while (i < n && !Character.isWhitespace(line.charAt(i))) {
                    i++;
                }
                continue;
            }
            String key = line.substring(keyStart, i);
            i++;

            String value;
            if (i < n && line.charAt(i) == '"') {
                i++;
                StringBuilder sb = new StringBuilder();
                boolean escaping = false;
                while (i < n) {
                    char c = line.charAt(i++);
                    if (escaping) {
                        sb.append(unescapeChar(c));
                        escaping = false;
                        continue;
                    }
                    if (c == '\\') {
                        escaping = true;
                        continue;
                    }
                    if (c == '"') {
                        break;
                    }
                    sb.append(c);
                }
                if (escaping) {
                    sb.append('\\');
                }
                value = sb.toString();
            } else {
                int valStart = i;
                while (i < n && !Character.isWhitespace(line.charAt(i))) {
                    i++;
                }
                value = line.substring(valStart, i);
            }
            out.add(new KvEntry(key, value));
        }
        return out;
    }

    private static char unescapeChar(char c) {
        switch (c) {
            case 'n':
                return '\n';
            case 'r':
                return '\r';
            case 't':
                return '\t';
            case '"':
                return '"';
            case '\\':
                return '\\';
            case 's':
                return ' ';
            default:
                return c;
        }
    }

    private static int parseIntBounded(String text, int fallback, int min, int max) {
        try {
            int parsed = Integer.parseInt(text);
            return clampInt(parsed, min, max);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static float parseFloatBounded(String text, float fallback, float min, float max) {
        try {
            float parsed = Float.parseFloat(text);
            if (!Float.isFinite(parsed)) {
                return fallback;
            }
            return clampFloat(parsed, min, max);
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static long parseLongBounded(String text, long fallback, long min, long max) {
        try {
            long parsed = Long.parseLong(text);
            if (parsed < min) {
                return min;
            }
            if (parsed > max) {
                return max;
            }
            return parsed;
        } catch (Throwable ignored) {
            return fallback;
        }
    }

    private static String decodeHexText(String value, int maxLen) {
        String raw = safeText(value).trim();
        if (raw.isEmpty() || "-".equals(raw)) {
            return "";
        }
        if (raw.startsWith("0x") || raw.startsWith("0X")) {
            raw = raw.substring(2);
        }
        if ((raw.length() & 1) != 0) {
            return "";
        }
        int outLen = raw.length() / 2;
        byte[] out = new byte[outLen];
        for (int i = 0; i < outLen; i++) {
            int hi = Character.digit(raw.charAt(i * 2), 16);
            int lo = Character.digit(raw.charAt(i * 2 + 1), 16);
            if (hi < 0 || lo < 0) {
                return "";
            }
            out[i] = (byte) ((hi << 4) | lo);
        }
        String decoded = new String(out, StandardCharsets.UTF_8);
        return sanitizeText(decoded, maxLen);
    }

    private static int clampInt(int v, int min, int max) {
        return Math.max(min, Math.min(max, v));
    }

    private static float clampFloat(float v, float min, float max) {
        return Math.max(min, Math.min(max, v));
    }

    private static boolean parseBoolean(String text) {
        String v = text == null ? "" : text.trim().toLowerCase(Locale.US);
        return "1".equals(v)
                || "true".equals(v)
                || "yes".equals(v)
                || "on".equals(v);
    }

    private static String safeText(String text) {
        return text == null ? "" : text;
    }

    private static String sanitizeText(String text, int maxLen) {
        String normalized = safeText(text)
                .replace('\r', ' ')
                .replace('\n', ' ');
        if (normalized.length() <= maxLen) {
            return normalized;
        }
        return normalized.substring(0, Math.max(0, maxLen));
    }

    private static RuntimeException asRuntime(Throwable t) {
        if (t instanceof RuntimeException) {
            return (RuntimeException) t;
        }
        return new RuntimeException(t);
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

    private static String describeThrowable(Throwable t) {
        if (t == null) {
            return "unknown";
        }
        String msg = t.getMessage();
        if (msg == null || msg.trim().isEmpty()) {
            return t.getClass().getName();
        }
        return t.getClass().getName() + ":" + msg;
    }

    private static void log(String line) {
        System.out.println(line);
    }

    private final class RenderThread extends Thread implements Choreographer.FrameCallback {
        private final SurfaceLayerSession session;
        private final int bufferW;
        private final int bufferH;
        private final long minFrameIntervalNs;
        private final CountDownLatch startupLatch = new CountDownLatch(1);
        private final CountDownLatch finishedLatch = new CountDownLatch(1);
        private final GlesRenderer renderer;

        private volatile boolean localRunning = true;
        private volatile Throwable startupError;
        private volatile Throwable runtimeError;
        private volatile Looper looper;
        private Choreographer choreographer;
        private long startedAtNs = -1L;
        private long lastRenderNs = -1L;
        private int appliedX = Integer.MIN_VALUE;
        private int appliedY = Integer.MIN_VALUE;
        private int appliedW = Integer.MIN_VALUE;
        private int appliedH = Integer.MIN_VALUE;

        RenderThread(SurfaceLayerSession session, int bufferW, int bufferH, float targetFrameRateHz) {
            super("dsapi-touch-ui-render");
            this.session = session;
            this.bufferW = Math.max(1, bufferW);
            this.bufferH = Math.max(1, bufferH);
            if (Float.isFinite(targetFrameRateHz) && targetFrameRateHz > 0.0f) {
                this.minFrameIntervalNs = (long) (1_000_000_000.0d / targetFrameRateHz);
            } else {
                this.minFrameIntervalNs = 0L;
            }
            this.renderer = new GlesRenderer(session.surfaceObject(), this.bufferW, this.bufferH, displayDensityDpi);
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
                finishedLatch.countDown();
                return;
            }

            startupLatch.countDown();
            try {
                choreographer.postFrameCallback(this);
                Looper.loop();
            } catch (Throwable t) {
                runtimeError = t;
            } finally {
                renderer.releaseQuietly();
                session.closeQuietly();
                if (surfaceSession == session) {
                    surfaceSession = null;
                }
                finishedLatch.countDown();
            }
        }

        private void onVsync(long frameTimeNanos) {
            if (!localRunning || !running) {
                quitLooper();
                return;
            }
            try {
                if (startedAtNs < 0L) {
                    startedAtNs = frameTimeNanos;
                    lastRenderNs = frameTimeNanos;
                }
                if (minFrameIntervalNs > 0L && lastRenderNs > 0L) {
                    long deltaNs = frameTimeNanos - lastRenderNs;
                    if (deltaNs > 0 && deltaNs < minFrameIntervalNs) {
                        // 限帧：保持 VSync 驱动，但跳过本帧绘制。
                        postNextVsync();
                        return;
                    }
                }
                float timeSec = (frameTimeNanos - startedAtNs) * 1.0e-9f;
                renderOnce(timeSec);
                lastRenderNs = frameTimeNanos;
            } catch (Throwable t) {
                runtimeError = t;
                localRunning = false;
                quitLooper();
                return;
            }
            postNextVsync();
        }

        private void postNextVsync() {
            if (!localRunning || !running) {
                quitLooper();
                return;
            }
            Choreographer c = choreographer;
            if (c != null) {
                c.postFrameCallback(this);
            } else {
                quitLooper();
            }
        }

        @Override
        public void doFrame(long frameTimeNanos) {
            onVsync(frameTimeNanos);
        }

        private void renderOnce(float timeSec) throws Exception {
            UiState s = snapshotState();
            int safeW = clampInt(s.w, 1, bufferW);
            int safeH = clampInt(s.h, 1, bufferH);
            boolean posChanged = appliedX != s.x || appliedY != s.y;
            boolean sizeChanged = appliedW != safeW || appliedH != safeH;
            if (sizeChanged) {
                // 几何变化（尤其是 resize）尽量用单 Transaction 提交，减少闪烁/中间态。
                session.setGeometry((float) s.x, (float) s.y, safeW, safeH);
                renderer.setViewport(safeW, safeH);
                appliedX = s.x;
                appliedY = s.y;
                appliedW = safeW;
                appliedH = safeH;
            } else if (posChanged) {
                session.setPosition((float) s.x, (float) s.y);
                appliedX = s.x;
                appliedY = s.y;
            }
            renderer.drawFrame(s, timeSec);
        }

        void awaitStartup() throws Exception {
            if (!startupLatch.await(STARTUP_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
                throw new IllegalStateException("touch_ui_render_start_timeout");
            }
            if (startupError != null) {
                throw asRuntime(startupError);
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

        void requestStop() {
            localRunning = false;
            quitLooper();
        }

        private void quitLooper() {
            Looper l = looper;
            if (l != null) {
                l.quitSafely();
            }
        }
    }

    private static final class GlesRenderer {
        private static final String VS = ""
                + "attribute vec2 aPos;\n"
                + "varying vec2 vUv;\n"
                + "void main() {\n"
                + "  vUv = aPos * 0.5 + 0.5;\n"
                + "  gl_Position = vec4(aPos, 0.0, 1.0);\n"
                + "}\n";

        private static final String TEXT_VS = ""
                + "attribute vec2 aPosPx;\n"
                + "attribute vec2 aUv;\n"
                + "uniform vec2 uResolution;\n"
                + "varying vec2 vUv;\n"
                + "void main() {\n"
                + "  vec2 p = aPosPx;\n"
                + "  vec2 ndc = vec2((p.x / uResolution.x) * 2.0 - 1.0, 1.0 - (p.y / uResolution.y) * 2.0);\n"
                + "  gl_Position = vec4(ndc, 0.0, 1.0);\n"
                + "  vUv = aUv;\n"
                + "}\n";

        private static final String TEXT_FS = ""
                + "precision mediump float;\n"
                + "varying vec2 vUv;\n"
                + "uniform sampler2D uTex;\n"
                + "uniform vec4 uColor;\n"
                + "void main() {\n"
                + "  float a = texture2D(uTex, vUv).a;\n"
                + "  gl_FragColor = vec4(uColor.rgb, uColor.a * a);\n"
                + "}\n";

        private static final String FS = ""
                + "precision mediump float;\n"
                + "varying vec2 vUv;\n"
                + "uniform vec2 uResolution;\n"
                + "uniform vec2 uViewportOrigin;\n"
                + "uniform float uTime;\n"
                + "uniform float uVisible;\n"
                + "uniform float uPanelAlpha;\n"
                + "uniform float uFocused;\n"
                + "uniform float uImeVisible;\n"
                + "uniform float uCursorVisible;\n"
                + "uniform float uClosePressed;\n"
                + "uniform float uSubmitPressed;\n"
                + "uniform float uCloseFlash;\n"
                + "uniform float uSubmitFlash;\n"
                + "uniform vec4 uMetricA;\n"
                + "uniform vec4 uMetricB;\n"
                + "float saturate(float v) { return clamp(v, 0.0, 1.0); }\n"
                + "float sdRoundRect(vec2 p, vec2 pos, vec2 size, float r) {\n"
                + "  vec2 c = pos + size * 0.5;\n"
                + "  vec2 h = size * 0.5;\n"
                + "  vec2 q = abs(p - c) - (h - vec2(r));\n"
                + "  return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;\n"
                + "}\n"
                + "float aaFill(float d) {\n"
                + "  return 1.0 - smoothstep(0.0, 1.0, d);\n"
                + "}\n"
                + "void main() {\n"
                + "  vec2 frag = gl_FragCoord.xy - uViewportOrigin;\n"
                + "  vec2 p = vec2(frag.x, uResolution.y - frag.y);\n"
                + "\n"
                + "  float titleH = uMetricA.x;\n"
                + "  float closeTop = uMetricA.y;\n"
                + "  float closeMargin = uMetricA.z;\n"
                + "  float padding = uMetricA.w;\n"
                + "  float btnW = uMetricB.x;\n"
                + "  float btnH = uMetricB.y;\n"
                + "  float gap = uMetricB.z;\n"
                + "  float inputH = uMetricB.w;\n"
                + "\n"
                + "  float r = clamp(titleH * 0.55, 10.0, 28.0);\n"
                + "  float borderW = max(1.0, titleH * 0.04);\n"
                + "  float dPanel = sdRoundRect(p, vec2(0.0), uResolution, r);\n"
                + "  float panelFill = aaFill(dPanel);\n"
                + "  float panelInner = aaFill(dPanel + borderW);\n"
                + "  float panelBorder = saturate(panelFill - panelInner);\n"
                + "\n"
                + "  float gy = saturate(p.y / max(1.0, uResolution.y));\n"
                + "  vec3 bgTop = vec3(0.05, 0.07, 0.10);\n"
                + "  vec3 bgBot = vec3(0.07, 0.10, 0.15);\n"
                + "  vec3 col = mix(bgTop, bgBot, pow(gy, 1.15));\n"
                + "  float pulse = 0.5 + 0.5 * sin(uTime * 1.6);\n"
                + "  col += vec3(0.02, 0.03, 0.04) * pulse;\n"
                + "  col += vec3(0.03, 0.06, 0.09) * uImeVisible;\n"
                + "\n"
                + "  float titleMask = step(p.y, titleH);\n"
                + "  col = mix(col, col * 0.82, titleMask);\n"
                + "\n"
                + "  vec2 closePos = vec2(uResolution.x - closeMargin - btnW, closeTop);\n"
                + "  vec2 submitPos = vec2(closePos.x - gap - btnW, closePos.y);\n"
                + "  vec2 inputPos = vec2(padding, titleH + padding);\n"
                + "  vec2 inputSize = vec2(max(40.0, uResolution.x - 2.0 * padding), max(22.0, inputH));\n"
                + "\n"
                + "  float dClose = sdRoundRect(p, closePos, vec2(btnW, btnH), btnH * 0.45);\n"
                + "  float dSubmit = sdRoundRect(p, submitPos, vec2(btnW, btnH), btnH * 0.45);\n"
                + "  float dInput = sdRoundRect(p, inputPos, inputSize, min(12.0, inputSize.y * 0.35));\n"
                + "  float closeFill = aaFill(dClose);\n"
                + "  float submitFill = aaFill(dSubmit);\n"
                + "  float inputFill = aaFill(dInput);\n"
                + "\n"
                + "  float closeOn = saturate(max(uClosePressed, uCloseFlash));\n"
                + "  float submitOn = saturate(max(uSubmitPressed, uSubmitFlash));\n"
                + "  vec3 closeBase = vec3(0.86, 0.26, 0.26);\n"
                + "  vec3 submitBase = vec3(0.20, 0.56, 0.94);\n"
                + "  vec3 closeCol = mix(closeBase * 0.72, closeBase, 0.55 + 0.45 * closeOn);\n"
                + "  vec3 submitCol = mix(submitBase * 0.72, submitBase, 0.55 + 0.45 * submitOn);\n"
                + "  col = mix(col, closeCol, closeFill);\n"
                + "  col = mix(col, submitCol, submitFill);\n"
                + "\n"
                + "  vec3 inputBg = vec3(0.03, 0.04, 0.06);\n"
                + "  col = mix(col, inputBg, inputFill * 0.86);\n"
                + "  float inputInner = aaFill(dInput + borderW);\n"
                + "  float inputBorder = saturate(inputFill - inputInner);\n"
                + "  vec3 inputBorderCol = mix(vec3(0.55, 0.63, 0.72), vec3(0.40, 0.92, 0.65), uFocused);\n"
                + "  col = mix(col, inputBorderCol, inputBorder);\n"
                + "\n"
                + "  if (uFocused > 0.5 && uCursorVisible > 0.5) {\n"
                + "    float cx = inputPos.x + 14.0;\n"
                + "    float cy0 = inputPos.y + 7.0;\n"
                + "    float cy1 = inputPos.y + inputSize.y - 7.0;\n"
                + "    float cursor = step(abs(p.x - cx), 1.0) * step(cy0, p.y) * step(p.y, cy1);\n"
                + "    col = mix(col, vec3(0.92, 0.96, 1.0), cursor);\n"
                + "  }\n"
                + "\n"
                + "  vec2 cC = closePos + vec2(btnW, btnH) * 0.5;\n"
                + "  vec2 qc = p - cC;\n"
                + "  float lim = min(btnW, btnH) * 0.28;\n"
                + "  float thick = max(1.6, btnH * 0.08);\n"
                + "  float x1 = 1.0 - smoothstep(thick, thick + 1.0, abs(qc.x - qc.y));\n"
                + "  float x2 = 1.0 - smoothstep(thick, thick + 1.0, abs(qc.x + qc.y));\n"
                + "  float xLim = step(max(abs(qc.x), abs(qc.y)), lim);\n"
                + "  float xMask = closeFill * xLim * max(x1, x2);\n"
                + "  col = mix(col, vec3(0.96, 0.96, 0.97), xMask);\n"
                + "\n"
                + "  vec2 sC = submitPos + vec2(btnW, btnH) * 0.5;\n"
                + "  vec2 qs = p - sC;\n"
                + "  float arrowL = min(btnW, btnH) * 0.32;\n"
                + "  float shaft = step(abs(qs.y), thick * 0.70) * step(-arrowL, qs.x) * step(qs.x, arrowL * 0.55);\n"
                + "  float head = step(abs(qs.x - arrowL * 0.55), thick) * step(abs(qs.y), thick * 1.6);\n"
                + "  float aLim = step(max(abs(qs.x), abs(qs.y)), arrowL * 1.2);\n"
                + "  float aMask = submitFill * aLim * saturate(shaft + head);\n"
                + "  col = mix(col, vec3(0.96, 0.97, 1.0), aMask);\n"
                + "\n"
                + "  vec3 panelBorderCol = mix(vec3(0.62, 0.70, 0.80), vec3(0.40, 0.92, 0.65), uFocused);\n"
                + "  col = mix(col, panelBorderCol, panelBorder);\n"
                + "\n"
                + "  float alpha = saturate(uPanelAlpha) * uVisible * panelFill;\n"
                + "  gl_FragColor = vec4(col, alpha);\n"
                + "}\n";

        private final Object windowSurfaceObject;
        private final int maxWidth;
        private final int maxHeight;
        private final FloatBuffer quadBuffer;
        private final float metricTitleHeight;
        private final float metricCloseTop;
        private final float metricCloseMargin;
        private final float metricPadding;
        private final float metricButtonW;
        private final float metricButtonH;
        private final float metricGap;
        private final float metricInputH;
        private final TextRenderer textRenderer;

        private EGLDisplay eglDisplay = EGL14.EGL_NO_DISPLAY;
        private EGLContext eglContext = EGL14.EGL_NO_CONTEXT;
        private EGLSurface eglSurface = EGL14.EGL_NO_SURFACE;
        private int programId;
        private int attrPos;
        private int uniformResolution;
        private int uniformViewportOrigin;
        private int uniformTime;
        private int uniformVisible;
        private int uniformPanelAlpha;
        private int uniformFocused;
        private int uniformImeVisible;
        private int uniformCursorVisible;
        private int uniformClosePressed;
        private int uniformSubmitPressed;
        private int uniformCloseFlash;
        private int uniformSubmitFlash;
        private int uniformMetricA;
        private int uniformMetricB;
        private int viewportW;
        private int viewportH;

        private static float clamp(float v, float min, float max) {
            return Math.max(min, Math.min(max, v));
        }

        GlesRenderer(Object windowSurfaceObject, int maxWidth, int maxHeight, int densityDpi) {
            this.windowSurfaceObject = windowSurfaceObject;
            this.maxWidth = Math.max(1, maxWidth);
            this.maxHeight = Math.max(1, maxHeight);
            this.viewportW = this.maxWidth;
            this.viewportH = this.maxHeight;
            float density = densityDpi > 0 ? (densityDpi / 160.0f) : 1.0f;
            float controlScale = clamp(1.0f + (density - 1.0f) * 0.18f, 1.0f, 1.45f);
            float titleH = Math.max(44.0f * controlScale, 20.0f);
            float closeW = 40.0f * controlScale;
            float closeH = 32.0f * controlScale;
            float closeMargin = 8.0f * controlScale;
            float closeTop = 6.0f * controlScale;
            float actionW = 96.0f * controlScale;
            float actionH = 34.0f * controlScale;
            float inputH = 36.0f * controlScale;
            float padding = 12.0f * controlScale;
            float gap = Math.max(4.0f * controlScale, 2.0f);
            float buttonW = Math.max(60.0f, Math.max(actionW, closeW));
            float buttonH = Math.max(24.0f, Math.max(actionH, closeH));
            this.metricTitleHeight = titleH;
            this.metricCloseTop = closeTop;
            this.metricCloseMargin = closeMargin;
            this.metricPadding = padding;
            this.metricButtonW = buttonW;
            this.metricButtonH = buttonH;
            this.metricGap = gap;
            this.metricInputH = inputH;
            this.textRenderer = new TextRenderer(
                    this.maxWidth,
                    this.maxHeight,
                    this.metricTitleHeight,
                    this.metricCloseTop,
                    this.metricCloseMargin,
                    this.metricPadding,
                    this.metricButtonW,
                    this.metricButtonH,
                    this.metricGap,
                    this.metricInputH
            );
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
                throw new IllegalStateException("touch_ui_egl_no_display");
            }

            int[] version = new int[2];
            if (!EGL14.eglInitialize(eglDisplay, version, 0, version, 1)) {
                throw new IllegalStateException("touch_ui_egl_init_failed err=" + hex(EGL14.eglGetError()));
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
                throw new IllegalStateException("touch_ui_egl_choose_config_failed err=" + hex(EGL14.eglGetError()));
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
                throw new IllegalStateException("touch_ui_egl_create_context_failed err=" + hex(EGL14.eglGetError()));
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
                throw new IllegalStateException("touch_ui_egl_create_surface_failed err=" + hex(EGL14.eglGetError()));
            }

            if (!EGL14.eglMakeCurrent(eglDisplay, eglSurface, eglSurface, eglContext)) {
                throw new IllegalStateException("touch_ui_egl_make_current_failed err=" + hex(EGL14.eglGetError()));
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
                String glLog = GLES20.glGetProgramInfoLog(programId);
                GLES20.glDeleteProgram(programId);
                programId = 0;
                throw new IllegalStateException("touch_ui_gl_link_failed " + glLog);
            }

            attrPos = GLES20.glGetAttribLocation(programId, "aPos");
            uniformResolution = GLES20.glGetUniformLocation(programId, "uResolution");
            uniformViewportOrigin = GLES20.glGetUniformLocation(programId, "uViewportOrigin");
            uniformTime = GLES20.glGetUniformLocation(programId, "uTime");
            uniformVisible = GLES20.glGetUniformLocation(programId, "uVisible");
            uniformPanelAlpha = GLES20.glGetUniformLocation(programId, "uPanelAlpha");
            uniformFocused = GLES20.glGetUniformLocation(programId, "uFocused");
            uniformImeVisible = GLES20.glGetUniformLocation(programId, "uImeVisible");
            uniformCursorVisible = GLES20.glGetUniformLocation(programId, "uCursorVisible");
            uniformClosePressed = GLES20.glGetUniformLocation(programId, "uClosePressed");
            uniformSubmitPressed = GLES20.glGetUniformLocation(programId, "uSubmitPressed");
            uniformCloseFlash = GLES20.glGetUniformLocation(programId, "uCloseFlash");
            uniformSubmitFlash = GLES20.glGetUniformLocation(programId, "uSubmitFlash");
            uniformMetricA = GLES20.glGetUniformLocation(programId, "uMetricA");
            uniformMetricB = GLES20.glGetUniformLocation(programId, "uMetricB");
            if (attrPos < 0
                    || uniformResolution < 0
                    || uniformViewportOrigin < 0
                    || uniformTime < 0
                    || uniformVisible < 0
                    || uniformPanelAlpha < 0
                    || uniformFocused < 0
                    || uniformImeVisible < 0
                    || uniformCursorVisible < 0
                    || uniformClosePressed < 0
                    || uniformSubmitPressed < 0
                    || uniformCloseFlash < 0
                    || uniformSubmitFlash < 0
                    || uniformMetricA < 0
                    || uniformMetricB < 0) {
                throw new IllegalStateException("touch_ui_gl_uniform_or_attrib_missing");
            }

            GLES20.glDisable(GLES20.GL_DEPTH_TEST);
            GLES20.glDisable(GLES20.GL_CULL_FACE);
            GLES20.glEnable(GLES20.GL_BLEND);
            GLES20.glBlendFunc(GLES20.GL_SRC_ALPHA, GLES20.GL_ONE_MINUS_SRC_ALPHA);

            textRenderer.initGl();
        }

        void setViewport(int w, int h) {
            viewportW = Math.max(1, Math.min(maxWidth, w));
            viewportH = Math.max(1, Math.min(maxHeight, h));
        }

        void drawFrame(UiState state, float timeSec) {
            int vw = Math.max(1, Math.min(maxWidth, viewportW));
            int vh = Math.max(1, Math.min(maxHeight, viewportH));

            // SurfaceFlinger crop uses top-left origin while GLES viewport uses bottom-left origin.
            // Align the rendered window to the crop rectangle by anchoring viewport to top edge.
            int viewportY = Math.max(0, maxHeight - vh);
            GLES20.glViewport(0, viewportY, vw, vh);
            GLES20.glClearColor(0.0f, 0.0f, 0.0f, 0.0f);
            GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT);
            GLES20.glUseProgram(programId);
            GLES20.glUniform2f(uniformResolution, (float) vw, (float) vh);
            GLES20.glUniform2f(uniformViewportOrigin, 0.0f, (float) viewportY);
            GLES20.glUniform1f(uniformTime, timeSec);
            GLES20.glUniform1f(uniformVisible, state.visible ? 1.0f : 0.0f);
            GLES20.glUniform1f(uniformPanelAlpha, clamp01(state.panelAlpha));
            GLES20.glUniform1f(uniformFocused, state.focused ? 1.0f : 0.0f);
            GLES20.glUniform1f(uniformImeVisible, state.imeVisible ? 1.0f : 0.0f);
            GLES20.glUniform1f(uniformCursorVisible, state.cursorVisible ? 1.0f : 0.0f);
            GLES20.glUniform1f(uniformClosePressed, state.closePressed ? 1.0f : 0.0f);
            GLES20.glUniform1f(uniformSubmitPressed, state.submitPressed ? 1.0f : 0.0f);
            GLES20.glUniform1f(uniformCloseFlash, state.closeFlash ? 1.0f : 0.0f);
            GLES20.glUniform1f(uniformSubmitFlash, state.submitFlash ? 1.0f : 0.0f);
            GLES20.glUniform4f(
                    uniformMetricA,
                    metricTitleHeight,
                    metricCloseTop,
                    metricCloseMargin,
                    metricPadding
            );
            GLES20.glUniform4f(
                    uniformMetricB,
                    metricButtonW,
                    metricButtonH,
                    metricGap,
                    metricInputH
            );

            quadBuffer.position(0);
            GLES20.glEnableVertexAttribArray(attrPos);
            GLES20.glVertexAttribPointer(attrPos, 2, GLES20.GL_FLOAT, false, 0, quadBuffer);
            GLES20.glDrawArrays(GLES20.GL_TRIANGLE_STRIP, 0, 4);
            GLES20.glDisableVertexAttribArray(attrPos);

            try {
                textRenderer.drawOverlay(vw, vh, viewportY, state, timeSec);
            } catch (Throwable t) {
                // Keep presenter alive even if text rendering fails on some ROMs.
                log("touch_ui_warn=text_renderer_failed err=" + describeThrowable(t));
                textRenderer.disable();
            }

            if (!EGL14.eglSwapBuffers(eglDisplay, eglSurface)) {
                throw new IllegalStateException("touch_ui_egl_swap_failed err=" + hex(EGL14.eglGetError()));
            }
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
            textRenderer.releaseGlQuietly();
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
                String glLog = GLES20.glGetShaderInfoLog(shader);
                GLES20.glDeleteShader(shader);
                throw new IllegalStateException("touch_ui_gl_compile_failed type=" + type + " log=" + glLog);
            }
            return shader;
        }

        private static float clamp01(float value) {
            if (!Float.isFinite(value)) {
                return 0.0f;
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

        private static final class TextRenderer {
            private static final int ATLAS_UNPACK_ALIGNMENT = 1;
            private static final long FPS_UPDATE_MIN_NS = 250_000_000L;

            private final int maxWidth;
            private final int maxHeight;
            private final float titleH;
            private final float closeTop;
            private final float closeMargin;
            private final float padding;
            private final float btnW;
            private final float btnH;
            private final float gap;
            private final float inputH;

            private final TinyFont titleFont;
            private final TinyFont inputFont;

            private final TextBitmap titleText = new TextBitmap();
            private final TextBitmap inputText = new TextBitmap();

            private final FloatBuffer quad = ByteBuffer
                    .allocateDirect(4 * 4 * 4)
                    .order(ByteOrder.nativeOrder())
                    .asFloatBuffer();

            private int programId;
            private int attrPosPx;
            private int attrUv;
            private int uniformResolution;
            private int uniformColor;
            private int uniformTex;

            private long lastFpsUpdateNs = 0L;
            private float lastFpsBucket = -1.0f;
            private String lastMode = "";
            private String lastTitleText = "";
            private String lastInputText = "";
            private boolean lastFocused = false;
            private boolean disabled = false;

            TextRenderer(
                    int maxWidth,
                    int maxHeight,
                    float titleH,
                    float closeTop,
                    float closeMargin,
                    float padding,
                    float btnW,
                    float btnH,
                    float gap,
                    float inputH
            ) {
                this.maxWidth = Math.max(1, maxWidth);
                this.maxHeight = Math.max(1, maxHeight);
                this.titleH = titleH;
                this.closeTop = closeTop;
                this.closeMargin = closeMargin;
                this.padding = padding;
                this.btnW = btnW;
                this.btnH = btnH;
                this.gap = gap;
                this.inputH = inputH;

                int titleScale = TinyFont.scaleForTargetPx(Math.max(12.0f, titleH * 0.44f));
                int inputScale = TinyFont.scaleForTargetPx(Math.max(12.0f, inputH * 0.62f));
                this.titleFont = new TinyFont(titleScale);
                this.inputFont = new TinyFont(inputScale);
            }

            void disable() {
                disabled = true;
            }

            void initGl() {
                if (disabled) return;
                int vs = compileShader(GLES20.GL_VERTEX_SHADER, TEXT_VS);
                int fs = compileShader(GLES20.GL_FRAGMENT_SHADER, TEXT_FS);
                programId = GLES20.glCreateProgram();
                GLES20.glAttachShader(programId, vs);
                GLES20.glAttachShader(programId, fs);
                GLES20.glLinkProgram(programId);
                int[] linked = new int[1];
                GLES20.glGetProgramiv(programId, GLES20.GL_LINK_STATUS, linked, 0);
                GLES20.glDeleteShader(vs);
                GLES20.glDeleteShader(fs);
                if (linked[0] == 0) {
                    String glLog = GLES20.glGetProgramInfoLog(programId);
                    GLES20.glDeleteProgram(programId);
                    programId = 0;
                    throw new IllegalStateException("touch_ui_text_gl_link_failed " + glLog);
                }

                attrPosPx = GLES20.glGetAttribLocation(programId, "aPosPx");
                attrUv = GLES20.glGetAttribLocation(programId, "aUv");
                uniformResolution = GLES20.glGetUniformLocation(programId, "uResolution");
                uniformColor = GLES20.glGetUniformLocation(programId, "uColor");
                uniformTex = GLES20.glGetUniformLocation(programId, "uTex");
                if (attrPosPx < 0 || attrUv < 0 || uniformResolution < 0 || uniformColor < 0 || uniformTex < 0) {
                    throw new IllegalStateException("touch_ui_text_gl_uniform_or_attrib_missing");
                }

                titleText.ensureTexture();
                inputText.ensureTexture();

                GLES20.glUseProgram(programId);
                GLES20.glUniform1i(uniformTex, 0);
            }

            void releaseGlQuietly() {
                try {
                    releaseGl();
                } catch (Throwable ignored) {
                }
            }

            private void releaseGl() {
                titleText.releaseGlQuietly();
                inputText.releaseGlQuietly();
                if (programId != 0) {
                    GLES20.glDeleteProgram(programId);
                    programId = 0;
                }
            }

            void drawOverlay(int vw, int vh, int viewportY, UiState state, float timeSec) {
                if (disabled || programId == 0) return;
                if (!state.visible) return;

                updateTitleIfNeeded(vw, state);
                updateInputIfNeeded(vw, state);

                GLES20.glUseProgram(programId);
                GLES20.glUniform2f(uniformResolution, (float) vw, (float) vh);

                float titleMaxX = resolveTitleMaxX(vw);
                drawTextWithShadow(titleText, padding, (titleH - titleText.height) * 0.5f, titleMaxX, 0.92f, 0.96f, 1.0f, 0.92f);

                float inputX = padding;
                float inputY = titleH + padding;
                float inputW = Math.max(40.0f, vw - 2.0f * padding);
                float inputBoxH = Math.max(22.0f, inputH);
                int scX = clampInt((int) Math.floor(inputX), 0, maxWidth);
                int scY = clampInt((int) Math.floor(viewportY + (vh - (inputY + inputBoxH))), 0, maxHeight);
                int scW = clampInt((int) Math.ceil(inputW), 0, maxWidth - scX);
                int scH = clampInt((int) Math.ceil(inputBoxH), 0, maxHeight - scY);
                if (scW > 0 && scH > 0) {
                    GLES20.glEnable(GLES20.GL_SCISSOR_TEST);
                    GLES20.glScissor(scX, scY, scW, scH);
                    float textInsetX = inputX + Math.max(10.0f, padding * 0.6f);
                    float textY = inputY + (inputBoxH - inputText.height) * 0.5f;
                    if (textY < inputY) textY = inputY;
                    float c = state.focused ? 0.96f : 0.82f;
                    float a = state.focused ? 0.95f : 0.78f;
                    drawTextWithShadow(inputText, textInsetX, textY, inputX + inputW, c, c, c, a);
                    GLES20.glDisable(GLES20.GL_SCISSOR_TEST);
                }
            }

            private static int clampInt(int v, int min, int max) {
                return Math.max(min, Math.min(max, v));
            }

            private float resolveTitleMaxX(int vw) {
                // Do not overlap the right-side buttons.
                float closeX = vw - closeMargin - btnW;
                float submitX = closeX - gap - btnW;
                return Math.max(padding + 40.0f, submitX - gap);
            }

            private void updateTitleIfNeeded(int vw, UiState state) {
                long now = System.nanoTime();
                float fpsBucket = Math.round(state.fps * 10.0f) / 10.0f;
                boolean fpsDue = (now - lastFpsUpdateNs) >= FPS_UPDATE_MIN_NS;
                boolean modeChanged = !safe(state.mode).equals(lastMode);
                boolean fpsChanged = fpsBucket != lastFpsBucket;
                if (!modeChanged && !(fpsDue && fpsChanged)) {
                    return;
                }

                String mode = normalizeAscii(safe(state.mode)).trim();
                if (!mode.isEmpty()) {
                    mode = mode.toUpperCase(Locale.US);
                }
                String fps = String.format(Locale.US, "%.1f", fpsBucket);
                String text = (mode.isEmpty() ? "IDLE" : mode) + "  " + fps + "FPS";
                if (!text.equals(lastTitleText) || modeChanged) {
                    int maxW = (int) Math.floor(resolveTitleMaxX(vw) - padding);
                    titleText.updateText(text, titleFont, Math.max(40, maxW));
                    lastTitleText = text;
                }
                lastMode = mode;
                lastFpsBucket = fpsBucket;
                lastFpsUpdateNs = now;
            }

            private void updateInputIfNeeded(int vw, UiState state) {
                String raw = normalizeAscii(safe(state.inputText));
                boolean focused = state.focused;
                String text = raw;
                if (text.isEmpty()) {
                    text = focused ? "" : "TAP TO TYPE";
                }
                if (text.equals(lastInputText) && focused == lastFocused) {
                    return;
                }
                float inputW = Math.max(40.0f, vw - 2.0f * padding);
                int maxW = (int) Math.floor(Math.max(20.0f, inputW - Math.max(20.0f, padding * 1.2f)));
                inputText.updateText(text, inputFont, Math.max(20, maxW));
                lastInputText = text;
                lastFocused = focused;
            }

            private void drawTextWithShadow(TextBitmap tex, float x, float y, float maxX, float r, float g, float b, float a) {
                if (tex.textureId == 0 || tex.width <= 0 || tex.height <= 0) return;
                float drawW = tex.width;
                float drawH = tex.height;
                if (x + drawW > maxX) {
                    // Texture already ellipsized to max width, but clamp just in case.
                    drawW = Math.max(0.0f, maxX - x);
                    if (drawW <= 1.0f) return;
                }

                GLES20.glActiveTexture(GLES20.GL_TEXTURE0);
                GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, tex.textureId);

                // Shadow
                drawQuad(x + 1.0f, y + 1.0f, drawW, drawH, 0.0f, 0.0f, 0.0f, a * 0.35f);
                // Main
                drawQuad(x, y, drawW, drawH, r, g, b, a);
            }

            private void drawQuad(float x, float y, float w, float h, float r, float g, float b, float a) {
                GLES20.glUniform4f(uniformColor, r, g, b, a);

                quad.position(0);
                // Triangle strip: (x,y) is top-left in UI coords
                // glTexImage2D treats the first row in the buffer as the bottom row.
                // Our TinyFont raster writes rows top-to-bottom, so map top of quad to v=0.
                quad.put(x).put(y).put(0.0f).put(0.0f);
                quad.put(x + w).put(y).put(1.0f).put(0.0f);
                quad.put(x).put(y + h).put(0.0f).put(1.0f);
                quad.put(x + w).put(y + h).put(1.0f).put(1.0f);
                quad.position(0);

                int stride = 4 * 4;
                GLES20.glEnableVertexAttribArray(attrPosPx);
                GLES20.glVertexAttribPointer(attrPosPx, 2, GLES20.GL_FLOAT, false, stride, quad);
                quad.position(2);
                GLES20.glEnableVertexAttribArray(attrUv);
                GLES20.glVertexAttribPointer(attrUv, 2, GLES20.GL_FLOAT, false, stride, quad);
                quad.position(0);
                GLES20.glDrawArrays(GLES20.GL_TRIANGLE_STRIP, 0, 4);
                GLES20.glDisableVertexAttribArray(attrPosPx);
                GLES20.glDisableVertexAttribArray(attrUv);
            }

            private static String safe(String v) {
                return v == null ? "" : v;
            }

            private static String normalizeAscii(String v) {
                if (v == null || v.isEmpty()) {
                    return "";
                }
                StringBuilder sb = new StringBuilder(v.length());
                for (int i = 0; i < v.length(); i++) {
                    char c = v.charAt(i);
                    if (c == '\r' || c == '\n' || c == '\t') {
                        sb.append(' ');
                        continue;
                    }
                    if (c < 32 || c > 126) {
                        sb.append('?');
                        continue;
                    }
                    sb.append(Character.toUpperCase(c));
                }
                return sb.toString();
            }
        }

        private static final class TinyFont {
            private static final String ELLIPSIS = "...";
            private static final int GLYPH_WIDTH = 5;
            private static final int GLYPH_HEIGHT = 7;
            private final int scale;

            TinyFont(int scale) {
                this.scale = clampInt(scale, 1, 8);
            }

            static int scaleForTargetPx(float px) {
                if (!Float.isFinite(px) || px <= 0.0f) {
                    return 2;
                }
                // glyph height is 7 * scale, pick a scale close to target while staying readable.
                return clampInt(Math.round(px / 8.0f), 1, 8);
            }

            int cellW() {
                return (GLYPH_WIDTH + 1) * scale;
            }

            int glyphW() {
                return GLYPH_WIDTH * scale;
            }

            int glyphH() {
                return GLYPH_HEIGHT * scale;
            }

            int scale() {
                return scale;
            }

            static String ellipsizeMonospace(String text, int maxChars) {
                if (text == null) return "";
                String raw = text.trim();
                if (raw.isEmpty() || maxChars <= 0) return "";
                if (raw.length() <= maxChars) return raw;
                if (maxChars <= ELLIPSIS.length()) return "";
                return raw.substring(0, Math.max(0, maxChars - ELLIPSIS.length())) + ELLIPSIS;
            }

            static int[] glyphRows(char ch) {
                char c = Character.toUpperCase(ch);
                switch (c) {
                    case 'A':
                        return GLYPH_A;
                    case 'B':
                        return GLYPH_B;
                    case 'C':
                        return GLYPH_C;
                    case 'D':
                        return GLYPH_D;
                    case 'E':
                        return GLYPH_E;
                    case 'F':
                        return GLYPH_F;
                    case 'G':
                        return GLYPH_G;
                    case 'H':
                        return GLYPH_H;
                    case 'I':
                        return GLYPH_I;
                    case 'J':
                        return GLYPH_J;
                    case 'K':
                        return GLYPH_K;
                    case 'L':
                        return GLYPH_L;
                    case 'M':
                        return GLYPH_M;
                    case 'N':
                        return GLYPH_N;
                    case 'O':
                        return GLYPH_O;
                    case 'P':
                        return GLYPH_P;
                    case 'Q':
                        return GLYPH_Q;
                    case 'R':
                        return GLYPH_R;
                    case 'S':
                        return GLYPH_S;
                    case 'T':
                        return GLYPH_T;
                    case 'U':
                        return GLYPH_U;
                    case 'V':
                        return GLYPH_V;
                    case 'W':
                        return GLYPH_W;
                    case 'X':
                        return GLYPH_X;
                    case 'Y':
                        return GLYPH_Y;
                    case 'Z':
                        return GLYPH_Z;
                    case '0':
                        return GLYPH_0;
                    case '1':
                        return GLYPH_1;
                    case '2':
                        return GLYPH_2;
                    case '3':
                        return GLYPH_3;
                    case '4':
                        return GLYPH_4;
                    case '5':
                        return GLYPH_5;
                    case '6':
                        return GLYPH_6;
                    case '7':
                        return GLYPH_7;
                    case '8':
                        return GLYPH_8;
                    case '9':
                        return GLYPH_9;
                    case '.':
                        return GLYPH_DOT;
                    case ':':
                        return GLYPH_COLON;
                    case '-':
                        return GLYPH_DASH;
                    case '_':
                        return GLYPH_UNDERSCORE;
                    case '/':
                        return GLYPH_SLASH;
                    case ' ':
                        return GLYPH_SPACE;
                    default:
                        return GLYPH_QMARK;
                }
            }

            private static int clampInt(int v, int min, int max) {
                return Math.max(min, Math.min(max, v));
            }

            private static final int[] GLYPH_SPACE = new int[]{0, 0, 0, 0, 0, 0, 0};
            private static final int[] GLYPH_DOT = new int[]{0, 0, 0, 0, 0, 0b00100, 0b00100};
            private static final int[] GLYPH_COLON = new int[]{0, 0b00100, 0b00100, 0, 0b00100, 0b00100, 0};
            private static final int[] GLYPH_DASH = new int[]{0, 0, 0, 0b01110, 0, 0, 0};
            private static final int[] GLYPH_UNDERSCORE = new int[]{0, 0, 0, 0, 0, 0, 0b11111};
            private static final int[] GLYPH_SLASH = new int[]{0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0, 0};
            private static final int[] GLYPH_QMARK = new int[]{0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0, 0b00100};

            private static final int[] GLYPH_0 = new int[]{0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110};
            private static final int[] GLYPH_1 = new int[]{0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110};
            private static final int[] GLYPH_2 = new int[]{0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111};
            private static final int[] GLYPH_3 = new int[]{0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110};
            private static final int[] GLYPH_4 = new int[]{0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010};
            private static final int[] GLYPH_5 = new int[]{0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110};
            private static final int[] GLYPH_6 = new int[]{0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110};
            private static final int[] GLYPH_7 = new int[]{0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000};
            private static final int[] GLYPH_8 = new int[]{0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110};
            private static final int[] GLYPH_9 = new int[]{0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110};

            private static final int[] GLYPH_A = new int[]{0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001};
            private static final int[] GLYPH_B = new int[]{0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110};
            private static final int[] GLYPH_C = new int[]{0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110};
            private static final int[] GLYPH_D = new int[]{0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110};
            private static final int[] GLYPH_E = new int[]{0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111};
            private static final int[] GLYPH_F = new int[]{0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000};
            private static final int[] GLYPH_G = new int[]{0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110};
            private static final int[] GLYPH_H = new int[]{0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001};
            private static final int[] GLYPH_I = new int[]{0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110};
            private static final int[] GLYPH_J = new int[]{0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100};
            private static final int[] GLYPH_K = new int[]{0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001};
            private static final int[] GLYPH_L = new int[]{0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111};
            private static final int[] GLYPH_M = new int[]{0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001};
            private static final int[] GLYPH_N = new int[]{0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001};
            private static final int[] GLYPH_O = new int[]{0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110};
            private static final int[] GLYPH_P = new int[]{0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000};
            private static final int[] GLYPH_Q = new int[]{0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101};
            private static final int[] GLYPH_R = new int[]{0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001};
            private static final int[] GLYPH_S = new int[]{0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110};
            private static final int[] GLYPH_T = new int[]{0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100};
            private static final int[] GLYPH_U = new int[]{0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110};
            private static final int[] GLYPH_V = new int[]{0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100};
            private static final int[] GLYPH_W = new int[]{0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001};
            private static final int[] GLYPH_X = new int[]{0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001};
            private static final int[] GLYPH_Y = new int[]{0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100};
            private static final int[] GLYPH_Z = new int[]{0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111};
        }

        private static final class TextBitmap {
            int textureId = 0;
            int texWidth = 0;
            int texHeight = 0;
            int width = 0;
            int height = 0;
            private ByteBuffer buffer;
            private String lastText = "";
            private int lastMaxWidth = -1;

            void ensureTexture() {
                if (textureId != 0) return;
                int[] ids = new int[1];
                GLES20.glGenTextures(1, ids, 0);
                textureId = ids[0];
                GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, textureId);
                GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MIN_FILTER, GLES20.GL_LINEAR);
                GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MAG_FILTER, GLES20.GL_LINEAR);
                GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_S, GLES20.GL_CLAMP_TO_EDGE);
                GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_T, GLES20.GL_CLAMP_TO_EDGE);
            }

            void releaseGlQuietly() {
                try {
                    if (textureId != 0) {
                        int[] ids = new int[]{textureId};
                        GLES20.glDeleteTextures(1, ids, 0);
                        textureId = 0;
                    }
                } catch (Throwable ignored) {
                }
            }

            void updateText(String text, TinyFont font, int maxWidth) {
                ensureTexture();
                String safe = text == null ? "" : text.trim();
                int scale = font == null ? 2 : font.scale();
                if (maxWidth <= 0 || safe.isEmpty()) {
                    width = 0;
                    height = 0;
                    lastText = "";
                    lastMaxWidth = maxWidth;
                    return;
                }
                if (maxWidth == lastMaxWidth && safe.equals(lastText)) {
                    return;
                }

                String render = font == null
                        ? safe
                        : TinyFont.ellipsizeMonospace(safe, Math.max(1, maxWidth / Math.max(1, font.cellW())));
                int texW = Math.max(1, maxWidth);
                int texH = Math.max(1, (font == null ? (TinyFont.GLYPH_HEIGHT * scale) : font.glyphH()));
                ensureBuffer(texW, texH);
                clearBuffer(texW * texH);

                int contentW = 0;
                if (!render.isEmpty() && font != null) {
                    int x = 0;
                    for (int i = 0; i < render.length(); i++) {
                        drawGlyph(font, render.charAt(i), texW, texH, x, 0);
                        x += font.cellW();
                        if (x >= texW) {
                            break;
                        }
                    }
                    contentW = Math.min(texW, Math.max(1, x - font.scale()));
                }

                GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, textureId);
                GLES20.glPixelStorei(GLES20.GL_UNPACK_ALIGNMENT, TextRenderer.ATLAS_UNPACK_ALIGNMENT);
                buffer.position(0);
                if (texW != texWidth || texH != texHeight) {
                    GLES20.glTexImage2D(
                            GLES20.GL_TEXTURE_2D,
                            0,
                            GLES20.GL_ALPHA,
                            texW,
                            texH,
                            0,
                            GLES20.GL_ALPHA,
                            GLES20.GL_UNSIGNED_BYTE,
                            buffer
                    );
                } else {
                    GLES20.glTexSubImage2D(
                            GLES20.GL_TEXTURE_2D,
                            0,
                            0,
                            0,
                            texW,
                            texH,
                            GLES20.GL_ALPHA,
                            GLES20.GL_UNSIGNED_BYTE,
                            buffer
                    );
                }
                texWidth = texW;
                texHeight = texH;
                width = contentW;
                height = texH;
                lastText = safe;
                lastMaxWidth = maxWidth;
            }

            private void ensureBuffer(int w, int h) {
                int size = w * h;
                if (buffer != null && buffer.capacity() >= size) {
                    return;
                }
                buffer = ByteBuffer.allocateDirect(size);
            }

            private void clearBuffer(int size) {
                buffer.position(0);
                for (int i = 0; i < size; i++) {
                    buffer.put((byte) 0);
                }
                buffer.position(0);
            }

            private void drawGlyph(TinyFont font, char ch, int texW, int texH, int x0, int y0) {
                int[] rows = TinyFont.glyphRows(ch);
                int scale = font.scale();
                for (int ry = 0; ry < TinyFont.GLYPH_HEIGHT; ry++) {
                    int row = rows[ry] & 0x1f;
                    for (int rx = 0; rx < TinyFont.GLYPH_WIDTH; rx++) {
                        int bit = 1 << (TinyFont.GLYPH_WIDTH - 1 - rx);
                        if ((row & bit) == 0) {
                            continue;
                        }
                        int px0 = x0 + rx * scale;
                        int py0 = y0 + ry * scale;
                        for (int sy = 0; sy < scale; sy++) {
                            int py = py0 + sy;
                            if (py < 0 || py >= texH) continue;
                            int base = py * texW;
                            for (int sx = 0; sx < scale; sx++) {
                                int px = px0 + sx;
                                if (px < 0 || px >= texW) continue;
                                buffer.put(base + px, (byte) 0xff);
                            }
                        }
                    }
                }
            }
        }
    }
}
