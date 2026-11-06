package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.internal.Bundled;
import io.github.cleanrules.internal.Platform;
import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.nio.file.attribute.FileTime;
import java.security.MessageDigest;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;
import java.util.stream.Stream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledOnOs;
import org.junit.jupiter.api.condition.OS;
import org.junit.jupiter.api.io.TempDir;

/**
 * Loading the library the jar bundles, in new JVMs whose class path has a
 * directory laid out as the jar is ({@code native/<platform>/<file>} and its
 * {@code .sha256}), holding the library the tests build or something else.
 */
class BundledLoadingTest {
    private static final Platform HOST = Platform.current();
    private static final String LOADED = "loaded {\"b\":2} from ";

    /** The library the tests load (from cleanrules.library.path). */
    private static Path library() {
        return Path.of(System.getProperty("cleanrules.library.path"));
    }

    /** A class path directory holding {@code bytes} as the library of {@code platform}, with {@code checksum}. */
    private static Path jarLike(Path directory, String platform, byte[] bytes, String checksum) throws Exception {
        String file = new Platform(platform.split("-")[0], "", null).fileName();
        Path native_ = Files.createDirectories(directory.resolve("native").resolve(platform));
        Files.write(native_.resolve(file), bytes);
        String sha = checksum != null
                ? checksum
                : HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
        Files.writeString(native_.resolve(file + ".sha256"), sha + "  " + file + "\n");
        return directory;
    }

    private static Path jarLike(Path directory) throws Exception {
        return jarLike(directory, HOST.name(), Files.readAllBytes(library()), null);
    }

