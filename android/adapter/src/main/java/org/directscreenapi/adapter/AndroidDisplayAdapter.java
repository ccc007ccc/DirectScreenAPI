package org.directscreenapi.adapter;

import java.lang.reflect.Field;
import java.lang.reflect.Method;

public final class AndroidDisplayAdapter implements DisplayAdapter {
    private static Method findMethodByName(Class<?> clazz, String name, int arity) {
        for (Method m : clazz.getMethods()) {
            if (m.getName().equals(name) && m.getParameterTypes().length == arity) {
                return m;
            }
        }
        return null;
    }

    private static int readIntField(Object obj, String... names) {
        for (String n : names) {
            try {
                Field f = obj.getClass().getField(n);
                Object v = f.get(obj);
                if (v instanceof Integer) return ((Integer) v).intValue();
            } catch (Throwable ignored) {
            }
            try {
                Field f = obj.getClass().getDeclaredField(n);
                f.setAccessible(true);
                Object v = f.get(obj);
                if (v instanceof Integer) return ((Integer) v).intValue();
            } catch (Throwable ignored) {
            }
        }
        return 0;
    }

    private static float readFloatField(Object obj, String... names) {
        for (String n : names) {
            try {
                Field f = obj.getClass().getField(n);
                Object v = f.get(obj);
                if (v instanceof Float) return ((Float) v).floatValue();
                if (v instanceof Double) return ((Double) v).floatValue();
            } catch (Throwable ignored) {
            }
            try {
                Field f = obj.getClass().getDeclaredField(n);
                f.setAccessible(true);
                Object v = f.get(obj);
                if (v instanceof Float) return ((Float) v).floatValue();
                if (v instanceof Double) return ((Double) v).floatValue();
            } catch (Throwable ignored) {
            }
        }
        return 0f;
    }

    private static Object readObjectField(Object obj, String... names) {
        for (String n : names) {
            try {
                Field f = obj.getClass().getField(n);
                return f.get(obj);
            } catch (Throwable ignored) {
            }
            try {
                Field f = obj.getClass().getDeclaredField(n);
                f.setAccessible(true);
                return f.get(obj);
            } catch (Throwable ignored) {
            }
        }
        return null;
    }

    private static int normalizeRotation(int rotation) {
        int r = rotation % 4;
        if (r < 0) r += 4;
        return r;
    }

    private static Object invokeSingleDisplayIdMethod(Object target, String methodName, int displayId) {
        for (Method method : target.getClass().getMethods()) {
            if (!methodName.equals(method.getName())) continue;
            Class<?>[] params = method.getParameterTypes();
            if (params.length != 1) continue;
            try {
                Class<?> paramType = params[0];
                if (paramType == long.class || paramType == Long.class) {
                    return method.invoke(target, Long.valueOf(displayId));
                }
                if (paramType == int.class || paramType == Integer.class) {
                    return method.invoke(target, Integer.valueOf(displayId));
                }
            } catch (Throwable ignored) {
            }
        }
        return null;
    }

    private static int invokeIntFromPoint(Object displayObj, String methodName, boolean xAxis) {
        try {
            Class<?> pointClass = Class.forName("android.graphics.Point");
            Object pt = pointClass.getDeclaredConstructor().newInstance();
            Method m = findMethodByName(displayObj.getClass(), methodName, 1);
            if (m == null) return 0;
            m.invoke(displayObj, pt);
            Field f = pointClass.getField(xAxis ? "x" : "y");
            return ((Integer) f.get(pt)).intValue();
        } catch (Throwable ignored) {
        }
        return 0;
    }

