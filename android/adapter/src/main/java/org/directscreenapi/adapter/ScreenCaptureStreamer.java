package org.directscreenapi.adapter;

import java.io.IOException;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.nio.ByteBuffer;
import java.util.Locale;

final class ScreenCaptureStreamer {
    private static final int DEFAULT_TARGET_FPS = 60;
    private static final int VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR = 16;
    private static final int MAX_IMAGES = 3;
    private static final String SUBMIT_MODE_ENV = "DSAPI_SCREEN_SUBMIT_MODE";

    private final String controlSocketPath;
    private final int targetFps;
    private final String submitMode;
    private final boolean dmabufSubmitEnabled;

    private final Object runLock = new Object();

    private DaemonSession session;
    private Object imageReader;
    private Object virtualDisplay;
    private Object imageListener;
    private Object imageListenerHandlerThread;
    private Object imageListenerHandler;

    private volatile boolean stopping;
    private long lastSubmitNs;
    private long perfWindowStartNs;
    private long perfFrames;
    private long perfBytes;
    private long lastFrameSeq;
    private boolean submitPathLogged;

    ScreenCaptureStreamer(String controlSocketPath, int targetFps) {
        this.controlSocketPath = controlSocketPath;
        this.targetFps = targetFps > 0 ? targetFps : DEFAULT_TARGET_FPS;
        String modeRaw = System.getenv(SUBMIT_MODE_ENV);
        String mode = modeRaw == null ? "dmabuf" : modeRaw.trim().toLowerCase(Locale.US);
        if (mode.isEmpty()) {
            mode = "dmabuf";
        }
        this.submitMode = mode;
        this.dmabufSubmitEnabled = !"shm".equals(mode);
    }

    void runLoop() throws Exception {
        Runtime.getRuntime().addShutdownHook(new Thread(this::shutdown, "dsapi-screen-stream-shutdown"));
        setup();
        log("screen_stream_status=started target_fps=" + targetFps + " submit_mode=" + submitMode);
        synchronized (runLock) {
            while (!stopping) {
                runLock.wait();
            }
        }
        log("screen_stream_status=stopped");
    }

    private void setup() throws Exception {
        DisplayAdapter.DisplaySnapshot snapshot = new AndroidDisplayAdapter().queryDisplaySnapshot();
        if (snapshot.width <= 0 || snapshot.height <= 0 || snapshot.densityDpi <= 0) {
            throw new IOException("screen_stream_display_snapshot_invalid");
        }

        session = new DaemonSession(controlSocketPath, false);
        String displaySet = String.format(
                Locale.US,
                "DISPLAY_SET %d %d %.2f %d %d",
                snapshot.width,
                snapshot.height,
                snapshot.refreshHz > 0f ? snapshot.refreshHz : 60f,
                snapshot.densityDpi,
                Math.max(0, snapshot.rotation)
        );
        session.command(displaySet);
        session.ensureFrameShmBound();
        if (dmabufSubmitEnabled && !HardwareBufferBridge.isAvailable()) {
            throw new IOException("screen_stream_dmabuf_bridge_unavailable");
        }

        Object context = buildSystemContext();
        startImageListenerThread();
        imageReader = createImageReader(snapshot.width, snapshot.height);
        installImageAvailableListener(imageReader);
        virtualDisplay = createVirtualDisplay(
                context,
                snapshot.width,
                snapshot.height,
                snapshot.densityDpi,
                imageReader
        );
        if (virtualDisplay == null) {
            throw new IOException("screen_stream_virtual_display_create_failed");
        }
        lastSubmitNs = System.nanoTime();
    }

    void shutdown() {
        if (stopping) {
            return;
        }
        stopping = true;

        if (imageReader != null && imageListener != null) {
            try {
                ReflectBridge.invoke(imageReader, "setOnImageAvailableListener", null, null);
            } catch (Throwable ignored) {
            }
        }

        if (virtualDisplay != null) {
            try {
                ReflectBridge.invoke(virtualDisplay, "release");
            } catch (Throwable ignored) {
            }
            virtualDisplay = null;
        }

        if (imageReader != null) {
            try {
                ReflectBridge.invoke(imageReader, "close");
            } catch (Throwable ignored) {
            }
            imageReader = null;
        }

        if (imageListenerHandlerThread != null) {
            try {
                ReflectBridge.invoke(imageListenerHandlerThread, "quitSafely");
            } catch (Throwable ignored) {
                try {
                    ReflectBridge.invoke(imageListenerHandlerThread, "quit");
                } catch (Throwable ignoredAgain) {
                }
            }
            imageListenerHandlerThread = null;
        }
        imageListenerHandler = null;
        imageListener = null;

        if (session != null) {
            session.closeQuietly();
            session = null;
        }

        synchronized (runLock) {
            runLock.notifyAll();
        }
    }

