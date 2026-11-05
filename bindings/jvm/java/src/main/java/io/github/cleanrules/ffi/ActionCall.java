package io.github.cleanrules.ffi;

import static java.lang.foreign.ValueLayout.JAVA_INT;

import io.github.cleanrules.Truth;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Values;
import java.lang.foreign.MemorySegment;
import java.util.Map;
import java.util.concurrent.locks.ReentrantReadWriteLock;
import java.util.function.Function;

/**
 * One call of an {@link Action}: the rule whose action it is, the input the run
 * was given, the rule's truth value and evaluated payload, and the decision.
 *
 * <p>An action call is valid during the call only, also from other threads:
 * afterwards every method throws {@link IllegalStateException}. A method that
 * another thread runs while the call ends finishes first, so nothing ever
 * reads the call's memory after the library freed it.
 */
public final class ActionCall {
    private final Actions.Frame frame;
    private final MemorySegment call;
    private final ReentrantReadWriteLock lock = new ReentrantReadWriteLock();
    private boolean active = true;

    ActionCall(Actions.Frame frame, MemorySegment call) {
        this.frame = frame;
        this.call = call;
    }

    /** End the call: wait for the methods running on other threads, then refuse new ones. */
    void end() {
        lock.writeLock().lock();
        try {
            active = false;
        } finally {
            lock.writeLock().unlock();
        }
    }

    /** {@code read} of the call's decision and rule, under the read lock, while the call lasts. */
    private <T> T with(Function<Integer, T> read) {
        lock.readLock().lock();
        try {
            if (!active) {
                throw new IllegalStateException("an ActionCall is valid only during its action");
            }
            return read.apply(Math.toIntExact(Native.actionCallRule(call)));
        } finally {
            lock.readLock().unlock();
        }
    }

    private MemorySegment decisionPointer() {
        return Native.actionCallDecision(call);
    }

    /**
     * The position of the rule in its rule set, from 0.
     *
     * @return the position
     */
    public int position() {
        return with(rule -> rule);
    }

    /**
     * The name of the rule.
     *
     * @return the name
     */
    public String ruleName() {
        return with(rule -> {
            try (Scope scope = new Scope()) {
                MemorySegment name = scope.pointer();
                MemorySegment len = scope.size();
                scope.check(Native.decisionRuleName(decisionPointer(), rule, name, len, scope.error()));
                return Scope.borrowedText(name, len);
            }
        });
    }

    /**
     * The truth value of the rule's condition.
     *
     * @return the truth value
     */
    public Truth truth() {
        return with(rule -> {
            try (Scope scope = new Scope()) {
                MemorySegment out = scope.int32();
                scope.check(Native.decisionTruth(decisionPointer(), rule, out, scope.error()));
                return Native.truth(out.get(JAVA_INT, 0));
            }
        });
    }

    /**
     * The rule's evaluated payload: its output, empty for a rule without one.
     *
     * @return the payload
     */
    @SuppressWarnings("unchecked") // A selected rule's payload is always a map.
    public Map<String, Object> output() {
        return with(rule -> {
            Object output = Values.take(
                    Scope.value((out, error) -> Native.decisionRuleOutput(decisionPointer(), rule, out, error)),
                    frame.opaques);
            return output == null ? Map.of() : (Map<String, Object>) output;
        });
    }

    /**
     * The input the run was given: the same object.
     *
     * @return the input
     */
    public Object data() {
        return with(rule -> frame.data);
    }

    /**
     * The decision the actions run for: a copy that the caller owns and may
     * keep after the call, made once per run and shared by the calls of all
     * its actions. It is not the object that {@link RuleSet#run} returns, and
     * closing it closes it for every action of the run.
     *
     * @return the copy of the decision
     */
    public Decision decision() {
        return with(rule -> frame.decision(decisionPointer()));
    }
}
