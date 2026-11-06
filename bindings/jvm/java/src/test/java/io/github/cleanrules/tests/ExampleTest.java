package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;

/** docs/examples/Discounts.java, DiscountRules.java and RiskTable.java run as their comments say, with the source launcher. */
class ExampleTest {
    /** The output of docs/examples/Discounts.java, run with the source launcher and {@code options}. */
    private static String run(String... options) throws Exception {
        return runExample("Discounts.java", options);
    }

    /** The output of an example, run with the source launcher and {@code options}. */
    private static String runExample(String example, String... options) throws Exception {
        return runWith(example, "discounts.rules", options);
    }

    /** The output of an example given the example file {@code argument}, run with the source launcher. */
    private static String runWith(String example, String argument, String... options) throws Exception {
        Path examples = Path.of(System.getProperty("cleanrules.examples"));
        List<String> command = new ArrayList<>();
        command.add(Path.of(System.getProperty("java.home"), "bin", "java").toString());
        command.add("-Dcleanrules.library.path=" + System.getProperty("cleanrules.library.path"));
        command.addAll(List.of(options));
        command.add(examples.resolve(example).toString());
        command.add(examples.resolve(argument).toString());
        Process process = new ProcessBuilder(command).redirectErrorStream(true).start();
        String output = new String(process.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
        assertEquals(0, process.waitFor(), output);
        return output;
    }

    @Test
    void theExampleRunsOnTheClassPath() throws Exception {
        check(run("--enable-native-access=ALL-UNNAMED", "-cp", System.getProperty("java.class.path")));
    }

    @Test
    void theExampleRunsOnTheModulePath() throws Exception {
        // The named module, allowed native access by name, and nothing else.
        String output = run(
                "--enable-native-access=io.github.cleanrules",
                "--illegal-native-access=deny",
                "--module-path",
                System.getProperty("cleanrules.module"),
                "--add-modules",
                "io.github.cleanrules");
        check(output);
        assertTrue(!output.contains("WARNING"), output);
    }

    @Test
    void theIdiomaticExampleRunsOnTheModulePath() throws Exception {
        String output = runExample(
                "DiscountRules.java",
                "--enable-native-access=io.github.cleanrules",
                "--illegal-native-access=deny",
                "--module-path",
                System.getProperty("cleanrules.module"),
                "--add-modules",
                "io.github.cleanrules");
        List<String> lines = output.lines().toList();
        assertEquals("discount: 0.15", lines.get(0), output);
        assertEquals("fired: vip, loyal", lines.get(1), output);
        assertTrue(lines.contains("HIT POLICY priority: selected vip"), output);
        assertTrue(lines.contains("vip: order.total > 1000 is false; needs gt, short by 100.00"), output);
        assertTrue(lines.contains("notify_sales for vip: 0.15"), output);
        assertEquals("missing_field in rule adult: [customer, age]", lines.getLast(), output);
        assertTrue(!output.contains("WARNING"), output);
    }

    @Test
    void theDocumentsExampleRunsOnTheModulePath() throws Exception {
        String output = runWith(
                "RiskTable.java",
                "risk.csv",
                "--enable-native-access=io.github.cleanrules",
                "--illegal-native-access=deny",
                "--module-path",
                System.getProperty("cleanrules.module"),
                "--add-modules",
                "io.github.cleanrules");
        List<String> lines = output.lines().toList();
        assertEquals("risk: medium, discount: 0.05", lines.get(0), output);
        assertEquals("HIT POLICY priority: selected big", lines.get(1), output);
        assertTrue(lines.contains("same table: true"), output);
        assertTrue(lines.contains("rows: new, vip, big, default"), output);
        assertTrue(lines.contains("vip: unknown; order.total > 1000 is unknown (missing_field)"), output);
        assertEquals("notify for vip: customer.years > 5 is true", lines.getLast(), output);
        assertTrue(!output.contains("WARNING"), output);
    }

    private static void check(String output) {
        List<String> lines = output.lines().toList();
        assertEquals("discount: 0.15", lines.get(0), output);
        assertEquals("selected: vip", lines.get(1), output);
        assertTrue(lines.contains("notify_sales for vip: 0.15"), output);
        assertTrue(lines.contains("ran: 1 rule"), output);
        assertTrue(lines.contains("action_error in vip: the sales service is down"), output);
        assertTrue(lines.getLast().startsWith("missing_field: "), output);
    }
}
