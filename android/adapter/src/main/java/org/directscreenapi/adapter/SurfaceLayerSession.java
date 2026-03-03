package org.directscreenapi.adapter;

import java.lang.reflect.Constructor;
import java.lang.reflect.Method;

final class SurfaceLayerSession {
    private final Object surfaceControl;
    private final Object surface;
    private final Class<?> transactionClass;
    private final Constructor<?> rectConstructor;
    private final Method lockCanvasMethod;
    private final Method lockHardwareCanvasMethod;
    private final Method unlockCanvasAndPostMethod;

    private SurfaceLayerSession(
            Object surfaceControl,
            Object surface,
            Class<?> transactionClass,
            Constructor<?> rectConstructor,
            Method lockCanvasMethod,
            Method lockHardwareCanvasMethod,
            Method unlockCanvasAndPostMethod
    ) {
        this.surfaceControl = surfaceControl;
        this.surface = surface;
        this.transactionClass = transactionClass;
        this.rectConstructor = rectConstructor;
        this.lockCanvasMethod = lockCanvasMethod;
        this.lockHardwareCanvasMethod = lockHardwareCanvasMethod;
        this.unlockCanvasAndPostMethod = unlockCanvasAndPostMethod;
    }

    static SurfaceLayerSession create(int width, int height, int zLayer, String layerName) throws Exception {
        return create(width, height, zLayer, layerName, true, 0, 0.0f);
    }

    static SurfaceLayerSession create(
            int width,
            int height,
            int zLayer,
            String layerName,
            boolean visible
    ) throws Exception {
        return create(width, height, zLayer, layerName, visible, 0, 0.0f);
    }

    static SurfaceLayerSession create(
            int width,
            int height,
            int zLayer,
            String layerName,
            boolean visible,
            int blurRadius
    ) throws Exception {
        return create(width, height, zLayer, layerName, visible, blurRadius, 0.0f);
    }

    static SurfaceLayerSession create(
            int width,
            int height,
            int zLayer,
            String layerName,
            boolean visible,
            int blurRadius,
            float frameRateHz
    ) throws Exception {
        Class<?> builderClass = Class.forName("android.view.SurfaceControl$Builder");
        Class<?> transactionClass = Class.forName("android.view.SurfaceControl$Transaction");
        Class<?> rectClass = Class.forName("android.graphics.Rect");
        Class<?> surfaceClass = Class.forName("android.view.Surface");
        Class<?> surfaceControlClass = Class.forName("android.view.SurfaceControl");
        Constructor<?> rectConstructor = rectClass.getDeclaredConstructor(
                int.class,
                int.class,
                int.class,
                int.class
        );

        Object sc = null;
        Object txShow = null;
        Object surface = null;
        try {
            Object builder = builderClass.getDeclaredConstructor().newInstance();
            ReflectBridge.invoke(builder, "setName", layerName);
            try {
                ReflectBridge.invoke(builder, "setCallsite", "DirectScreenAPI");
            } catch (Throwable ignored) {
            }
            ReflectBridge.invoke(builder, "setBufferSize", Integer.valueOf(width), Integer.valueOf(height));
            ReflectBridge.invoke(builder, "setFormat", Integer.valueOf(1)); // RGBA_8888
            ReflectBridge.invoke(builder, "setOpaque", Boolean.FALSE);
            ReflectBridge.invoke(builder, "setHidden", Boolean.valueOf(!visible));
            sc = ReflectBridge.invoke(builder, "build");

            txShow = transactionClass.getDeclaredConstructor().newInstance();
            ReflectBridge.invoke(txShow, "setLayer", sc, Integer.valueOf(zLayer));
            ReflectBridge.invoke(txShow, "setPosition", sc, Float.valueOf(0f), Float.valueOf(0f));
            Object fullRect = rectConstructor.newInstance(0, 0, width, height);
            ReflectBridge.invoke(txShow, "setWindowCrop", sc, fullRect);
            trySetFrameRate(txShow, sc, frameRateHz);
            try {
                ReflectBridge.invoke(txShow, "setTrustedOverlay", sc, Boolean.TRUE);
            } catch (Throwable ignored) {
            }
            if (blurRadius > 0) {
                try {
                    ReflectBridge.invoke(txShow, "setBackgroundBlurRadius", sc, Integer.valueOf(blurRadius));
                } catch (Throwable ignored) {
                }
            }
            if (visible) {
                ReflectBridge.invoke(txShow, "show", sc);
            }
            ReflectBridge.invoke(txShow, "apply");
            ReflectBridge.invoke(txShow, "close");
            txShow = null;

            surface = createSurfaceFromSurfaceControl(surfaceClass, surfaceControlClass, sc);
            Method lockCanvas = ReflectBridge.findMethodByArity(surfaceClass, "lockCanvas", 1);
            Method unlockCanvasAndPost = ReflectBridge.findMethodByArity(surfaceClass, "unlockCanvasAndPost", 1);
            Method lockHardwareCanvas = null;
            try {
                lockHardwareCanvas = ReflectBridge.findMethodByArity(surfaceClass, "lockHardwareCanvas", 0);
            } catch (Throwable ignored) {
            }

            return new SurfaceLayerSession(
                    sc,
                    surface,
                    transactionClass,
                    rectConstructor,
                    lockCanvas,
                    lockHardwareCanvas,
                    unlockCanvasAndPost
            );
        } catch (Throwable t) {
            closeTransactionQuietly(txShow);
            releaseSurfaceQuietly(surface);
            removeSurfaceControlQuietly(transactionClass, sc);
            releaseSurfaceControlQuietly(sc);
            throw t;
        }
    }

