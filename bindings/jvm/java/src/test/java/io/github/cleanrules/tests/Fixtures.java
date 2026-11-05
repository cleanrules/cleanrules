package io.github.cleanrules.tests;

import io.github.cleanrules.Format;
import io.github.cleanrules.ffi.RuleSet;

/** Rule sets shared by the tests. */
final class Fixtures {
    private Fixtures() {}

    /** The rules of docs/examples/discounts.rules. */
    static final String DISCOUNTS = """
            HIT POLICY priority

            RULE blocked PRIORITY 99 DESCRIPTION "blocked customers never get a discount"
            IF customer.blocked
            THEN discount = 0

            RULE vip PRIORITY 10 TAGS loyalty
            IF customer.years > 5 AND order.total > 1000
            THEN discount = 0.15
            DO notify_sales

            RULE loyal PRIORITY 5 TAGS loyalty
            IF customer.years > 2 OR customer.newsletter
            THEN discount = 0.05
            """;

    /** A collect rule set whose payloads give back what they read. */
    static final String ECHO = """
            HIT POLICY collect

            RULE echo
            IF true
            THEN value = value
            """;

    static RuleSet discounts() {
        return RuleSet.read(DISCOUNTS, Format.TEXT);
    }

    static RuleSet echo() {
        return RuleSet.read(ECHO, Format.TEXT);
    }
}
