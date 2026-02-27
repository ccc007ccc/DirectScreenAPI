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

    private static int normalizeRotation(int rotation) {
        int r = rotation % 4;
        if (r < 0) r += 4;
        return r;
    }

    private static Object getDisplayInfo(Object dmg, int displayId) {
        Method m = null;
        for (Method x : dmg.getClass().getMethods()) {
            if (!"getDisplayInfo".equals(x.getName())) continue;
            if (x.getParameterTypes().length != 1) continue;
            m = x;
            break;
        }
        if (m == null) return null;
        try {
            Class<?> t = m.getParameterTypes()[0];
            if (t == int.class || t == Integer.class) {
                return m.invoke(dmg, Integer.valueOf(displayId));
            }
            if (t == long.class || t == Long.class) {
                return m.invoke(dmg, Long.valueOf(displayId));
            }
        } catch (Throwable ignored) {
        }
        return null;
    }

    private static Object getRealDisplay(Object dmg, int displayId) {
        Method m = null;
        for (Method x : dmg.getClass().getMethods()) {
            if (!"getRealDisplay".equals(x.getName())) continue;
            if (x.getParameterTypes().length != 1) continue;
            m = x;
            break;
        }
        if (m == null) return null;
        try {
            Class<?> t = m.getParameterTypes()[0];
            if (t == int.class || t == Integer.class) {
                return m.invoke(dmg, Integer.valueOf(displayId));
            }
            if (t == long.class || t == Long.class) {
                return m.invoke(dmg, Long.valueOf(displayId));
            }
        } catch (Throwable ignored) {
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
        int densityDpi = 420;
        int rotation = 0;

        try {
            Class<?> dmgClass = Class.forName("android.hardware.display.DisplayManagerGlobal");
            Method getInstance = findMethodByName(dmgClass, "getInstance", 0);
            Object dmg = getInstance != null ? getInstance.invoke(null) : null;

            if (dmg != null) {
                Object info = getDisplayInfo(dmg, 0);
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
                    }
                    if (density > 0) {
                        densityDpi = density;
                    }
                    rotation = rot;
                }

                Object display = getRealDisplay(dmg, 0);
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

        return new DisplaySnapshot(width, height, refreshHz, densityDpi, normalizeRotation(rotation));
    }
}