    void show() throws Exception {
        Object tx = transactionClass.getDeclaredConstructor().newInstance();
        ReflectBridge.invoke(tx, "show", surfaceControl);
        ReflectBridge.invoke(tx, "apply");
        ReflectBridge.invoke(tx, "close");
    }

    void setBackgroundBlurRadius(int blurRadius) throws Exception {
        int safeRadius = Math.max(0, blurRadius);
        Object tx = transactionClass.getDeclaredConstructor().newInstance();
        try {
            ReflectBridge.invoke(tx, "setBackgroundBlurRadius", surfaceControl, Integer.valueOf(safeRadius));
            ReflectBridge.invoke(tx, "apply");
        } finally {
            try {
                ReflectBridge.invoke(tx, "close");
            } catch (Throwable ignored) {
            }
        }
    }

    void setWindowCrop(int left, int top, int width, int height) throws Exception {
        int safeLeft = Math.max(0, left);
        int safeTop = Math.max(0, top);
        int safeWidth = Math.max(1, width);
        int safeHeight = Math.max(1, height);
        Object crop = rectConstructor.newInstance(
                safeLeft,
                safeTop,
                safeLeft + safeWidth,
                safeTop + safeHeight
        );
        Object tx = transactionClass.getDeclaredConstructor().newInstance();
        try {
            ReflectBridge.invoke(tx, "setWindowCrop", surfaceControl, crop);
            ReflectBridge.invoke(tx, "apply");
        } finally {
            try {
                ReflectBridge.invoke(tx, "close");
            } catch (Throwable ignored) {
            }
        }
    }

    void setPosition(float x, float y) throws Exception {
        Object tx = transactionClass.getDeclaredConstructor().newInstance();
        try {
            ReflectBridge.invoke(tx, "setPosition", surfaceControl, Float.valueOf(x), Float.valueOf(y));
            ReflectBridge.invoke(tx, "apply");
        } finally {
            try {
                ReflectBridge.invoke(tx, "close");
            } catch (Throwable ignored) {
            }
        }
    }

    void setFrameRate(float frameRateHz) throws Exception {
        Object tx = transactionClass.getDeclaredConstructor().newInstance();
        try {
            trySetFrameRate(tx, surfaceControl, frameRateHz);
            ReflectBridge.invoke(tx, "apply");
        } finally {
            try {
                ReflectBridge.invoke(tx, "close");
            } catch (Throwable ignored) {
            }
        }
    }

