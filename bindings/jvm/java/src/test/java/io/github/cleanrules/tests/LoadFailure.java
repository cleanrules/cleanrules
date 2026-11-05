package io.github.cleanrules.tests;

import io.github.cleanrules.Format;
import io.github.cleanrules.Limits;
import io.github.cleanrules.ffi.RuleSet;

/** A program that loads the library and prints what fails: for {@link LoadingTest}, in a new JVM. */
public final class LoadFailure {
    private LoadFailure() {}

    /**
     * Load the library and its default limits, twice, and print the class and message of each error.
     *
     * @param arguments none
     */
    public static void main(String[] arguments) {
        for (int i = 0; i < 2; i++) {
            try {
                RuleSet.read("IF a > 1", Format.TEXT).close();
                System.out.println("loaded");
            } catch (Throwable thrown) {
                System.out.println(thrown.getClass().getName() + ": " + thrown.getMessage());
            }
            try {
                System.out.println(Limits.defaults().maxNesting());
            } catch (Throwable thrown) {
                System.out.println(thrown.getClass().getName() + ": " + thrown.getMessage());
            }
        }
    }
}
