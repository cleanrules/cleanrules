package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.internal.Bundled;
import io.github.cleanrules.internal.Bundled.ExtractionException;
import io.github.cleanrules.internal.Platform;
import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.FileSystems;
import java.nio.file.LinkOption;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.attribute.BasicFileAttributeView;
import java.nio.file.attribute.FileTime;
import java.nio.file.attribute.PosixFilePermissions;
import java.nio.file.attribute.UserPrincipal;
import java.security.MessageDigest;
import java.time.Duration;
import java.time.Instant;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.stream.Stream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.DisabledOnOs;
import org.junit.jupiter.api.condition.EnabledOnOs;
import org.junit.jupiter.api.condition.OS;
import org.junit.jupiter.api.io.TempDir;

/** Extracting the jar's library to a private temporary directory, with fake resources. */
class BundledTest {
    private static final Platform LINUX = Platform.of("Linux", "amd64", () -> "gnu");
    private static final byte[] LIBRARY = "not really a library, but bytes to extract".getBytes(StandardCharsets.UTF_8);

    private static String sha256(byte[] bytes) throws Exception {
        return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
    }

    /** Resources holding {@code library} for {@code platform}, with {@code checksum} as its .sha256 (none if null). */
    private static Bundled.Resources resources(Platform platform, byte[] library, String checksum) {
        Map<String, byte[]> files = new HashMap<>();
        String name = Bundled.resource(platform);
        files.put(name, library);
        if (checksum != null) {
            files.put(name + ".sha256", checksum.getBytes(StandardCharsets.US_ASCII));
        }
        return resource -> files.containsKey(resource) ? new ByteArrayInputStream(files.get(resource)) : null;
    }

    private static Bundled.Resources intact(Platform platform) throws Exception {
        return resources(platform, LIBRARY, sha256(LIBRARY) + "  " + platform.fileName() + "\n");
    }

    private static List<Path> list(Path directory) throws IOException {
        try (Stream<Path> entries = Files.list(directory)) {
            return entries.sorted().toList();
        }
    }

    private static boolean posix() {
        return FileSystems.getDefault().supportedFileAttributeViews().contains("posix");
    }

    @Test
    void eachPlatformHasItsResource() {
        List<String> names = Platform.BUNDLED.stream()
                .map(name -> {
                    String[] parts = name.split("-");
                    return Bundled.resource(new Platform(parts[0], parts[1], parts.length > 2 ? parts[2] : null));
                })
                .toList();
        assertEquals(
                List.of(
                        "/native/darwin-x64/libcleanrules_ffi.dylib",
                        "/native/darwin-arm64/libcleanrules_ffi.dylib",
                        "/native/linux-x64-gnu/libcleanrules_ffi.so",
                        "/native/linux-arm64-gnu/libcleanrules_ffi.so",
                        "/native/linux-x64-musl/libcleanrules_ffi.so",
                        "/native/linux-arm64-musl/libcleanrules_ffi.so",
                        "/native/win32-x64-msvc/cleanrules_ffi.dll",
                        "/native/win32-arm64-msvc/cleanrules_ffi.dll"),
                names);
    }

    @Test
    void theLibraryIsExtractedIntoANewPrivateDirectory(@TempDir Path root) throws Exception {
        Path first = Bundled.extract(intact(LINUX), LINUX, root);
        Path second = Bundled.extract(intact(LINUX), LINUX, root);
        for (Path file : List.of(first, second)) {
            assertEquals("libcleanrules_ffi.so", file.getFileName().toString());
            assertEquals(root, file.getParent().getParent());
            assertTrue(file.getParent().getFileName().toString().startsWith(Bundled.PREFIX), file.toString());
            assertArrayEquals(LIBRARY, Files.readAllBytes(file));
            assertEquals(List.of(file), list(file.getParent()));
            if (posix()) {
                assertEquals("rwx------", PosixFilePermissions.toString(Files.getPosixFilePermissions(file.getParent())));
                assertEquals("rwx------", PosixFilePermissions.toString(Files.getPosixFilePermissions(file)));
            }
        }
        // One directory per extraction: two JVMs never share a file.
        assertNotEquals(first.getParent(), second.getParent());
        assertEquals(2, list(root).size());
    }