    Object lockFrame() throws Exception {
        if (lockCanvasMethod != null) {
            try {
                return lockCanvasMethod.invoke(surface, new Object[]{null});
            } catch (Throwable ignored) {
            }
        }
        if (lockHardwareCanvasMethod != null) {
            return lockHardwareCanvasMethod.invoke(surface);
        }
        throw new IllegalStateException("surface_lock_canvas_unavailable");
    }

    void unlockFrame(Object canvas) throws Exception {
        unlockCanvasAndPostMethod.invoke(surface, canvas);
    }

    void closeQuietly() {
        try {
            Object txRemove = transactionClass.getDeclaredConstructor().newInstance();
            ReflectBridge.invoke(txRemove, "remove", surfaceControl);
            ReflectBridge.invoke(txRemove, "apply");
            ReflectBridge.invoke(txRemove, "close");
        } catch (Throwable ignored) {
        }
        try {
            ReflectBridge.invoke(surface, "release");
        } catch (Throwable ignored) {
        }
        try {
            ReflectBridge.invoke(surfaceControl, "release");
        } catch (Throwable ignored) {
        }
    }

    private static Object createSurfaceFromSurfaceControl(
            Class<?> surfaceClass,
            Class<?> surfaceControlClass,
            Object surfaceControl
    ) throws Exception {
        for (Constructor<?> c : surfaceClass.getDeclaredConstructors()) {
            Class<?>[] params = c.getParameterTypes();
            if (params.length == 1 && params[0].getName().equals(surfaceControlClass.getName())) {
                c.setAccessible(true);
                return c.newInstance(surfaceControl);
            }
        }

        Object surface = surfaceClass.getDeclaredConstructor().newInstance();
        Method copyFrom = ReflectBridge.findMethod(surfaceClass, "copyFrom", surfaceControl);
        copyFrom.invoke(surface, surfaceControl);
        return surface;
    }

    private static void closeTransactionQuietly(Object tx) {
        if (tx == null) {
            return;
        }
        try {
            ReflectBridge.invoke(tx, "close");
        } catch (Throwable ignored) {
        }
    }

    private static void removeSurfaceControlQuietly(Class<?> transactionClass, Object surfaceControl) {
        if (surfaceControl == null) {
            return;
        }
        try {
            Object txRemove = transactionClass.getDeclaredConstructor().newInstance();
            try {
                ReflectBridge.invoke(txRemove, "remove", surfaceControl);
                ReflectBridge.invoke(txRemove, "apply");
            } finally {
                closeTransactionQuietly(txRemove);
            }
        } catch (Throwable ignored) {
        }
    }

    private static void releaseSurfaceQuietly(Object surface) {
        if (surface == null) {
            return;
        }
        try {
            ReflectBridge.invoke(surface, "release");
        } catch (Throwable ignored) {
        }
    }

    private static void releaseSurfaceControlQuietly(Object surfaceControl) {
        if (surfaceControl == null) {
            return;
        }
        try {
            ReflectBridge.invoke(surfaceControl, "release");
        } catch (Throwable ignored) {
        }
    }

    private static void trySetFrameRate(Object tx, Object surfaceControl, float frameRateHz) {
        if (tx == null || surfaceControl == null) {
            return;
        }
        if (!Float.isFinite(frameRateHz) || frameRateHz <= 0.0f) {
            return;
        }
        try {
            ReflectBridge.invoke(
                    tx,
                    "setFrameRate",
                    surfaceControl,
                    Float.valueOf(frameRateHz),
                    Integer.valueOf(1),
                    Integer.valueOf(1)
            );
            return;
        } catch (Throwable ignored) {
        }
        try {
            ReflectBridge.invoke(
                    tx,
                    "setFrameRate",
                    surfaceControl,
                    Float.valueOf(frameRateHz),
                    Integer.valueOf(1)
            );
        } catch (Throwable ignored) {
        }
    }
}