    private static int invokeIntFromDisplayMetrics(Object displayObj, boolean density) {
        try {
            Class<?> dmClass = Class.forName("android.util.DisplayMetrics");
            Object dm = dmClass.getDeclaredConstructor().newInstance();
            Method m = findMethodByName(displayObj.getClass(), "getRealMetrics", 1);
            if (m == null) {
                m = findMethodByName(displayObj.getClass(), "getMetrics", 1);
            }
            if (m == null) return 0;
            m.invoke(displayObj, dm);
            Field f = dmClass.getField(density ? "densityDpi" : "noncompatDensityDpi");
            return ((Integer) f.get(dm)).intValue();
        } catch (Throwable ignored) {
        }
        return 0;
    }

    private static float invokeFloatNoArg(Object obj, String name, float fallback) {
        try {
            Method m = findMethodByName(obj.getClass(), name, 0);
            if (m == null) return fallback;
            Object v = m.invoke(obj);
            if (v instanceof Float) return ((Float) v).floatValue();
            if (v instanceof Double) return ((Double) v).floatValue();
        } catch (Throwable ignored) {
        }
        return fallback;
    }

    private static float maxRefreshFromModesObject(
            Object modesObj,
            int targetWidth,
            int targetHeight,
            float fallback
    ) {
        if (modesObj == null) {
            return fallback;
        }

        float maxMatched = 0f;
        float maxAll = 0f;
        Object[] modesArray = null;
        if (modesObj instanceof Object[]) {
            modesArray = (Object[]) modesObj;
        } else if (modesObj instanceof Iterable) {
            java.util.ArrayList<Object> list = new java.util.ArrayList<Object>();
            for (Object it : (Iterable<?>) modesObj) {
                list.add(it);
            }
            modesArray = list.toArray(new Object[0]);
        }
        if (modesArray == null || modesArray.length == 0) {
            return fallback;
        }

        for (Object mode : modesArray) {
            if (mode == null) {
                continue;
            }
            int mw = invokeIntNoArg(mode, "getPhysicalWidth", 0);
            if (mw <= 0) {
                mw = invokeIntNoArg(mode, "getWidth", 0);
            }
            if (mw <= 0) {
                mw = readIntField(mode, "physicalWidth", "width", "mWidth");
            }

            int mh = invokeIntNoArg(mode, "getPhysicalHeight", 0);
            if (mh <= 0) {
                mh = invokeIntNoArg(mode, "getHeight", 0);
            }
            if (mh <= 0) {
                mh = readIntField(mode, "physicalHeight", "height", "mHeight");
            }

            float hz = invokeFloatNoArg(mode, "getRefreshRate", 0f);
            if (hz <= 0f) {
                hz = readFloatField(mode, "refreshRate", "fps", "mRefreshRate");
            }
            if (hz <= 0f) {
                continue;
            }
            if (hz > maxAll) {
                maxAll = hz;
            }
            if (targetWidth > 0 && targetHeight > 0 && mw == targetWidth && mh == targetHeight && hz > maxMatched) {
                maxMatched = hz;
            }
        }

        if (maxMatched > 0f) {
            return maxMatched;
        }
        if (maxAll > 0f) {
            return maxAll;
        }
        return fallback;
    }

    private static float readMaxRefreshFromDisplay(Object displayObj, int width, int height, float fallback) {
        try {
            Method m = findMethodByName(displayObj.getClass(), "getSupportedModes", 0);
            if (m == null) {
                return fallback;
            }
            Object modesObj = m.invoke(displayObj);
            return maxRefreshFromModesObject(modesObj, width, height, fallback);
        } catch (Throwable ignored) {
        }
        return fallback;
    }

    private static float readMaxRefreshFromDisplayInfo(Object info, int width, int height, float fallback) {
        if (info == null) {
            return fallback;
        }
        Object supportedModes = readObjectField(info, "supportedModes", "appsSupportedModes", "mSupportedModes");
        return maxRefreshFromModesObject(supportedModes, width, height, fallback);
    }

    private static int invokeIntNoArg(Object obj, String name, int fallback) {
        try {
            Method m = findMethodByName(obj.getClass(), name, 0);
            if (m == null) return fallback;
            Object v = m.invoke(obj);
            if (v instanceof Integer) return ((Integer) v).intValue();
        } catch (Throwable ignored) {
        }
        return fallback;
    }

