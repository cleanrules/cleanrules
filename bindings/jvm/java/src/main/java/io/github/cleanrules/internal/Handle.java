package io.github.cleanrules.internal;

import java.lang.foreign.MemorySegment;
import java.lang.ref.Cleaner;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Consumer;

/**
 * A native handle that a Java object owns: a pointer, the C function that
 * frees it, and a count of the calls using it.
 *
 * <p>The C ABI's handles never change once made, so several threads may use
 * one at once, but never while it is freed. Every call holds the handle
 * ({@link #acquire()} and {@link #release()}), and freeing waits for the last
 * call: {@link #close()} marks the handle closed, so that no new call starts,
 * and the handle is freed at once if no call holds it, or else by the last
 * one to release it. So closing twice is harmless, closing while another
 * thread evaluates is safe, and a call after closing is an
 * {@link IllegalStateException}.
 *
 * <p>The {@link Cleaner} frees the handle of an object that becomes
 * unreachable without being closed. Its action holds this handle, never the
 * object, and honours the count like {@link #close()}.
 */
public final class Handle {
    /** The cleaner of every handle of the binding. */
    public static final Cleaner CLEANER = Cleaner.create();

    private static final long CLOSED = 1L << 62;

    private final MemorySegment pointer;
    private final Consumer<MemorySegment> free;
    private final String what;
    /** The number of calls holding the handle, plus {@link #CLOSED} once closed. */
    private final AtomicLong state = new AtomicLong();

    private Handle(MemorySegment pointer, Consumer<MemorySegment> free, String what) {
        this.pointer = pointer;
        this.free = free;
        this.what = what;
    }

    /**
     * A handle for {@code pointer}, owned by {@code owner}: freed with
     * {@code free} when {@code owner} is closed or becomes unreachable.
     */
    public static Handle own(Object owner, MemorySegment pointer, Consumer<MemorySegment> free, String what) {
        if (pointer.equals(MemorySegment.NULL)) {
            throw new IllegalStateException("cleanrules gave no " + what + " and no error; please report it");
        }
        Handle handle = new Handle(pointer, free, what);
        handle.cleanable = CLEANER.register(owner, handle::close);
        return handle;
    }

    private Cleaner.Cleanable cleanable;

    /** The pointer, for one call; {@link #release()} it when the call ends. */
    public MemorySegment acquire() {
        while (true) {
            long now = state.get();
            if ((now & CLOSED) != 0) {
                throw new IllegalStateException("this " + what + " is closed");
            }
            if (state.compareAndSet(now, now + 1)) {
                return pointer;
            }
        }
    }

    /** End a call that {@link #acquire()} began. */
    public void release() {
        if (state.decrementAndGet() == CLOSED) {
            free.accept(pointer);
        }
    }

    /** Whether the handle was closed. */
    public boolean isClosed() {
        return (state.get() & CLOSED) != 0;
    }

    /** Close the handle, freeing it now or when the last call ends; again does nothing. */
    public void close() {
        while (true) {
            long now = state.get();
            if ((now & CLOSED) != 0) {
                return;
            }
            if (state.compareAndSet(now, now | CLOSED)) {
                if (now == 0) {
                    free.accept(pointer);
                }
                return;
            }
        }
    }

    /** Close the handle from its owner: the cleaner's action, run once. */
    public void closeFromOwner() {
        cleanable.clean();
    }
}
