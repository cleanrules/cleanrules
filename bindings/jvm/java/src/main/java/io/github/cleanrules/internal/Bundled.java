package io.github.cleanrules.internal;

import static java.nio.file.LinkOption.NOFOLLOW_LINKS;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.DirectoryStream;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.nio.file.attribute.BasicFileAttributes;
import java.nio.file.attribute.FileAttribute;
import java.nio.file.attribute.PosixFilePermission;
import java.nio.file.attribute.PosixFilePermissions;
import java.nio.file.attribute.UserPrincipal;
import java.security.DigestInputStream;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;
import java.util.Set;

/**
 * The native library bundled in the jar: at {@code /native/<platform>/<file>}
 * with its SHA-256 in {@code <file>.sha256} (as {@code sha256sum} writes it),
 * extracted to a private temporary directory before it is loaded.
 *
 * <p>Each JVM (each class loader that loads the binding) extracts its own
 * copy into a new directory {@code cleanrules-native-<random>} that only its
 * user may read, write or enter ({@code rwx------} where the file system has
 * POSIX permissions; on Windows it inherits the access control lists of the
 * directory it is in, private in the default per-user {@code %TEMP%}), under
 * the system property {@code cleanrules.tmpdir} or else
 * {@code java.io.tmpdir}. The file is created there, never replaced, so no
 * other process can race to swap it, and its size and SHA-256 are checked
 * against the jar's before it is loaded. On Linux and macOS the file and the
 * directory are deleted as soon as the library is loaded (the mapping stays),
 * so nothing is left behind unless the JVM dies while loading it. Windows
 * cannot delete a loaded library: the directory stays until a later JVM of
 * the same user finds it at least an hour old, with nothing in it but a
 * library no process has loaded, and deletes it.
 *
 * <p>Only file operations happen here: {@link Native} loads the file.
 */
public final class Bundled {
    private Bundled() {}

    /** The system property that names the directory the library is extracted to. */
    public static final String TMPDIR = "cleanrules.tmpdir";

    /** The prefix of the directories the library is extracted to. */
    public static final String PREFIX = "cleanrules-native-";

    /** How old an extraction directory is before a later JVM on Windows deletes it. */
    public static final Duration STALE = Duration.ofHours(1);

    /** The library's file names, the only files an extraction directory holds. */
    private static final Set<String> FILES = Set.of(
            "lib" + Native.LIBRARY + ".dylib", "lib" + Native.LIBRARY + ".so", Native.LIBRARY + ".dll");

    /** The resources of the jar, by absolute name. */
    @FunctionalInterface
    public interface Resources {
        /**
         * The resource, or null when there is none.
         *
         * @throws IOException if it cannot be read
         */
        InputStream open(String name) throws IOException;
    }

    /** The resources of the binding's own jar (or module). */
    public static Resources jar() {
        // On the class path another jar's /native/... could shadow this one's;
        // the ABI and format checks of Native catch a library that does not match.
        return Bundled.class::getResourceAsStream;
    }

    /** The resource name of a platform's library. */
    public static String resource(Platform platform) {
        return "/native/" + platform.name() + "/" + platform.fileName();
    }

    /** The platforms whose library {@code resources} hold, in the order of {@link Platform#BUNDLED}. */
    public static List<String> platforms(Resources resources) {
        List<String> found = new ArrayList<>();
        for (String name : Platform.BUNDLED) {
            String[] parts = name.split("-");
            Platform platform = new Platform(parts[0], parts[1], parts.length > 2 ? parts[2] : null);
            try (InputStream in = resources.open(resource(platform))) {
                if (in != null) {
                    found.add(name);
                }
            } catch (IOException cannotRead) {
                // Not listed.
            }
        }
        return found;
    }

    /** The directory to extract to: the system property {@code cleanrules.tmpdir}, else {@code java.io.tmpdir}. */
    public static Path root() {
        String given = System.getProperty(TMPDIR);
        return Path.of(given == null || given.isEmpty() ? System.getProperty("java.io.tmpdir") : given);
    }

