package io.github.cleanrules;

import io.github.cleanrules.internal.Native;
import java.nio.file.Path;
import java.util.Optional;

/**
 * The library itself: its versions, and where it was loaded from.
 *
 * <p>The native library ({@code cleanrules_ffi}, the C ABI) is loaded the
 * first time the binding needs it, from the path of the system property
 * {@code cleanrules.library.path} or the environment variable
 * {@code CLEANRULES_LIBRARY_PATH} (a file, or a directory that holds
 * {@code System.mapLibraryName("cleanrules_ffi")}), else from
 * {@code java.library.path}, else by the system's dynamic loader. A library
 * that cannot be found, or whose ABI or format version is not this binding's,
 * is an {@link UnsatisfiedLinkError} that says why.
 */
public final class CleanRules {
    private CleanRules() {}

    /** The version of the JSON format this binding reads and writes ({@code "version"} in documents). */
    public static final int FORMAT_VERSION = 1;

    /** The version of the C ABI this binding is written for ({@code CR_ABI_VERSION}). */
    public static final int ABI_VERSION = Native.ABI_VERSION;

    /**
     * The native library's version, such as {@code 0.1.0}.
     *
     * @return the version
     * @throws UnsatisfiedLinkError if the library cannot be loaded
     */
    public static String version() {
        Native.ensureLoaded();
        return Native.version();
    }

    /**
     * Load the native library now, so that a problem shows at start-up
     * rather than at the first evaluation.
     *
     * @throws UnsatisfiedLinkError if the library cannot be loaded, or its
     *     ABI or format version is not this binding's
     */
    public static void load() {
        Native.ensureLoaded();
    }

    /**
     * The file the native library was loaded from, when the system property
     * or the environment variable gave it.
     *
     * @return the path, or empty when it was found on {@code java.library.path}
     *     or by the system's loader
     * @throws UnsatisfiedLinkError if the library cannot be loaded
     */
    public static Optional<Path> libraryPath() {
        Native.ensureLoaded();
        return Optional.ofNullable(Native.loadedFrom());
    }
}
