package org.directscreenapi.adapter;

import java.lang.reflect.Method;

final class ReflectBridge {
    private ReflectBridge() {
    }

    private static boolean isPrimitiveAssignable(Class<?> paramType, Class<?> argType) {
        if (!paramType.isPrimitive()) return false;
        if (paramType == int.class && argType == Integer.class) return true;
        if (paramType == long.class && argType == Long.class) return true;
        if (paramType == boolean.class && argType == Boolean.class) return true;
        if (paramType == float.class && argType == Float.class) return true;
        if (paramType == double.class && argType == Double.class) return true;
        if (paramType == short.class && argType == Short.class) return true;
        if (paramType == byte.class && argType == Byte.class) return true;
        if (paramType == char.class && argType == Character.class) return true;
        return false;
    }

    static Method findMethod(Class<?> clazz, String name, Object... args) {
        for (Method m : clazz.getMethods()) {
            if (!m.getName().equals(name)) continue;
            Class<?>[] params = m.getParameterTypes();
            if (params.length != args.length) continue;
            boolean ok = true;
            for (int i = 0; i < params.length; i++) {
                Object arg = args[i];
                if (arg == null) {
                    if (params[i].isPrimitive()) {
                        ok = false;
                        break;
                    }
                    continue;
                }
                Class<?> argClass = arg.getClass();
                if (params[i].isAssignableFrom(argClass)) continue;
                if (isPrimitiveAssignable(params[i], argClass)) continue;
                ok = false;
                break;
            }
            if (ok) return m;
        }
        throw new NoSuchMethodError("method_not_found:" + clazz.getName() + "#" + name);
    }

    static Method findMethodByArity(Class<?> clazz, String name, int arity) {
        for (Method m : clazz.getMethods()) {
            if (m.getName().equals(name) && m.getParameterTypes().length == arity) {
                return m;
            }
        }
        throw new NoSuchMethodError("method_not_found:" + clazz.getName() + "#" + name + "/" + arity);
    }

    static Object invoke(Object target, String name, Object... args) throws Exception {
        Method method = findMethod(target.getClass(), name, args);
        return method.invoke(target, args);
    }

    static Object invokeStatic(Class<?> clazz, String name, Object... args) throws Exception {
        Method method = findMethod(clazz, name, args);
        return method.invoke(null, args);
    }
}
