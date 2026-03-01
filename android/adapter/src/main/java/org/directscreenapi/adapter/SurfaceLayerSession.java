package org.directscreenapi.adapter;

import java.lang.reflect.Constructor;
import java.lang.reflect.Method;

final class SurfaceLayerSession {
    private final Object surfaceControl;
    private final Object surface;
    private final Class<?> transactionClass;
    private final Method lockCanvasMethod;
    private final Method lockHardwareCanvasMethod;
    private final Method unlockCanvasAndPostMethod;

    private SurfaceLayerSession(
            Object surfaceControl,
            Object surface,
            Class<?> transactionClass,
            Method lockCanvasMethod,
            Method lockHardwareCanvasMethod,
            Method unlockCanvasAndPostMethod
    ) {
        this.surfaceControl = surfaceControl;
        this.surface = surface;
        this.transactionClass = transactionClass;
        this.lockCanvasMethod = lockCanvasMethod;
        this.lockHardwareCanvasMethod = lockHardwareCanvasMethod;
        this.unlockCanvasAndPostMethod = unlockCanvasAndPostMethod;
    }

    static SurfaceLayerSession create(int width, int height, int zLayer, String layerName) throws Exception {
        return create(width, height, zLayer, layerName, true);
    }

    static SurfaceLayerSession create(
            int width,
            int height,
            int zLayer,
            String layerName,
            boolean visible
    ) throws Exception {
        Class<?> builderClass = Class.forName("android.view.SurfaceControl$Builder");
        Class<?> transactionClass = Class.forName("android.view.SurfaceControl$Transaction");
        Class<?> rectClass = Class.forName("android.graphics.Rect");
        Class<?> surfaceClass = Class.forName("android.view.Surface");
        Class<?> surfaceControlClass = Class.forName("android.view.SurfaceControl");

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
            Object fullRect = rectClass.getDeclaredConstructor(int.class, int.class, int.class, int.class)
                    .newInstance(0, 0, width, height);
            ReflectBridge.invoke(txShow, "setWindowCrop", sc, fullRect);
            try {
                ReflectBridge.invoke(txShow, "setTrustedOverlay", sc, Boolean.TRUE);
            } catch (Throwable ignored) {
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

    Object lockFrame() throws Exception {
        if (lockHardwareCanvasMethod != null) {
            return lockHardwareCanvasMethod.invoke(surface);
        }
        return lockCanvasMethod.invoke(surface, new Object[]{null});
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
}
