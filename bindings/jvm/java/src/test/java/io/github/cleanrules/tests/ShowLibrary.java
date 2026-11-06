package io.github.cleanrules.tests;

import io.github.cleanrules.CleanRules;
import io.github.cleanrules.Format;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.RuleSet;
import java.nio.file.Files;
import java.nio.file.Path;

/** A program that loads the library and prints where from, or what fails: for {@link BundledLoadingTest}, in a new JVM. */
public final class ShowLibrary {
    private ShowLibrary() {}

    /**
     * Load the library, twice, and print where it came from and whether that file is still there, or the class
     * and message of the error.
     *
     * @param arguments none
     */
    public static void main(String[] arguments) {
        for (int i = 0; i < 2; i++) {
            try (RuleSet rules = RuleSet.read("IF a > 1 THEN b = 2", Format.TEXT);
                    Decision decision = rules.evaluateJson("{\"a\": 5}")) {
                Path path = CleanRules.libraryPath().orElse(null);
                System.out.println("loaded " + decision.outputJson() + " from " + path);
                System.out.println(path == null ? "no file" : Files.exists(path) ? "file kept" : "file deleted");
            } catch (Throwable thrown) {
                System.out.println(thrown.getClass().getName() + ": " + thrown.getMessage());
            }
        }
    }
}
