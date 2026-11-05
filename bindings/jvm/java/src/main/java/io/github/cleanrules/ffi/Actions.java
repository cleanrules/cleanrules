package io.github.cleanrules.ffi;

import io.github.cleanrules.internal.DataReader;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.List;
import java.util.Objects;

/**
 * Actions as upcalls. The library calls one stub, {@link #trampoline}, for
 * every action, with the action's position in the run's registry as its data
 * pointer. The run in progress on the thread is the top of a per-thread stack
 * of frames (the library calls actions on the thread that called
 * {@code cr_ruleset_run}, and an action may run a rule set again, which pushes
 * another frame).
 *
 * <p>No Java exception ever crosses the native frames: the trampoline catches
 * everything an action throws, keeps it in the frame, tells the library why
 * with {@code cr_action_call_fail} and returns 1, and the run then throws an
 * {@link io.github.cleanrules.ActionException} with it as the cause (or, for a
 * {@link VirtualMachineError}, rethrows it).
 */
final class Actions {
    private Actions() {}

    /** The stub, made the first time an action is registered: the library is loaded by then. */
    private static final class Stub {
        static final MemorySegment TRAMPOLINE = stub();

        private static MemorySegment stub() {
            try {
                return Native.upcallStub(MethodHandles.lookup()
                        .findStatic(
                                Actions.class,
                                "trampoline",
                                MethodType.methodType(int.class, MemorySegment.class, MemorySegment.class)));
            } catch (ReflectiveOperationException impossible) {
                throw new IllegalStateException(impossible);
            }
        }
    }

    /** A run in progress: its actions, its input, its opaque objects, what failed, and its decision's copy. */
    static final class Frame {
        final List<Action> actions;
        final Object data;
        final List<Object> opaques;
        Throwable failure;
        private Decision decision;

        Frame(List<Action> actions, Object data, List<Object> opaques) {
            this.actions = actions;
            this.data = data;
            this.opaques = opaques;
        }

        /** The copy of the run's decision that its action calls share, made once. */
        synchronized Decision decision(MemorySegment borrowed) {
            if (decision == null) {
                decision = Decision.copyOf(borrowed, opaques);
            }
            return decision;
        }
    }

    private static final ThreadLocal<ArrayDeque<Frame>> FRAMES = ThreadLocal.withInitial(ArrayDeque::new);

    /** A registry of {@code names}, each calling the trampoline, freed when {@code scope} closes. */
    static MemorySegment registry(List<String> names, Scope scope) {
        MemorySegment registry = Native.actionsNew();
        scope.onClose(() -> Native.actionsFree(registry));
        for (int i = 0; i < names.size(); i++) {
            String name = Objects.requireNonNull(names.get(i), "an action's name is null");
            byte[] bytes = Text.name(name);
            scope.check(Native.actionsAdd(
                    registry,
                    scope.scratch(bytes),
                    bytes.length,
                    Stub.TRAMPOLINE,
                    MemorySegment.ofAddress(i + 1L),
                    scope.error()));
        }
        return registry;
    }

    static Frame push(List<Action> actions, Object data, List<Object> opaques) {
        Frame frame = new Frame(actions, data, opaques);
        FRAMES.get().push(frame);
        return frame;
    }

    static void pop(Frame frame) {
        ArrayDeque<Frame> frames = FRAMES.get();
        frames.remove(frame);
        if (frames.isEmpty()) {
            FRAMES.remove();
        }
    }

    /** The upcall of every action: {@code int32_t (*)(void *data, cr_action_call *call)}. */
    private static int trampoline(MemorySegment data, MemorySegment call) {
        try {
            Frame frame = FRAMES.get().peek();
            if (frame == null) {
                fail(call, "no run of this binding is in progress on this thread");
                return 1;
            }
            Action action = frame.actions.get(Math.toIntExact(data.address() - 1));
            ActionCall context = new ActionCall(frame, call);
            try {
                action.run(context);
                return 0;
            } catch (Throwable thrown) {
                frame.failure = thrown;
                fail(call, DataReader.describe(thrown));
                return 1;
            } finally {
                context.end();
            }
        } catch (Throwable unexpected) {
            try {
                fail(call, "the action could not be called: " + unexpected.getClass().getName());
            } catch (Throwable ignored) {
                // Nothing may cross the native frames.
            }
            return 1;
        }
    }

    private static void fail(MemorySegment call, String message) {
        byte[] bytes = message.getBytes(StandardCharsets.UTF_8);
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment text = bytes.length == 0 ? MemorySegment.NULL : arena.allocateFrom(ValueLayout.JAVA_BYTE, bytes);
            Native.actionCallFail(call, text, bytes.length);
        }
    }
}
