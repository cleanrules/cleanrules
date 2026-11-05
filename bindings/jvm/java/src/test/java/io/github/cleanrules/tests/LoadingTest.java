package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.CleanRules;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.RuleSet;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class LoadingTest {
    @Test
    void theLibraryLoadsFromThePropertyAndMatchesTheBinding() {
        CleanRules.load();
        assertEquals(1, CleanRules.ABI_VERSION);
        assertEquals(1, CleanRules.FORMAT_VERSION);
        assertTrue(CleanRules.version().matches("\\d+\\.\\d+\\.\\d+.*"), CleanRules.version());
        var path = CleanRules.libraryPath().orElseThrow();
        assertTrue(Files.isRegularFile(path), path.toString());
        assertEquals(System.mapLibraryName("cleanrules_ffi"), path.getFileName().toString());
    }

    @Test
    void aSmokeEvaluation() {
        try (RuleSet rules = Fixtures.discounts();
                Decision decision = rules.evaluateJson(
                        "{\"customer\": {\"years\": 7, \"newsletter\": false}, \"order\": {\"total\": 1500}}")) {
            assertEquals(3, rules.size());
            assertEquals(java.util.List.of(1), decision.selected());
            assertFalse(decision.firedAt(0));
            assertEquals("{\"discount\":{\"$decimal\":\"0.15\"}}", decision.outputJson());
        }
    }

    /** What {@link LoadFailure} prints in a new JVM with {@code property} as the library's path. */
    private static List<String> load(String property) throws IOException, InterruptedException {
        return load(property, "--enable-native-access=ALL-UNNAMED");
    }

    private static List<String> load(String property, String access) throws IOException, InterruptedException {
        String java = Path.of(System.getProperty("java.home"), "bin", "java").toString();
        // The JVM writes its own warnings to stderr (on Linux, about the stack
        // guard of a file it failed to load), so only stdout is compared.
        Path errors = Files.createTempFile("cleanrules-load", ".err");
        try {
            Process process = new ProcessBuilder(
                            java,
                            access,
                            "-Dcleanrules.library.path=" + property,
                            "-cp",
                            System.getProperty("java.class.path"),
                            LoadFailure.class.getName())
                    .redirectError(errors.toFile())
                    .start();
            String output = new String(process.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
            assertEquals(0, process.waitFor(), output + Files.readString(errors));
            return output.lines().toList();
        } finally {
            Files.deleteIfExists(errors);
        }
    }

    @Test
    void aMissingLibraryIsAClearErrorEveryTime(@TempDir Path directory) throws Exception {
        Path missing = directory.resolve("nothing").resolve(System.mapLibraryName("cleanrules_ffi"));
        String expected = "java.lang.UnsatisfiedLinkError: no cleanrules library at " + missing
                + " (from the system property cleanrules.library.path); build it with `cargo build -p"
                + " cleanrules-ffi` or fix the path";
        // The same error every time, also for the default limits.
        assertEquals(List.of(expected, expected, expected, expected), load(missing.toString()));
    }

    @Test
    void aFileThatIsNotTheLibraryIsAClearError(@TempDir Path directory) throws Exception {
        Path fake = Files.writeString(directory.resolve(System.mapLibraryName("cleanrules_ffi")), "not a library");
        // A directory is searched for the library's file name.
        List<String> lines = load(directory.toString());
        assertEquals(4, lines.size(), lines.toString());
        assertTrue(lines.get(0).startsWith("java.lang.UnsatisfiedLinkError: cannot load the cleanrules library at "
                + fake.toRealPath()) || lines.get(0).contains(fake.toString()), lines.get(0));
        assertEquals(List.of(lines.get(0), lines.get(0), lines.get(0), lines.get(0)), lines);
    }

    @Test
    void withoutNativeAccessTheErrorNamesTheFlag() throws Exception {
        String expected = "java.lang.UnsatisfiedLinkError: native access is not enabled for io.github.cleanrules:"
                + " run with --enable-native-access=io.github.cleanrules (module path) or ALL-UNNAMED (class path)";
        assertEquals(
                List.of(expected, expected, expected, expected),
                load(CleanRules.libraryPath().orElseThrow().toString(), "--illegal-native-access=deny"));
    }

    @Test
    void theLibraryOfThePropertyLoadsInANewProcess() throws Exception {
        assertEquals(
                List.of("loaded", "1024", "loaded", "1024"), load(CleanRules.libraryPath().orElseThrow().toString()));
    }
}
