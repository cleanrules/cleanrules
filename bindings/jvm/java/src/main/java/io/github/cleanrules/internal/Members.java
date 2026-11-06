package io.github.cleanrules.internal;

import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.lang.reflect.RecordComponent;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/**
 * What the reader may read of an object ({@code spec/data-model.md},
 * <i>Safety</i> and <i>Java and Scala</i>): a record's components, in their
 * order, or an object's public instance fields and public JavaBean accessors
 * ({@code getAge()} and {@code isActive()} read as {@code age} and
 * {@code active}), sorted by name, by Unicode code point.
 *
 * <p>Never a non-public member, a static one, a method with parameters or one
 * that returns nothing, and never a member that a class of the Java platform
 * (or of the Scala library) declares ({@code getClass()},
 * {@code Enum.getDeclaringClass()}...): objects of the platform are opaque,
 * and so nothing reaches a {@link Class}, a {@link ClassLoader} or a
 * {@link Thread} through them. An accessor wins over a field of the same
 * name, and {@code getX} over {@code isX}.
 */
public final class Members {
    /** One readable member. */
    public static final class Member {
        private final Method method;
        private final Field field;
        private final String problem;

        private Member(Method method, Field field, String problem) {
            this.method = method;
            this.field = field;
            this.problem = problem;
        }

        /** The member's value on {@code target}; what a getter throws is thrown. */
        public Object get(Object target) throws Throwable {
            if (problem != null) {
                throw new IllegalAccessException(problem);
            }
            try {
                return method != null ? method.invoke(target) : field.get(target);
            } catch (InvocationTargetException thrown) {
                throw thrown.getCause();
            }
        }
    }

    private static final ClassValue<Map<String, Member>> MEMBERS = new ClassValue<>() {
        @Override
        protected Map<String, Member> computeValue(Class<?> type) {
            return type.isRecord() ? components(type) : bean(type);
        }
    };

    private Members() {}

    /** The members of {@code type}, in the order a whole conversion writes them. */
    public static Map<String, Member> of(Class<?> type) {
        return MEMBERS.get(type);
    }

    /**
     * Whether {@code type} belongs to the platform: a module {@code java.*} or
     * {@code jdk.*}, or the Scala library (a package {@code scala} or
     * {@code scala.*}, whose values the Scala layer converts before the reader
     * sees them).
     */
    public static boolean platform(Class<?> type) {
        String pkg = type.getPackageName();
        if (pkg.equals("scala") || pkg.startsWith("scala.")) {
            return true;
        }
        Module module = type.getModule();
        if (!module.isNamed()) {
            return false;
        }
        String name = module.getName();
        return name.startsWith("java.") || name.startsWith("jdk.");
    }

    private static Map<String, Member> components(Class<?> type) {
        Map<String, Member> members = new LinkedHashMap<>();
        for (RecordComponent component : type.getRecordComponents()) {
            members.put(component.getName(), accessor(type, component.getAccessor()));
        }
        return Collections.unmodifiableMap(members);
    }

    private static Map<String, Member> bean(Class<?> type) {
        Map<String, Member> members = new TreeMap<>(Members::compareCodePoints);
        for (Field field : type.getFields()) {
            if (!Modifier.isStatic(field.getModifiers()) && !platform(field.getDeclaringClass())) {
                members.put(field.getName(), field(field));
            }
        }
        // Accessors replace fields; `get` replaces `is`.
        Map<String, Method> is = new TreeMap<>();
        Map<String, Method> get = new TreeMap<>();
        for (Method method : type.getMethods()) {
            if (Modifier.isStatic(method.getModifiers())
                    || method.getParameterCount() != 0
                    || method.getReturnType() == void.class
                    || method.isBridge()
                    || method.isSynthetic()
                    || platform(method.getDeclaringClass())) {
                continue;
            }
            String name = method.getName();
            if (name.startsWith("get") && name.length() > 3) {
                get.put(property(name.substring(3)), method);
            } else if (name.startsWith("is")
                    && name.length() > 2
                    && (method.getReturnType() == boolean.class || method.getReturnType() == Boolean.class)) {
                is.put(property(name.substring(2)), method);
            }
        }
        is.forEach((name, method) -> members.put(name, accessor(type, method)));
        get.forEach((name, method) -> members.put(name, accessor(type, method)));
        return Collections.unmodifiableMap(new LinkedHashMap<>(members));
    }

    /** A JavaBeans property name: {@code Age} is {@code age}, {@code URL} stays {@code URL}. */
    private static String property(String suffix) {
        if (suffix.length() > 1 && Character.isUpperCase(suffix.charAt(0)) && Character.isUpperCase(suffix.charAt(1))) {
            return suffix;
        }
        return Character.toLowerCase(suffix.charAt(0)) + suffix.substring(1);
    }

    /**
     * A public method, made callable: a public method of a class that is not
     * public (a private nested record, a class of another package) needs
     * {@code setAccessible}, which works where the package is open to this
     * module (always on the class path); otherwise a public declaration in a
     * public supertype is used. Only public members are ever made callable.
     */
    private static Member accessor(Class<?> type, Method method) {
        if (method.trySetAccessible()) {
            return new Member(method, null, null);
        }
        for (Method declared : publicDeclarations(type, method)) {
            if (declared.trySetAccessible()) {
                return new Member(declared, null, null);
            }
        }
        return new Member(null, null, inaccessible(method.getDeclaringClass(), method.getName() + "()"));
    }

    private static Member field(Field field) {
        if (field.trySetAccessible()) {
            return new Member(null, field, null);
        }
        return new Member(null, null, inaccessible(field.getDeclaringClass(), field.getName()));
    }

    private static String inaccessible(Class<?> type, String member) {
        return "cannot read " + type.getName() + "." + member + ": its package "
                + type.getPackageName() + " is not open to io.github.cleanrules";
    }

    /** The declarations of {@code method} in the public supertypes of {@code type}. */
    private static List<Method> publicDeclarations(Class<?> type, Method method) {
        List<Method> found = new ArrayList<>();
        List<Class<?>> pending = new ArrayList<>(List.of(type));
        while (!pending.isEmpty()) {
            Class<?> next = pending.removeLast();
            if (Modifier.isPublic(next.getModifiers()) && !platform(next)) {
                try {
                    found.add(next.getMethod(method.getName()));
                } catch (NoSuchMethodException absent) {
                    // Not declared there.
                }
            }
            if (next.getSuperclass() != null) {
                pending.add(next.getSuperclass());
            }
            pending.addAll(List.of(next.getInterfaces()));
        }
        return found;
    }

    /** Compare by Unicode code point, not by UTF-16 unit as {@link String#compareTo} does. */
    public static int compareCodePoints(String left, String right) {
        int i = 0;
        int j = 0;
        while (i < left.length() && j < right.length()) {
            int a = left.codePointAt(i);
            int b = right.codePointAt(j);
            if (a != b) {
                return Integer.compare(a, b);
            }
            i += Character.charCount(a);
            j += Character.charCount(b);
        }
        return Integer.compare(left.length() - i, right.length() - j);
    }
}