    @Test
    void anUpperCaseChecksumOrAChecksumAloneIsRead(@TempDir Path root) throws Exception {
        Path upper = Bundled.extract(resources(LINUX, LIBRARY, sha256(LIBRARY).toUpperCase(java.util.Locale.ROOT)), LINUX, root);
        assertArrayEquals(LIBRARY, Files.readAllBytes(upper));
    }

    @Test
    void withoutALibraryForThePlatformNothingIsExtracted(@TempDir Path root) throws Exception {
        Platform windows = Platform.of("Windows 11", "amd64", () -> "gnu");
        assertNull(Bundled.extract(intact(LINUX), windows, root));
        assertNull(Bundled.extract(resource -> null, LINUX, root));
        assertEquals(List.of(), list(root));
    }

    @Test
    void aLibraryThatIsNotWhatItsChecksumSaysIsAnError(@TempDir Path root) throws Exception {
        String other = sha256("other bytes".getBytes(StandardCharsets.UTF_8));
        var error = assertThrows(
                ExtractionException.class, () -> Bundled.extract(resources(LINUX, LIBRARY, other), LINUX, root));
        assertEquals(
                "the jar's /native/linux-x64-gnu/libcleanrules_ffi.so has SHA-256 " + sha256(LIBRARY)
                        + ", and its /native/linux-x64-gnu/libcleanrules_ffi.so.sha256 says " + other
                        + ": the jar is damaged; use an intact one",
                error.getMessage());
        // Nothing is left behind.
        assertEquals(List.of(), list(root));
    }

    @Test
    void aMissingOrUnreadableChecksumIsAnError(@TempDir Path root) throws Exception {
        var missing = assertThrows(
                ExtractionException.class, () -> Bundled.extract(resources(LINUX, LIBRARY, null), LINUX, root));
        assertEquals(
                "the jar has /native/linux-x64-gnu/libcleanrules_ffi.so but not"
                        + " /native/linux-x64-gnu/libcleanrules_ffi.so.sha256, its checksum: the jar is incomplete;"
                        + " use one built by the cleanrules build",
                missing.getMessage());
        var garbled = assertThrows(
                ExtractionException.class,
                () -> Bundled.extract(resources(LINUX, LIBRARY, "abc  libcleanrules_ffi.so"), LINUX, root));
        assertEquals(
                "the jar's /native/linux-x64-gnu/libcleanrules_ffi.so.sha256 holds no SHA-256 (abc "
                        + " libcleanrules_ffi.so): the jar is damaged; use an intact one",
                garbled.getMessage());
        var empty = assertThrows(
                ExtractionException.class, () -> Bundled.extract(resources(LINUX, LIBRARY, ""), LINUX, root));
        assertTrue(empty.getMessage().contains("holds no SHA-256 ()"), empty.getMessage());
        assertEquals(List.of(), list(root));
    }

    @Test
    void aResourceThatCannotBeReadIsAnError(@TempDir Path root) {
        var error = assertThrows(
                ExtractionException.class,
                () -> Bundled.extract(
                        resource -> {
                            throw new IOException("zip file closed");
                        },
                        LINUX,
                        root));
        assertEquals(
                "cannot read /native/linux-x64-gnu/libcleanrules_ffi.so from the jar: IOException: zip file closed",
                error.getMessage());
    }

    @Test
    void aDirectoryThatCannotBeWrittenIsAnErrorNamingTheProperty(@TempDir Path root) throws Exception {
        Path missing = root.resolve("missing");
        var error = assertThrows(ExtractionException.class, () -> Bundled.extract(intact(LINUX), LINUX, missing));
        String message = error.getMessage();
        assertTrue(
                message.startsWith("cannot extract the cleanrules library for linux-x64-gnu to " + missing
                        + ": NoSuchFileException: "),
                message);
        assertTrue(message.endsWith("; set the system property cleanrules.tmpdir to a writable directory"), message);
        assertFalse(Files.exists(missing));
    }