    /** Why extracting failed: the message names the platform and what to do. */
    public static final class ExtractionException extends IOException {
        private static final long serialVersionUID = 1L;

        ExtractionException(String message) {
            super(message);
        }

        ExtractionException(String message, Throwable cause) {
            super(message, cause);
        }
    }

    /**
     * Extract the library of {@code platform} into a new private directory
     * under {@code root}, and check it.
     *
     * @return the extracted file, or null when {@code resources} hold no
     *     library for the platform
     * @throws ExtractionException if it cannot be extracted, or what was
     *     extracted is not what the jar holds; nothing is left behind
     */
    public static Path extract(Resources resources, Platform platform, Path root) throws ExtractionException {
        String name = resource(platform);
        InputStream library;
        try {
            library = resources.open(name);
        } catch (IOException cannotRead) {
            throw new ExtractionException("cannot read " + name + " from the jar: " + describe(cannotRead), cannotRead);
        }
        if (library == null) {
            return null;
        }
        Path directory = null;
        Path file = null;
        try (InputStream in = library) {
            String expected = checksum(resources, name);
            boolean posix = FileSystems.getDefault().supportedFileAttributeViews().contains("posix");
            directory = posix
                    ? Files.createTempDirectory(root, PREFIX, ownerOnly())
                    : Files.createTempDirectory(root, PREFIX);
            file = directory.resolve(platform.fileName());
            if (posix) {
                Files.createFile(file, ownerOnly());
            } else {
                Files.createFile(file);
            }
            MessageDigest digest = sha256();
            long size;
            try (DigestInputStream hashed = new DigestInputStream(in, digest);
                    OutputStream out = Files.newOutputStream(file, StandardOpenOption.WRITE)) {
                size = hashed.transferTo(out);
            }
            String read = HexFormat.of().formatHex(digest.digest());
            if (!read.equals(expected)) {
                throw new ExtractionException("the jar's " + name + " has SHA-256 " + read + ", and its "
                        + name + ".sha256 says " + expected + ": the jar is damaged; use an intact one");
            }
            // What is on disk, which is what will be loaded.
            String written = hash(file);
            long length = Files.size(file);
            if (length != size || !written.equals(expected)) {
                throw new ExtractionException("the cleanrules library extracted to " + file + " has " + length
                        + " bytes and SHA-256 " + written + ", not the jar's " + size + " bytes and " + expected
                        + ": something changed it; set the system property " + TMPDIR
                        + " to a private directory");
            }
            return file;
        } catch (ExtractionException failed) {
            delete(file, directory);
            throw failed;
        } catch (IOException | RuntimeException failed) {
            delete(file, directory);
            throw new ExtractionException(
                    "cannot extract the cleanrules library for " + platform + " to " + (directory != null ? directory : root)
                            + ": " + describe(failed) + "; set the system property " + TMPDIR
                            + " to a writable directory",
                    failed);
        }
    }

    /** The SHA-256 that {@code <name>.sha256} gives, in lower-case hexadecimal. */
    private static String checksum(Resources resources, String name) throws IOException {
        try (InputStream in = resources.open(name + ".sha256")) {
            if (in == null) {
                throw new ExtractionException("the jar has " + name + " but not " + name
                        + ".sha256, its checksum: the jar is incomplete; use one built by the cleanrules build");
            }
            String text = new String(in.readNBytes(4096), StandardCharsets.US_ASCII).strip();
            String first = text.split("\\s+", 2)[0].toLowerCase(java.util.Locale.ROOT);
            if (!first.matches("[0-9a-f]{64}")) {
                throw new ExtractionException("the jar's " + name + ".sha256 holds no SHA-256 (" + abbreviate(text)
                        + "): the jar is damaged; use an intact one");
            }
            return first;
        }
    }

