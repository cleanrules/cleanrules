package io.github.cleanrules.internal;

import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.RuleSet;
import java.lang.foreign.MemorySegment;
import java.util.List;

/**
 * What the idiomatic API needs of the native layer's objects and that their
 * public API does not give: a rule set made from a handle, and the handles and
 * opaque objects of rule sets and decisions. The native layer registers it
 * when {@link RuleSet} is initialised.
 */
public final class Bridge {
    private Bridge() {}

    /** The native layer's side. */
    public interface Ffi {
        /** A rule set that owns {@code owned}, a {@code cr_ruleset *}. */
        RuleSet ruleSet(MemorySegment owned);

        /** The handle of a rule set. */
        Handle handle(RuleSet ruleset);

        /** The handle of a decision. */
        Handle handle(Decision decision);

        /** The objects of a decision's opaque values, by token. */
        List<Object> opaques(Decision decision);
    }

    private static volatile Ffi ffi;

    /** Register the native layer's side: {@link RuleSet} does, once. */
    public static void register(Ffi access) {
        ffi = access;
    }

    /** The native layer's side. */
    public static Ffi ffi() {
        Ffi access = ffi;
        if (access == null) {
            try {
                Class.forName(RuleSet.class.getName(), true, Bridge.class.getClassLoader());
            } catch (ClassNotFoundException missing) {
                throw new IllegalStateException("the native layer is missing", missing);
            }
            access = ffi;
        }
        return access;
    }
}