    @Test
    void theRootIsThePropertyOrJavasTemporaryDirectory() {
        String before = System.getProperty(Bundled.TMPDIR);
        try {
            System.clearProperty(Bundled.TMPDIR);
            assertEquals(Path.of(System.getProperty("java.io.tmpdir")), Bundled.root());
            System.setProperty(Bundled.TMPDIR, "");
            assertEquals(Path.of(System.getProperty("java.io.tmpdir")), Bundled.root());
            System.setProperty(Bundled.TMPDIR, "/some/where");
            assertEquals(Path.of("/some/where"), Bundled.root());
        } finally {
            if (before == null) {
                System.clearProperty(Bundled.TMPDIR);
            } else {
                System.setProperty(Bundled.TMPDIR, before);
            }
        }
    }

    @Test
    void thePlatformsOfTheResourcesAreListed() throws Exception {
        Platform musl = Platform.of("Linux", "aarch64", () -> "musl");
        Platform mac = Platform.of("Mac OS X", "aarch64", () -> "gnu");
        Map<String, byte[]> files = Map.of(
                Bundled.resource(musl), LIBRARY, Bundled.resource(mac), LIBRARY, "/native/other/x.so", LIBRARY);
        Bundled.Resources resources =
                name -> files.containsKey(name) ? new ByteArrayInputStream(files.get(name)) : null;
        // In the order of the npm packages, whatever the order of the jar.
        assertEquals(List.of("darwin-arm64", "linux-arm64-musl"), Bundled.platforms(resources));
        assertEquals(List.of(), Bundled.platforms(name -> null));
        assertEquals(List.of(), Bundled.platforms(name -> {
            throw new IOException("closed");
        }));
        // The test classes and resources bundle none.
        assertEquals(List.of(), Bundled.platforms(Bundled.jar()));
    }

    @Test
    void aLoadedLibraryIsDeletedExceptOnWindows(@TempDir Path root) throws Exception {
        Path loaded = Bundled.extract(intact(LINUX), LINUX, root);
        Bundled.release(loaded, true, false);
        assertEquals(List.of(), list(root));

        Path onWindows = Bundled.extract(intact(LINUX), LINUX, root);
        Bundled.release(onWindows, true, true);
        assertTrue(Files.exists(onWindows));

        // One that was not loaded goes everywhere.
        Path failed = Bundled.extract(intact(LINUX), LINUX, root);
        Bundled.release(failed, false, true);
        assertFalse(Files.exists(failed.getParent()));
        assertEquals(List.of(onWindows.getParent()), list(root));

        // Never throws, also for what is gone already.
        Bundled.release(failed, false, false);
    }

    private static Path aged(Path path, Instant when) throws IOException {
        Files.setLastModifiedTime(path, FileTime.from(when));
        return path;
    }

    @Test
    void oldExtractionsHoldingOnlyTheLibraryAreSweptAway(@TempDir Path root) throws Exception {
        Instant now = Instant.parse("2026-11-06T12:00:00Z");
        Instant old = now.minus(Duration.ofHours(2));

        Path stale = Files.createDirectory(root.resolve(Bundled.PREFIX + "1"));
        Files.write(stale.resolve("cleanrules_ffi.dll"), LIBRARY);
        aged(stale, old);
        Path staleEmpty = aged(Files.createDirectory(root.resolve(Bundled.PREFIX + "2")), old);
        Path recent = Files.createDirectory(root.resolve(Bundled.PREFIX + "3"));
        Files.write(recent.resolve("cleanrules_ffi.dll"), LIBRARY);
        aged(recent, now.minus(Duration.ofMinutes(5)));
        Path someoneElses = Files.createDirectory(root.resolve(Bundled.PREFIX + "4"));
        Files.write(someoneElses.resolve("cleanrules_ffi.dll"), LIBRARY);
        Files.write(someoneElses.resolve("notes.txt"), LIBRARY);
        aged(someoneElses, old);
        Path nested = Files.createDirectory(root.resolve(Bundled.PREFIX + "5"));
        Files.createDirectory(nested.resolve("libcleanrules_ffi.so"));
        aged(nested, old);
        Path otherName = Files.createDirectory(root.resolve("cleanrules-other"));
        Files.write(otherName.resolve("cleanrules_ffi.dll"), LIBRARY);
        aged(otherName, old);
        Path aFile = aged(Files.write(root.resolve(Bundled.PREFIX + "6"), LIBRARY), old);

        Bundled.sweep(root, now, Bundled.STALE, owner(root));

        assertFalse(Files.exists(stale));
        assertFalse(Files.exists(staleEmpty));
        assertEquals(List.of(recent, someoneElses, nested, aFile, otherName).stream().sorted().toList(), list(root));
        assertTrue(Files.exists(someoneElses.resolve("cleanrules_ffi.dll")));
        assertTrue(Files.exists(nested.resolve("libcleanrules_ffi.so")));
        assertEquals(Duration.ofHours(1), Bundled.STALE);

        // A root that does not exist is no error.
        Bundled.sweep(root.resolve("missing"), now, Bundled.STALE, owner(root));
    }

