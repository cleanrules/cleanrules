package io.github.cleanrules.tests;

import io.github.cleanrules.CleanRules;
import java.io.IOException;

/** A program that loads the library, prints where from, and keeps it loaded until its input ends: for {@link BundledLoadingTest}. */
public final class HoldLibrary {
    private HoldLibrary() {}

    /**
     * Load the library, print its path, then wait for the end of standard input.
     *
     * @param arguments none
     * @throws IOException if standard input cannot be read
     */
    public static void main(String[] arguments) throws IOException {
        CleanRules.load();
        System.out.println(CleanRules.libraryPath().orElseThrow());
        System.out.flush();
        System.in.transferTo(java.io.OutputStream.nullOutputStream());
    }
}