    private static FileAttribute<Set<PosixFilePermission>> ownerOnly() {
        return PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------"));
    }

    private static MessageDigest sha256() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException impossible) {
            // Every Java platform has SHA-256 (MessageDigest's own documentation).
            throw new IllegalStateException(impossible);
        }
    }

    /** The SHA-256 of a file, in lower-case hexadecimal. */
    private static String hash(Path file) throws IOException {
        MessageDigest digest = sha256();
        try (InputStream in = new DigestInputStream(Files.newInputStream(file), digest)) {
            in.transferTo(OutputStream.nullOutputStream());
        }
        return HexFormat.of().formatHex(digest.digest());
    }

    private static String describe(Throwable thrown) {
        String message = thrown.getMessage();
        String type = thrown.getClass().getSimpleName();
        return message == null || message.isEmpty() ? type : type + ": " + message;
    }

    private static String abbreviate(String text) {
        return text.length() > 80 ? text.substring(0, 80) + "..." : text;
    }

    /**
     * Delete an extracted library and its directory where the system allows:
     * always when it was not loaded, and when it was, everywhere but on
     * Windows, which cannot delete a loaded library. Never throws.
     *
     * @param loaded whether the library was loaded
     * @param windows whether this is Windows
     */
    public static void release(Path file, boolean loaded, boolean windows) {
        if (!loaded || !windows) {
            delete(file, file.getParent());
        }
    }

    private static void delete(Path file, Path directory) {
        try {
            if (file != null) {
                Files.deleteIfExists(file);
            }
            if (directory != null) {
                Files.deleteIfExists(directory);
            }
        } catch (IOException | RuntimeException leftBehind) {
            // A later JVM on Windows deletes it; elsewhere the system's own
            // clean-up of temporary files does.
        }
    }

    /**
     * Delete the extraction directories under {@code root} that {@code owner}
     * owns, are older than {@code age} at {@code now}, and hold nothing but the
     * library's file. Symbolic links, Windows junctions and other special
     * entries are left alone (a junction reports itself as a directory too).
     * On Windows a library another process still has loaded cannot be
     * deleted, so its directory stays; the age keeps a JVM that is still
     * extracting from losing its file. Never throws.
     *
     * @param owner the owner of the directory this JVM just created (on
     *     Windows an elevated process's files belong to Administrators, so
     *     the owner is read, not assumed)
     */
    public static void sweep(Path root, Instant now, Duration age, UserPrincipal owner) {
        Instant before = now.minus(age);
        List<Path> directories = new ArrayList<>();
        try (DirectoryStream<Path> entries = Files.newDirectoryStream(root, PREFIX + "*")) {
            entries.forEach(directories::add);
        } catch (IOException | RuntimeException cannotList) {
            return;
        }
        for (Path directory : directories) {
            try {
                BasicFileAttributes attributes =
                        Files.readAttributes(directory, BasicFileAttributes.class, NOFOLLOW_LINKS);
                if (!attributes.isDirectory()
                        || attributes.isSymbolicLink()
                        || attributes.isOther()
                        || !attributes.lastModifiedTime().toInstant().isBefore(before)
                        || !owner.equals(Files.getOwner(directory, NOFOLLOW_LINKS))) {
                    continue;
                }
                List<Path> files = new ArrayList<>();
                try (DirectoryStream<Path> entries = Files.newDirectoryStream(directory)) {
                    entries.forEach(files::add);
                }
                boolean ours = true;
                for (Path file : files) {
                    ours = ours
                            && FILES.contains(file.getFileName().toString())
                            && Files.isRegularFile(file, NOFOLLOW_LINKS);
                }
                if (!ours) {
                    continue;
                }
                for (Path file : files) {
                    Files.delete(file);
                }
                Files.delete(directory);
            } catch (IOException | RuntimeException inUse) {
                // Loaded by a running JVM, or not ours to delete: left alone.
            }
        }
    }
}