    private void startImageListenerThread() throws Exception {
        Class<?> handlerThreadClass = Class.forName("android.os.HandlerThread");
        imageListenerHandlerThread = handlerThreadClass
                .getDeclaredConstructor(String.class)
                .newInstance("dsapi-screen-image-listener");
        ReflectBridge.invoke(imageListenerHandlerThread, "start");
        Object looper = ReflectBridge.invoke(imageListenerHandlerThread, "getLooper");

        Class<?> handlerClass = Class.forName("android.os.Handler");
        Class<?> looperClass = Class.forName("android.os.Looper");
        imageListenerHandler = handlerClass
                .getDeclaredConstructor(looperClass)
                .newInstance(looper);
    }

    private static Object buildSystemContext() throws Exception {
        Class<?> looperClass = Class.forName("android.os.Looper");
        Object myLooper = ReflectBridge.invokeStatic(looperClass, "myLooper");
        if (myLooper == null) {
            ReflectBridge.invokeStatic(looperClass, "prepare");
        }

        Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
        Object activityThread = ReflectBridge.invokeStatic(activityThreadClass, "systemMain");
        Object context = ReflectBridge.invoke(activityThread, "getSystemContext");
        if (context == null) {
            throw new IOException("screen_stream_system_context_unavailable");
        }
        return context;
    }

    private static Object createImageReader(int width, int height) throws Exception {
        Class<?> pixelFormatClass = Class.forName("android.graphics.PixelFormat");
        int rgba8888 = ((Integer) pixelFormatClass.getField("RGBA_8888").get(null)).intValue();
        Class<?> imageReaderClass = Class.forName("android.media.ImageReader");
        return ReflectBridge.invokeStatic(
                imageReaderClass,
                "newInstance",
                Integer.valueOf(width),
                Integer.valueOf(height),
                Integer.valueOf(rgba8888),
                Integer.valueOf(MAX_IMAGES)
        );
    }

    private Object createVirtualDisplay(
            Object context,
            int width,
            int height,
            int densityDpi,
            Object reader
    ) throws Exception {
        Object surface = ReflectBridge.invoke(reader, "getSurface");

        Class<?> builderClass = Class.forName("android.hardware.display.VirtualDisplayConfig$Builder");
        Object builder = builderClass
                .getDeclaredConstructor(String.class, int.class, int.class, int.class)
                .newInstance("DirectScreenAPI-Capture", width, height, densityDpi);
        ReflectBridge.invoke(builder, "setSurface", surface);
        ReflectBridge.invoke(builder, "setFlags", Integer.valueOf(VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR));
        try {
            ReflectBridge.invoke(builder, "setDisplayIdToMirror", Integer.valueOf(-1));
        } catch (Throwable ignored) {
        }
        try {
            ReflectBridge.invoke(builder, "setWindowManagerMirroringEnabled", Boolean.FALSE);
        } catch (Throwable ignored) {
        }
        if (targetFps > 0) {
            try {
                ReflectBridge.invoke(builder, "setRequestedRefreshRate", Float.valueOf((float) targetFps));
            } catch (Throwable ignored) {
            }
        }
        Object config = ReflectBridge.invoke(builder, "build");

        Class<?> dmgClass = Class.forName("android.hardware.display.DisplayManagerGlobal");
        Object dmg = ReflectBridge.invokeStatic(dmgClass, "getInstance");
        return ReflectBridge.invoke(dmg, "createVirtualDisplay", context, null, config, null, null);
    }

    private void installImageAvailableListener(Object reader) throws Exception {
        Class<?> listenerIface = Class.forName("android.media.ImageReader$OnImageAvailableListener");
        InvocationHandler handler = (proxy, method, args) -> {
            if ("onImageAvailable".equals(method.getName()) && args != null && args.length == 1) {
                onImageAvailable(args[0]);
            }
            return null;
        };
        imageListener = Proxy.newProxyInstance(
                listenerIface.getClassLoader(),
                new Class<?>[]{listenerIface},
                handler
        );
        ReflectBridge.invoke(reader, "setOnImageAvailableListener", imageListener, imageListenerHandler);
    }

