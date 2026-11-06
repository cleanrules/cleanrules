package io.github.cleanrules.internal;

import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import java.util.function.Supplier;
import java.util.stream.Stream;

/**
 * A platform of the native library: its system, processor and, on Linux, C
 * library, named as the npm packages {@code @cleanrules/node-<platform>} name
 * theirs ({@code darwin-arm64}, {@code linux-x64-gnu}, {@code linux-x64-musl},
 * {@code win32-x64-msvc}...). The jar holds the library of each platform at
 * {@code /native/<name>/<file>}.
 *
 * @param os {@code darwin}, {@code linux}, {@code win32}, or another system's
 *     name in lower case
 * @param arch {@code x64}, {@code arm64}, or another {@code os.arch} in lower case
 * @param abi {@code gnu} or {@code musl} on Linux, {@code msvc} on Windows,
 *     null elsewhere
 */
public record Platform(String os, String arch, String abi) {
    /** The platforms whose library a jar may bundle, in the order of the npm packages. */
    public static final List<String> BUNDLED = List.of(
            "darwin-x64",
            "darwin-arm64",
            "linux-x64-gnu",
            "linux-arm64-gnu",
            "linux-x64-musl",
            "linux-arm64-musl",
            "win32-x64-msvc",
            "win32-arm64-msvc");

    /** The name: {@code <os>-<arch>}, then {@code -<abi>} when there is one. */
    public String name() {
        return abi == null ? os + "-" + arch : os + "-" + arch + "-" + abi;
    }

    /** Whether a jar may bundle the library of this platform. */
    public boolean bundleable() {
        return BUNDLED.contains(name());
    }

    /** The library's file name here, as {@code System.mapLibraryName("cleanrules_ffi")} gives it. */
    public String fileName() {
        return switch (os) {
            case "darwin" -> "lib" + Native.LIBRARY + ".dylib";
            case "win32" -> Native.LIBRARY + ".dll";
            default -> "lib" + Native.LIBRARY + ".so";
        };
    }

    /** Whether this is Windows, where a loaded library cannot be deleted. */
    public boolean windows() {
        return os.equals("win32");
    }

    @Override
    public String toString() {
        return name();
    }

    /** The platform this JVM runs on. */
    public static Platform current() {
        return of(System.getProperty("os.name", ""), System.getProperty("os.arch", ""), Platform::linuxLibc);
    }

    /**
     * The platform of the system properties {@code os.name} and {@code os.arch},
     * asking {@code libc} for the C library on Linux only.
     */
    public static Platform of(String osName, String osArch, Supplier<String> libc) {
        String name = osName.toLowerCase(Locale.ROOT);
        String os;
        if (name.startsWith("mac") || name.startsWith("darwin")) {
            os = "darwin";
        } else if (name.startsWith("windows")) {
            os = "win32";
        } else if (name.startsWith("linux")) {
            os = "linux";
        } else {
            os = name.replaceAll("[^a-z0-9]", "");
        }
        String arch = switch (osArch.toLowerCase(Locale.ROOT)) {
            case "amd64", "x86_64", "x64" -> "x64";
            case "aarch64", "arm64" -> "arm64";
            default -> osArch.toLowerCase(Locale.ROOT).replaceAll("[^a-z0-9_]", "");
        };
        String abi = switch (os) {
            case "linux" -> libc.get();
            case "win32" -> "msvc";
            default -> null;
        };
        return new Platform(os, arch, abi);
    }

    /**
     * The C library of this process on Linux: {@code musl} or {@code gnu}.
     *
     * <p>The library must match the C library the JVM itself runs on, not
     * whatever is installed: a glibc system may hold musl's loader
     * ({@code musl-tools}), and a glibc JDK runs on Alpine through
     * {@code gcompat}, on musl. So the binding asks the process: first the
     * files mapped into it ({@code /proc/self/maps}: musl's
     * {@code ld-musl-<arch>.so.1} or glibc's {@code libc.so.6}), then the
     * program interpreter of the {@code java} launcher (its ELF
     * {@code PT_INTERP}, {@code /lib/ld-musl-...} or {@code ld-linux-...}),
     * for when {@code /proc} cannot be read. Neither spawns a process. glibc
     * is the answer when both are silent.
     */
    static String linuxLibc() {
        try (Stream<String> lines = Files.lines(Path.of("/proc/self/maps"), StandardCharsets.ISO_8859_1)) {
            String found = libcOfMaps(lines);
            if (found != null) {
                return found;
            }
        } catch (IOException | UncheckedIOException | SecurityException cannotRead) {
            // Try the launcher below.
        }
        Path launcher = Path.of(System.getProperty("java.home", ""), "bin", "java");
        try (InputStream in = Files.newInputStream(launcher)) {
            String found = libcOfInterpreter(in.readNBytes(1 << 16));
            if (found != null) {
                return found;
            }
        } catch (IOException | RuntimeException cannotRead) {
            // glibc below.
        }
        return "gnu";
    }

    /**
     * The C library that lines of {@code /proc/<pid>/maps} show mapped:
     * {@code musl}, {@code gnu}, or null when they show neither.
     */
    public static String libcOfMaps(Stream<String> lines) {
        boolean glibc = false;
        for (String line : (Iterable<String>) lines::iterator) {
            int slash = line.lastIndexOf('/');
            if (slash < 0) {
                continue;
            }
            String file = line.substring(slash + 1).strip();
            if (file.startsWith("ld-musl-") || file.startsWith("libc.musl-")) {
                return "musl";
            }
            if (file.equals("libc.so.6") || file.matches("libc-2\\.\\d+\\.so")) {
                glibc = true;
            }
        }
        return glibc ? "gnu" : null;
    }

    /** {@code PT_INTERP}, the type of an ELF program header that names the program interpreter. */
    private static final int PT_INTERP = 3;

    /**
     * The C library whose loader the start of a 64-bit little-endian ELF
     * executable names as its program interpreter: {@code musl},
     * {@code gnu}, or null when it is no such ELF file or names neither.
     */
    public static String libcOfInterpreter(byte[] elf) {
        ByteBuffer bytes = ByteBuffer.wrap(elf).order(ByteOrder.LITTLE_ENDIAN);
        // \x7fELF, 64-bit, little-endian.
        if (elf.length < 64 || bytes.getInt(0) != 0x464c457f || elf[4] != 2 || elf[5] != 1) {
            return null;
        }
        long phoff = bytes.getLong(0x20);
        int phentsize = Short.toUnsignedInt(bytes.getShort(0x36));
        int phnum = Short.toUnsignedInt(bytes.getShort(0x38));
        if (phentsize < 56 || phoff < 0 || phoff > elf.length) {
            return null;
        }
        for (int i = 0; i < phnum; i++) {
            long at = phoff + (long) i * phentsize;
            if (at + 56 > elf.length) {
                return null;
            }
            if (bytes.getInt((int) at) != PT_INTERP) {
                continue;
            }
            long offset = bytes.getLong((int) at + 8);
            long size = bytes.getLong((int) at + 32);
            if (offset < 0 || size < 0 || offset > elf.length || size > elf.length - offset) {
                return null;
            }
            String interpreter = new String(elf, (int) offset, (int) size, StandardCharsets.ISO_8859_1);
            if (interpreter.contains("ld-musl-")) {
                return "musl";
            }
            if (interpreter.contains("ld-linux")) {
                return "gnu";
            }
            return null;
        }
        return null;
    }
}