    /** The owner of a directory this JVM created, as Native takes it. */
    private static UserPrincipal owner(Path directory) throws IOException {
        return Files.getOwner(directory, LinkOption.NOFOLLOW_LINKS);
    }

    /** Set a link's own times, not its target's. */
    private static void agedLink(Path link, Instant when) throws IOException {
        Files.getFileAttributeView(link, BasicFileAttributeView.class, LinkOption.NOFOLLOW_LINKS)
                .setTimes(FileTime.from(when), null, null);
    }

    @Test
    void anotherUsersExtractionsAreLeftAlone(@TempDir Path root) throws Exception {
        Instant now = Instant.parse("2026-11-06T12:00:00Z");
        Path stale = Files.createDirectory(root.resolve(Bundled.PREFIX + "1"));
        Files.write(stale.resolve("cleanrules_ffi.dll"), LIBRARY);
        aged(stale, now.minus(Duration.ofHours(2)));
        UserPrincipal stranger = () -> "someone-else";
        Bundled.sweep(root, now, Bundled.STALE, stranger);
        assertTrue(Files.exists(stale.resolve("cleanrules_ffi.dll")));
        Bundled.sweep(root, now, Bundled.STALE, owner(root));
        assertFalse(Files.exists(stale));
    }

    @Test
    @DisabledOnOs(OS.WINDOWS) // Symbolic links need a privilege there; junctions are tested below.
    void aSymbolicLinkIsNeverFollowed(@TempDir Path root, @TempDir Path elsewhere) throws Exception {
        Instant now = Instant.parse("2026-11-06T12:00:00Z");
        Instant old = now.minus(Duration.ofHours(2));
        Path library = Files.write(elsewhere.resolve("cleanrules_ffi.dll"), LIBRARY);
        aged(elsewhere, old);
        Path link = Files.createSymbolicLink(root.resolve(Bundled.PREFIX + "1"), elsewhere);
        agedLink(link, old);
        Bundled.sweep(root, now, Bundled.STALE, owner(root));
        assertTrue(Files.exists(library));
        assertTrue(Files.isSymbolicLink(link));
    }

    @Test
    @EnabledOnOs(OS.WINDOWS)
    void aJunctionIsNeverFollowed(@TempDir Path root, @TempDir Path elsewhere) throws Exception {
        Instant now = Instant.parse("2026-11-06T12:00:00Z");
        Instant old = now.minus(Duration.ofHours(2));
        Path library = Files.write(elsewhere.resolve("cleanrules_ffi.dll"), LIBRARY);
        aged(elsewhere, old);
        Path junction = root.resolve(Bundled.PREFIX + "1");
        Process mklink = new ProcessBuilder("cmd", "/c", "mklink", "/J", junction.toString(), elsewhere.toString())
                .redirectErrorStream(true)
                .start();
        String output = new String(mklink.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
        assertEquals(0, mklink.waitFor(), output);
        // Java reads a junction as a directory (and "other"): the sweep must not go through it.
        agedLink(junction, old);
        Bundled.sweep(root, now, Bundled.STALE, owner(root));
        assertTrue(Files.exists(library));
        assertTrue(Files.exists(junction, LinkOption.NOFOLLOW_LINKS));
        // The junction alone, so that cleaning up the temporary directories never goes through it.
        Files.delete(junction);
        assertTrue(Files.exists(library));
    }
}