    private void onImageAvailable(Object reader) {
        if (stopping || reader == null) {
            return;
        }

        Object image = null;
        try {
            image = ReflectBridge.invoke(reader, "acquireLatestImage");
            if (image == null) {
                return;
            }
            maybeSubmitImage(image);
        } catch (Throwable t) {
            log("screen_stream_error=on_image_available_failed err=" + t.getClass().getSimpleName() + ":" + t.getMessage());
            shutdown();
        } finally {
            if (image != null) {
                try {
                    ReflectBridge.invoke(image, "close");
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private void maybeSubmitImage(Object image) throws Exception {
        int width = ((Integer) ReflectBridge.invoke(image, "getWidth")).intValue();
        int height = ((Integer) ReflectBridge.invoke(image, "getHeight")).intValue();
        if (width <= 0 || height <= 0) {
            return;
        }

        Object[] planes = (Object[]) ReflectBridge.invoke(image, "getPlanes");
        if (planes == null || planes.length < 1 || planes[0] == null) {
            return;
        }
        Object plane0 = planes[0];
        ByteBuffer src = (ByteBuffer) ReflectBridge.invoke(plane0, "getBuffer");
        if (src == null) {
            return;
        }
        int rowStride = ((Integer) ReflectBridge.invoke(plane0, "getRowStride")).intValue();
        int pixelStride = ((Integer) ReflectBridge.invoke(plane0, "getPixelStride")).intValue();
        if (rowStride <= 0 || pixelStride <= 0) {
            return;
        }

        long nowNs = System.nanoTime();
        long intervalNs = targetFps > 0 ? (1_000_000_000L / (long) targetFps) : 0L;
        if (intervalNs > 0L && lastSubmitNs > 0L && nowNs - lastSubmitNs < intervalNs) {
            return;
        }

        long frameSeq;
        if (dmabufSubmitEnabled) {
            if (!submitPathLogged) {
                log("screen_stream_submit_path=dmabuf");
                submitPathLogged = true;
            }
            HardwareBufferBridge.NativeFrame nativeFrame = tryExportHardwareBuffer(image);
            if (nativeFrame == null || nativeFrame.fd == null) {
                throw new IOException("screen_stream_dmabuf_export_failed");
            }
            try {
                frameSeq = session.frameSubmitDmabuf(
                        nativeFrame.width,
                        nativeFrame.height,
                        nativeFrame.stride,
                        nativeFrame.format,
                        nativeFrame.usage,
                        nativeFrame.byteLen,
                        nativeFrame.byteOffset,
                        0,
                        0,
                        nativeFrame.fd
                );
            } finally {
                HardwareBufferBridge.closeQuietly(nativeFrame.fd);
            }
        } else if (pixelStride >= 4 && rowStride >= width * 4) {
            if (!submitPathLogged) {
                log("screen_stream_submit_path=shm_plane");
                submitPathLogged = true;
            }
            frameSeq = session.frameSubmitBoundPlane(width, height, src, rowStride, pixelStride);
        } else {
            throw new IOException("screen_stream_plane_layout_unsupported");
        }
        lastFrameSeq = frameSeq;
        lastSubmitNs = nowNs;
        perfFrames += 1L;
        perfBytes += (long) width * (long) height * 4L;
        logPerf(nowNs);
    }

    private void logPerf(long nowNs) {
        if (perfWindowStartNs == 0L) {
            perfWindowStartNs = nowNs;
            return;
        }
        long elapsedNs = nowNs - perfWindowStartNs;
        if (elapsedNs < 1_000_000_000L) {
            return;
        }
        double sec = elapsedNs / 1_000_000_000.0;
        double fps = perfFrames / sec;
        double mibPerSec = (perfBytes / (1024.0 * 1024.0)) / sec;
        log(
                String.format(
                        Locale.US,
                        "screen_stream_perf fps=%.1f throughput_mib_s=%.1f frame_seq=%d",
                        fps,
                        mibPerSec,
                        lastFrameSeq
                )
        );
        perfWindowStartNs = nowNs;
        perfFrames = 0L;
        perfBytes = 0L;
    }

    private static void log(String line) {
        System.out.println(line);
    }

    private HardwareBufferBridge.NativeFrame tryExportHardwareBuffer(Object image) {
        if (image == null) {
            return null;
        }
        try {
            Object hardwareBuffer = ReflectBridge.invoke(image, "getHardwareBuffer");
            if (hardwareBuffer == null) {
                return null;
            }
            return HardwareBufferBridge.exportFrame(hardwareBuffer, lastFrameSeq + 1L);
        } catch (Throwable ignored) {
            return null;
        }
    }
}