    private static int readSystemDensityDpi(int fallback) {
        try {
            Class<?> resourcesClass = Class.forName("android.content.res.Resources");
            Method getSystem = findMethodByName(resourcesClass, "getSystem", 0);
            Object resources = getSystem != null ? getSystem.invoke(null) : null;
            if (resources != null) {
                Method getDisplayMetrics = findMethodByName(resourcesClass, "getDisplayMetrics", 0);
                if (getDisplayMetrics != null) {
                    Object dm = getDisplayMetrics.invoke(resources);
                    int density = readIntField(dm, "densityDpi", "noncompatDensityDpi");
                    if (density > 0) return density;
                }
            }
        } catch (Throwable ignored) {
        }
        return fallback;
    }

    @Override
    public DisplaySnapshot queryDisplaySnapshot() {
        int width = 1080;
        int height = 2400;
        float refreshHz = 60f;
        float maxRefreshHz = 60f;
        int densityDpi = 420;
        int rotation = 0;

        try {
            Class<?> dmgClass = Class.forName("android.hardware.display.DisplayManagerGlobal");
            Method getInstance = findMethodByName(dmgClass, "getInstance", 0);
            Object dmg = getInstance != null ? getInstance.invoke(null) : null;

            if (dmg != null) {
                Object info = invokeSingleDisplayIdMethod(dmg, "getDisplayInfo", 0);
                if (info != null) {
                    int w = readIntField(info, "logicalWidth", "appWidth", "width");
                    int h = readIntField(info, "logicalHeight", "appHeight", "height");
                    int rot = readIntField(info, "rotation", "logicalRotation", "orientation");
                    int density = readIntField(info, "logicalDensityDpi", "densityDpi");
                    float hz = readFloatField(info, "refreshRate", "renderFrameRate");

                    if (w > 0 && h > 0) {
                        width = w;
                        height = h;
                    }
                    if (hz > 0f) {
                        refreshHz = hz;
                        maxRefreshHz = hz;
                    }
                    if (density > 0) {
                        densityDpi = density;
                    }
                    rotation = rot;
                    maxRefreshHz = readMaxRefreshFromDisplayInfo(info, width, height, maxRefreshHz);
                }

                Object display = invokeSingleDisplayIdMethod(dmg, "getRealDisplay", 0);
                if (display != null) {
                    int rw = invokeIntFromPoint(display, "getRealSize", true);
                    int rh = invokeIntFromPoint(display, "getRealSize", false);
                    if (rw > 0 && rh > 0) {
                        width = rw;
                        height = rh;
                    }
                    float hz = invokeFloatNoArg(display, "getRefreshRate", refreshHz);
                    if (hz > 0f) {
                        refreshHz = hz;
                    }
                    maxRefreshHz = readMaxRefreshFromDisplay(display, width, height, Math.max(maxRefreshHz, refreshHz));
                    int rot = invokeIntNoArg(display, "getRotation", rotation);
                    rotation = rot;

                    int density = invokeIntFromDisplayMetrics(display, true);
                    if (density > 0) {
                        densityDpi = density;
                    }
                }
            }
        } catch (Throwable ignored) {
        }

        if (densityDpi <= 0) {
            densityDpi = readSystemDensityDpi(420);
        }
        if (width <= 0) width = 1080;
        if (height <= 0) height = 2400;
        if (refreshHz <= 0f) refreshHz = 60f;
        if (maxRefreshHz <= 0f) maxRefreshHz = refreshHz;
        if (maxRefreshHz < refreshHz) maxRefreshHz = refreshHz;

        return new DisplaySnapshot(
                width,
                height,
                refreshHz,
                maxRefreshHz,
                densityDpi,
                normalizeRotation(rotation)
        );
    }
}
