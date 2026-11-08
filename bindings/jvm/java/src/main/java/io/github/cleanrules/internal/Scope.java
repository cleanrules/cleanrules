package io.github.cleanrules.internal;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_BOOLEAN;
import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import io.github.cleanrules.Limits;
import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.ArrayDeque;

/**
 * The native memory of one call: a confined arena for the texts passed in and
 * the out-parameters, and the call's {@code cr_error *} slot. Everything is
 * freed when the scope closes; texts and values that the library gives are
 * copied into Java and freed before that.
 */
public final class Scope implements AutoCloseable {
    private final Arena arena = Arena.ofConfined();
    private final MemorySegment error = arena.allocate(ADDRESS);
    private final ArrayDeque<Runnable> cleanups = new ArrayDeque<>();

    /** What the call knows about the causes of its errors, if anything. */
    public Errors.Causes causes;

    /** Run {@code cleanup} when the scope closes, before what was added earlier. */
    public void onClose(Runnable cleanup) {
        cleanups.push(cleanup);
    }

    /** The error slot, set to NULL, for the last argument of a fallible call. */
    public MemorySegment error() {
        error.set(ADDRESS, 0, MemorySegment.NULL);
        return error;
    }

    /** A pointer out-parameter, set to NULL. */
    public MemorySegment pointer() {
        MemorySegment slot = arena.allocate(ADDRESS);
        slot.set(ADDRESS, 0, MemorySegment.NULL);
        return slot;
    }

    /** A {@code size_t} out-parameter, set to 0. */
    public MemorySegment size() {
        MemorySegment slot = arena.allocate(JAVA_LONG);
        slot.set(JAVA_LONG, 0, 0);
        return slot;
    }

    /** An {@code int32_t} out-parameter, set to 0. */
    public MemorySegment int32() {
        MemorySegment slot = arena.allocate(JAVA_INT);
        slot.set(JAVA_INT, 0, 0);
        return slot;
    }

    /** A {@code bool} out-parameter, set to false. */
    public MemorySegment bool() {
        MemorySegment slot = arena.allocate(JAVA_BOOLEAN);
        slot.set(JAVA_BOOLEAN, 0, false);
        return slot;
    }

    /** Bytes passed in, borrowed for the call: NULL for none. */
    public MemorySegment bytes(byte[] bytes) {
        return bytes.length == 0 ? MemorySegment.NULL : arena.allocateFrom(JAVA_BYTE, bytes);
    }

    /** An array of pointers passed in, borrowed for the call: NULL for none. */
    public MemorySegment pointers(MemorySegment... pointers) {
        if (pointers.length == 0) {
            return MemorySegment.NULL;
        }
        MemorySegment array = arena.allocate(ADDRESS, pointers.length);
        for (int i = 0; i < pointers.length; i++) {
            array.setAtIndex(ADDRESS, i, pointers[i]);
        }
        return array;
    }

    /** An array of {@code bool}s passed in, borrowed for the call: NULL for none. */
    public MemorySegment bools(boolean... flags) {
        if (flags.length == 0) {
            return MemorySegment.NULL;
        }
        MemorySegment array = arena.allocate(JAVA_BOOLEAN, flags.length);
        for (int i = 0; i < flags.length; i++) {
            array.setAtIndex(JAVA_BOOLEAN, i, flags[i]);
        }
        return array;
    }

    private MemorySegment scratch = MemorySegment.NULL;

    /**
     * Bytes passed in to a call that copies them at once (a key, a string
     * value): a buffer reused from call to call; NULL for none.
     */
    public MemorySegment scratch(byte[] bytes) {
        if (bytes.length == 0) {
            return MemorySegment.NULL;
        }
        if (scratch.byteSize() < bytes.length) {
            scratch = arena.allocate(Math.max(bytes.length, 2 * scratch.byteSize()));
        }
        MemorySegment.copy(bytes, 0, scratch, JAVA_BYTE, 0, bytes.length);
        return scratch;
    }

    /** The error of a call that failed with {@code status}, which this frees: to discard it. */
    public RuntimeException failure(int status) {
        return Errors.of(status, error.get(ADDRESS, 0), causes);
    }

    /**
     * Native limits for one call, freed when the scope closes; NULL for the
     * defaults or for null.
     */
    public MemorySegment limits(Limits limits) {
        if (limits == null || limits.equals(Limits.defaults())) {
            return MemorySegment.NULL;
        }
        MemorySegment native_ = Native.limitsNew();
        if (native_.equals(MemorySegment.NULL)) {
            throw new OutOfMemoryError("cannot allocate cleanrules limits");
        }
        onClose(() -> Native.limitsFree(native_));
        long[] values = {
            limits.maxSize(),
            limits.maxNesting(),
            limits.maxDepth(),
            limits.maxTableSize(),
            limits.maxYamlSize(),
            limits.maxTextSize(),
            limits.maxValues()
        };
        // The size first: it sets the YAML and text limits too.
        for (int i = 0; i < values.length; i++) {
            check(Native.limitsSet(native_, i + 1, values[i], error()));
        }
        return native_;
    }

    /** Throw the error of a call that returned {@code status}, if it failed. */
    public void check(int status) {
        check(status, null);
    }

    /** Throw the error of a call that returned {@code status}, with the causes that apply. */
    public void check(int status, Errors.Causes causes) {
        if (status != Native.CR_OK) {
            throw Errors.of(status, error.get(ADDRESS, 0), causes);
        }
    }

    /** A call that gives an owned text through {@code out} and {@code len}. */
    @FunctionalInterface
    public interface TextCall {
        /** The call. */
        int call(MemorySegment out, MemorySegment len, MemorySegment error);
    }

    /** The text that {@code call} gives, copied and freed. */
    public static String text(TextCall call) {
        try (Scope scope = new Scope()) {
            MemorySegment out = scope.pointer();
            MemorySegment len = scope.size();
            scope.check(call.call(out, len, scope.error()));
            return takeText(out, len);
        }
    }

    /** A call that gives an owned value through {@code out}. */
    @FunctionalInterface
    public interface ValueCall {
        /** The call. */
        int call(MemorySegment out, MemorySegment error);
    }

    /** The owned value that {@code call} gives (never NULL): free it. */
    public static MemorySegment value(ValueCall call) {
        try (Scope scope = new Scope()) {
            MemorySegment out = scope.pointer();
            scope.check(call.call(out, scope.error()));
            return out.get(ADDRESS, 0);
        }
    }

    /** The owned text a call gave through {@code out} and {@code len}, copied and freed. */
    public static String takeText(MemorySegment out, MemorySegment len) {
        MemorySegment text = out.get(ADDRESS, 0);
        if (text.equals(MemorySegment.NULL)) {
            return null;
        }
        try {
            return Native.borrowed(text, len.get(JAVA_LONG, 0));
        } finally {
            Native.stringFree(text);
        }
    }

    /** The borrowed text a call gave through {@code text} and {@code len}, copied. */
    public static String borrowedText(MemorySegment text, MemorySegment len) {
        return Native.borrowed(text.get(ADDRESS, 0), len.get(JAVA_LONG, 0));
    }

    @Override
    public void close() {
        try {
            while (!cleanups.isEmpty()) {
                cleanups.pop().run();
            }
        } finally {
            arena.close();
        }
    }
}