    /**
     * What {@link ShowLibrary} prints in a new JVM with {@code resources} on its class path, the library extracted
     * under {@code tmp}, no java.library.path and none of the variables that would find the library otherwise.
     */
    private static List<String> show(Path resources, Path tmp, String... options) throws Exception {
        ProcessBuilder builder = jvm(ShowLibrary.class, resources, tmp, options);
        Path errors = Files.createTempFile("cleanrules-show", ".err");
        try {
            Process process = builder.redirectError(errors.toFile()).start();
            String output = new String(process.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
            assertEquals(0, process.waitFor(), output + Files.readString(errors));
            return output.lines().toList();
        } finally {
            Files.deleteIfExists(errors);
        }
    }

    /** A new JVM that runs {@code program} as {@link #show} says. */
    private static ProcessBuilder jvm(Class<?> program, Path resources, Path tmp, String... options)
            throws IOException {
        String java = Path.of(System.getProperty("java.home"), "bin", "java").toString();
        Path empty = Files.createDirectories(tmp.resolveSibling("empty"));
        List<String> command = new ArrayList<>(List.of(
                java,
                "--enable-native-access=ALL-UNNAMED",
                "--illegal-native-access=deny",
                "-Djava.library.path=" + empty,
                "-Dcleanrules.tmpdir=" + tmp));
        command.addAll(List.of(options));
        command.addAll(List.of(
                "-cp",
                System.getProperty("java.class.path") + File.pathSeparator + resources,
                program.getName()));
        ProcessBuilder builder = new ProcessBuilder(command);
        for (String variable : List.of(
                "CLEANRULES_LIBRARY_PATH", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH")) {
            builder.environment().remove(variable);
        }
        return builder;
    }

    private static List<Path> list(Path directory) throws IOException {
        try (Stream<Path> entries = Files.list(directory)) {
            return entries.toList();
        }
    }

    @Test
    void theJarsLibraryIsExtractedCheckedLoadedAndDeleted(@TempDir Path directory) throws Exception {
        Path tmp = Files.createDirectory(directory.resolve("tmp"));
        List<String> lines = show(jarLike(directory.resolve("classes")), tmp);
        assertEquals(4, lines.size(), lines.toString());
        assertTrue(lines.get(0).startsWith(LOADED + tmp.resolve(Bundled.PREFIX)), lines.toString());
        assertTrue(lines.get(0).endsWith(HOST.fileName()), lines.toString());
        // The same library both times: loaded once.
        assertEquals(lines.subList(0, 2), lines.subList(2, 4));
        if (HOST.windows()) {
            assertEquals("file kept", lines.get(1));
            assertEquals(1, list(tmp).size());
        } else {
            // Deleted once loaded: nothing is left behind.
            assertEquals("file deleted", lines.get(1));
            assertEquals(List.of(), list(tmp));
        }
    }

    @Test
    void thePropertyComesBeforeTheJar(@TempDir Path directory) throws Exception {
        Path tmp = Files.createDirectory(directory.resolve("tmp"));
        List<String> lines = show(
                jarLike(directory.resolve("classes")), tmp, "-Dcleanrules.library.path=" + library());
        assertEquals(LOADED + library().toAbsolutePath(), lines.get(0));
        assertEquals("file kept", lines.get(1));
        // Nothing was extracted.
        assertEquals(List.of(), list(tmp));
    }

    @Test
    void aDamagedLibraryInTheJarIsAClearError(@TempDir Path directory) throws Exception {
        Path tmp = Files.createDirectory(directory.resolve("tmp"));
        String wrong = "0".repeat(64);
        List<String> lines = show(
                jarLike(directory.resolve("classes"), HOST.name(), Files.readAllBytes(library()), wrong), tmp);
        String resource = Bundled.resource(HOST);
        String expected = "java.lang.UnsatisfiedLinkError: the jar's " + resource + " has SHA-256 ";
        assertTrue(lines.get(0).startsWith(expected), lines.toString());
        assertTrue(
                lines.get(0)
                        .endsWith(", and its " + resource + ".sha256 says " + wrong
                                + ": the jar is damaged; use an intact one; or set the system property"
                                + " cleanrules.library.path or the environment variable CLEANRULES_LIBRARY_PATH to"
                                + " a cleanrules library of this release for " + HOST),
                lines.toString());
        // The same error every time, and nothing left behind.
        assertEquals(List.of(lines.get(0), lines.get(0)), lines);
        assertEquals(List.of(), list(tmp));
    }

    @Test
    void aLibraryOfTheJarThatCannotLoadIsAClearError(@TempDir Path directory) throws Exception {
        Path tmp = Files.createDirectory(directory.resolve("tmp"));
        byte[] notALibrary = "not a library".getBytes(StandardCharsets.UTF_8);
        List<String> lines = show(jarLike(directory.resolve("classes"), HOST.name(), notALibrary, null), tmp);
        String line = lines.get(0);
        assertTrue(
                line.startsWith("java.lang.UnsatisfiedLinkError: cannot load the cleanrules library for " + HOST),
                line);
        assertTrue(line.contains(", extracted from the jar to " + tmp.resolve(Bundled.PREFIX)), line);
        assertTrue(
                line.endsWith(". If that directory does not allow programs (mounted noexec), set the system property"
                        + " cleanrules.tmpdir to one that does; or set the system property cleanrules.library.path or"
                        + " the environment variable CLEANRULES_LIBRARY_PATH to a cleanrules library of this release"
                        + " built for this system"),
                line);
        assertEquals(List.of(line, line), lines);
        // Not loaded, so deleted, on Windows too.
        assertEquals(List.of(), list(tmp));
    }

    @Test
    void aJarWithoutThisPlatformsLibraryNamesItAndTheProperty(@TempDir Path directory) throws Exception {
        Path tmp = Files.createDirectory(directory.resolve("tmp"));
        String other = HOST.name().equals("linux-x64-musl") ? "linux-arm64-musl" : "linux-x64-musl";
        List<String> lines = show(
                jarLike(directory.resolve("classes"), other, "elsewhere".getBytes(StandardCharsets.UTF_8), null), tmp);
        String expected = "java.lang.UnsatisfiedLinkError: this jar has no cleanrules library for " + HOST
                + " (it has " + other + "), and there is no " + HOST.fileName()
                + " on java.library.path or the system's library path: build it for this platform (cargo build"
                + " --release -p cleanrules-ffi) and set the system property cleanrules.library.path or the"
                + " environment variable CLEANRULES_LIBRARY_PATH to its path";
        assertEquals(List.of(expected, expected), lines);
        assertEquals(List.of(), list(tmp));
    }

    @Test
    void aPlatformWithoutALibraryNamesItsSystemAndProcessor(@TempDir Path directory) throws Exception {
        Path tmp = Files.createDirectory(directory.resolve("tmp"));
        String arch = System.getProperty("os.arch");
        Platform freebsd = Platform.of("FreeBSD", arch, HOST::abi);
        List<String> lines = show(jarLike(directory.resolve("classes")), tmp, "-Dos.name=FreeBSD");
        String expected = "java.lang.UnsatisfiedLinkError: cleanrules bundles no library for " + freebsd
                + " (os.name FreeBSD, os.arch " + arch + "; the jar has " + HOST + "), and there is no "
                + HOST.fileName()
                + " on java.library.path or the system's library path: build it for this platform (cargo build"
                + " --release -p cleanrules-ffi) and set the system property cleanrules.library.path or the"
                + " environment variable CLEANRULES_LIBRARY_PATH to its path";
        assertEquals(List.of(expected, expected), lines);
        assertEquals(List.of(), list(tmp));
    }

    @Test
    void aProcessorTheRuntimeHasNoLinkerForIsAClearError(@TempDir Path directory) throws Exception {
        Path tmp = Files.createDirectory(directory.resolve("tmp"));
        // Java's own linker follows os.arch: without one, the binding says so
        // (rather than failing to initialise); with one, there is no library.
        List<String> lines = show(jarLike(directory.resolve("classes")), tmp, "-Dos.arch=riscv64");
        String line = lines.get(0);
        assertTrue(
                line.startsWith("java.lang.UnsatisfiedLinkError: this Java runtime has no native linker for os.name "
                                + System.getProperty("os.name") + ", os.arch riscv64 (")
                        || line.startsWith("java.lang.UnsatisfiedLinkError: cleanrules bundles no library for "),
                line);
        assertTrue(line.contains("riscv64"), line);
        assertEquals(List.of(line, line), lines);
        assertEquals(List.of(), list(tmp));
    }

    @Test
    void inANativeImageTheErrorSaysToRegisterTheResources(@TempDir Path directory) throws Exception {
        Path tmp = Files.createDirectory(directory.resolve("tmp"));
        // GraalVM sets this property in a native image; the jar's resources are absent there.
        List<String> lines = show(directory.resolve("classes"), tmp, "-Dorg.graalvm.nativeimage.imagecode=runtime");
        String line = lines.get(0);
        assertTrue(line.startsWith("java.lang.UnsatisfiedLinkError: this jar has no cleanrules library for " + HOST
                + " (it has none)"), line);
        assertTrue(
                line.endsWith(" to its path (native image: register the resources /native/" + HOST
                        + "/*, or set cleanrules.library.path)"),
                line);
        assertEquals(List.of(line, line), lines);
    }

    @Test
    @EnabledOnOs(OS.WINDOWS)
    void theSweepLeavesADllAnotherJvmHasLoaded(@TempDir Path directory) throws Exception {
        Path tmp = Files.createDirectory(directory.resolve("tmp"));
        Process holder = jvm(HoldLibrary.class, jarLike(directory.resolve("classes")), tmp)
                .redirectError(ProcessBuilder.Redirect.DISCARD)
                .start();
        try {
            var output = new java.io.BufferedReader(
                    new java.io.InputStreamReader(holder.getInputStream(), StandardCharsets.UTF_8));
            Path library = Path.of(output.readLine());
            Path extracted = library.getParent();
            assertEquals(tmp, extracted.getParent());
            // Two hours old: only being loaded protects it now.
            Files.setLastModifiedTime(extracted, FileTime.from(Instant.now().minus(Duration.ofHours(2))));
            Bundled.sweep(tmp, Instant.now(), Bundled.STALE, Files.getOwner(extracted, LinkOption.NOFOLLOW_LINKS));
            assertTrue(Files.exists(library), library.toString());
            assertTrue(Files.isDirectory(extracted));
        } finally {
            holder.getOutputStream().close();
            assertEquals(0, holder.waitFor());
        }
        // Once that JVM is gone, the next sweep takes it.
        Bundled.sweep(tmp, Instant.now(), Bundled.STALE, Files.getOwner(tmp, LinkOption.NOFOLLOW_LINKS));
        assertEquals(List.of(), list(tmp));
    }

    @Test
    void aTemporaryDirectoryThatCannotBeWrittenNamesTheProperty(@TempDir Path directory) throws Exception {
        Path tmp = Files.createDirectory(directory.resolve("tmp"));
        Path missing = tmp.resolve("missing");
        List<String> lines =
                show(jarLike(directory.resolve("classes")), tmp, "-Dcleanrules.tmpdir=" + missing);
        String line = lines.get(0);
        assertTrue(
                line.startsWith("java.lang.UnsatisfiedLinkError: cannot extract the cleanrules library for " + HOST
                        + " to " + missing + ": NoSuchFileException: "),
                line);
        assertTrue(
                line.endsWith("; set the system property cleanrules.tmpdir to a writable directory; or set the system"
                        + " property cleanrules.library.path or the environment variable CLEANRULES_LIBRARY_PATH to a"
                        + " cleanrules library of this release for " + HOST),
                line);
        assertEquals(List.of(line, line), lines);
    }
}
