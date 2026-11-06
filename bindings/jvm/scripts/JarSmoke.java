// The smoke test of the jar (.github/workflows/jvm-natives.yml): the binding
// loads the native library the jar bundles for this platform, with no
// override, and evaluates rules through both layers. Run it with the source
// launcher, the jar on the class path or the module path:
//
//   java --enable-native-access=ALL-UNNAMED -cp cleanrules-<version>.jar JarSmoke.java
//   java --enable-native-access=io.github.cleanrules --module-path cleanrules-<version>.jar \
//        --add-modules io.github.cleanrules JarSmoke.java

import static io.github.cleanrules.Conditions.field;

import io.github.cleanrules.CleanRules;
import io.github.cleanrules.Decision;
import io.github.cleanrules.Format;
import io.github.cleanrules.Rule;
import io.github.cleanrules.RuleSet;
import java.math.BigDecimal;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.Optional;

public final class JarSmoke {
    private JarSmoke() {}

    private static void check(boolean condition, String what) {
        if (!condition) {
            throw new AssertionError(what);
        }
    }

    public static void main(String[] arguments) {
        check(System.getProperty("cleanrules.library.path") == null, "cleanrules.library.path must not be set");
        check(System.getenv("CLEANRULES_LIBRARY_PATH") == null, "CLEANRULES_LIBRARY_PATH must not be set");

        // The idiomatic API.
        Rule vip = Rule.named("vip")
                .when(field("customer", "years").gt(5))
                .then("discount", new BigDecimal("0.15"));
        Decision decision = RuleSet.of(vip).evaluate(Map.of("customer", Map.of("years", 7)));
        check(decision.output().equals(Optional.of(Map.of("discount", new BigDecimal("0.15")))),
                "output " + decision.output());

        // The native layer, on JSON.
        try (var rules = io.github.cleanrules.ffi.RuleSet.read("IF a > 1 THEN b = 2", Format.TEXT);
                var native_ = rules.evaluateJson("{\"a\": 5}")) {
            check(native_.outputJson().equals("{\"b\":2}"), "native output " + native_.outputJson());
        }

        // From the jar: extracted to a private directory under the temporary
        // directory, and deleted once loaded but on Windows.
        Path library = CleanRules.libraryPath().orElseThrow(() -> new AssertionError("no library path"));
        String tmpdir = System.getProperty("cleanrules.tmpdir", System.getProperty("java.io.tmpdir"));
        check(library.getFileName().toString().equals(System.mapLibraryName("cleanrules_ffi")), "file " + library);
        check(library.getParent().getFileName().toString().startsWith("cleanrules-native-"), "directory " + library);
        check(library.getParent().getParent().equals(Path.of(tmpdir)), library + " is not in " + tmpdir);
        boolean windows = System.getProperty("os.name").startsWith("Windows");
        check(Files.exists(library) == windows, library + (windows ? " was deleted" : " was not deleted"));

        System.out.println("cleanrules " + CleanRules.version() + " on " + System.getProperty("os.name") + " "
                + System.getProperty("os.arch") + ", from " + library + (windows ? "" : " (deleted)")
                + ": " + decision.output().orElseThrow());
    }
}
