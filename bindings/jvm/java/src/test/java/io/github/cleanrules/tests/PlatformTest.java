package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import io.github.cleanrules.internal.Platform;
import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.stream.Stream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.CsvSource;

/** Which platform's library the binding takes from the jar: system, processor and C library. */
class PlatformTest {
    @ParameterizedTest
    @CsvSource({
        "Mac OS X, aarch64, , darwin-arm64, libcleanrules_ffi.dylib, true",
        "Mac OS X, x86_64, , darwin-x64, libcleanrules_ffi.dylib, true",
        "Darwin, arm64, , darwin-arm64, libcleanrules_ffi.dylib, true",
        "Linux, amd64, gnu, linux-x64-gnu, libcleanrules_ffi.so, true",
        "Linux, aarch64, gnu, linux-arm64-gnu, libcleanrules_ffi.so, true",
        "Linux, amd64, musl, linux-x64-musl, libcleanrules_ffi.so, true",
        "Linux, aarch64, musl, linux-arm64-musl, libcleanrules_ffi.so, true",
        "Windows 11, amd64, , win32-x64-msvc, cleanrules_ffi.dll, true",
        "Windows Server 2025, aarch64, , win32-arm64-msvc, cleanrules_ffi.dll, true",
        "Linux, riscv64, gnu, linux-riscv64-gnu, libcleanrules_ffi.so, false",
        "Linux, x86, gnu, linux-x86-gnu, libcleanrules_ffi.so, false",
        "Windows 10, x86, , win32-x86-msvc, cleanrules_ffi.dll, false",
        "FreeBSD, amd64, , freebsd-x64, libcleanrules_ffi.so, false",
        "SunOS, sparcv9, , sunos-sparcv9, libcleanrules_ffi.so, false",
    })
    void theSystemAndProcessorNameThePlatform(
            String osName, String osArch, String libc, String name, String file, boolean bundleable) {
        Platform platform = Platform.of(osName, osArch, () -> libc);
        assertEquals(name, platform.name());
        assertEquals(name, platform.toString());
        assertEquals(file, platform.fileName());
        assertEquals(bundleable, platform.bundleable());
        assertEquals(name.startsWith("win32-"), platform.windows());
    }

    @Test
    void theCLibraryIsAskedForOnLinuxOnly() {
        AtomicInteger asked = new AtomicInteger();
        for (String os : List.of("Mac OS X", "Windows 11", "FreeBSD")) {
            Platform.of(os, "aarch64", () -> {
                asked.incrementAndGet();
                return "musl";
            });
        }
        assertEquals(0, asked.get());
        assertEquals("linux-arm64-musl", Platform.of("Linux", "aarch64", () -> {
                    asked.incrementAndGet();
                    return "musl";
                })
                .name());
        assertEquals(1, asked.get());
    }

    @Test
    void thePlatformsAreThoseOfTheNpmPackagesAndTheBuild() throws IOException {
        Path repository = Path.of(System.getProperty("cleanrules.examples")).resolve("../..").normalize();
        try (Stream<Path> packages = Files.list(repository.resolve("bindings/js/npm"))) {
            assertEquals(
                    packages.map(p -> p.getFileName().toString()).sorted().toList(),
                    Platform.BUNDLED.stream().sorted().toList());
        }
        String build = Files.readString(repository.resolve("bindings/jvm/java/build.gradle.kts"));
        for (String platform : Platform.BUNDLED) {
            assertTrue(build.contains("\"" + platform + "\","), platform);
        }
    }

    @Test
    void thisPlatformHasTheLibraryFileNameJavaMapsTo() {
        Platform platform = Platform.current();
        assertEquals(System.mapLibraryName("cleanrules_ffi"), platform.fileName());
        String os = System.getProperty("os.name").toLowerCase(java.util.Locale.ROOT);
        String arch = System.getProperty("os.arch");
        if ((os.startsWith("mac") || os.startsWith("linux") || os.startsWith("windows"))
                && List.of("amd64", "x86_64", "aarch64").contains(arch)) {
            assertTrue(platform.bundleable(), platform.name());
        }
    }

    @Test
    void onLinuxTheMappedFilesAndTheLaunchersInterpreterAgree() throws IOException {
        assumeTrue(System.getProperty("os.name").startsWith("Linux"), "Linux only");
        String mapped;
        try (Stream<String> lines = Files.lines(Path.of("/proc/self/maps"), StandardCharsets.ISO_8859_1)) {
            mapped = Platform.libcOfMaps(lines);
        }
        String interpreter;
        try (InputStream in = Files.newInputStream(Path.of(System.getProperty("java.home"), "bin", "java"))) {
            interpreter = Platform.libcOfInterpreter(in.readNBytes(1 << 16));
        }
        assertEquals(mapped, interpreter);
        assertEquals("linux-" + Platform.current().arch() + "-" + mapped, Platform.current().name());
    }

    @Test
    void theMappedFilesTellMuslFromGlibc() {
        String alpine = """
                55d4c3a00000-55d4c3a01000 r--p 00000000 00:2f 1840     /opt/java/openjdk/bin/java
                7f0a1c000000-7f0a1c021000 rw-p 00000000 00:00 0
                7f0a1d5e2000-7f0a1d5f6000 r--p 00000000 00:2f 1059     /lib/ld-musl-x86_64.so.1
                7ffd2c9b0000-7ffd2c9d1000 rw-p 00000000 00:00 0                          [stack]
                """;
        assertEquals("musl", Platform.libcOfMaps(alpine.lines()));
        String alpineLink = "7f0a1d5e2000-7f0a1d5f6000 r--p 00000000 00:2f 1059 /lib/libc.musl-aarch64.so.1";
        assertEquals("musl", Platform.libcOfMaps(Stream.of(alpineLink)));
        String debian = """
                7f6b1c400000-7f6b1c428000 r--p 00000000 08:01 1574  /usr/lib/x86_64-linux-gnu/libc.so.6
                7f6b1c650000-7f6b1c652000 r--p 00000000 08:01 1570  /usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2
                """;
        assertEquals("gnu", Platform.libcOfMaps(debian.lines()));
        assertEquals("gnu", Platform.libcOfMaps(Stream.of("7f-7f r-xp 0 08:01 1 /lib64/libc-2.28.so")));
        // A glibc JDK on Alpine (gcompat): the process runs on musl.
        String gcompat = """
                7f0a1d000000-7f0a1d001000 r--p 00000000 00:2f 2 /lib/libgcompat.so.0
                7f0a1d5e2000-7f0a1d5f6000 r--p 00000000 00:2f 1059 /lib/ld-musl-x86_64.so.1
                """;
        assertEquals("musl", Platform.libcOfMaps(gcompat.lines()));
        // Neither, or no files at all.
        assertNull(Platform.libcOfMaps(Stream.of("7f-7f rw-p 0 00:00 0 [heap]", "", "7f-7f r--p 0 0:0 1 /lib/libm.so.6")));
        assertNull(Platform.libcOfMaps(Stream.empty()));
        // A file whose name only looks like one.
        assertNull(Platform.libcOfMaps(Stream.of("7f-7f r--p 0 0:0 1 /opt/libc.so.6.bak")));
    }

    /** A 64-bit little-endian ELF header with a PT_LOAD and, when not null, a PT_INTERP naming {@code interpreter}. */
    private static byte[] elf(String interpreter) {
        ByteBuffer bytes = ByteBuffer.allocate(512).order(ByteOrder.LITTLE_ENDIAN);
        bytes.putInt(0, 0x464c457f).put(4, (byte) 2).put(5, (byte) 1).put(6, (byte) 1);
        bytes.putLong(0x20, 64).putShort(0x36, (short) 56).putShort(0x38, (short) (interpreter == null ? 1 : 2));
        bytes.putInt(64, 1); // PT_LOAD
        if (interpreter != null) {
            byte[] text = (interpreter + "\0").getBytes(StandardCharsets.US_ASCII);
            bytes.putInt(120, 3).putLong(128, 300).putLong(152, text.length);
            bytes.put(300, text);
        }
        return bytes.array();
    }

    @Test
    void theLaunchersInterpreterTellsMuslFromGlibc() {
        assertEquals("musl", Platform.libcOfInterpreter(elf("/lib/ld-musl-x86_64.so.1")));
        assertEquals("musl", Platform.libcOfInterpreter(elf("/lib/ld-musl-aarch64.so.1")));
        assertEquals("gnu", Platform.libcOfInterpreter(elf("/lib64/ld-linux-x86-64.so.2")));
        assertEquals("gnu", Platform.libcOfInterpreter(elf("/lib/ld-linux-aarch64.so.1")));
        assertNull(Platform.libcOfInterpreter(elf("/system/bin/linker64")));
        assertNull(Platform.libcOfInterpreter(elf(null)));
    }

    @Test
    void anythingButASoundElfFileTellsNothing() {
        assertNull(Platform.libcOfInterpreter(new byte[0]));
        assertNull(Platform.libcOfInterpreter("#!/bin/sh\nexec java \"$@\"\n".repeat(4).getBytes(StandardCharsets.US_ASCII)));
        byte[] thirtyTwoBits = elf("/lib/ld-musl-x86_64.so.1");
        thirtyTwoBits[4] = 1;
        assertNull(Platform.libcOfInterpreter(thirtyTwoBits));
        byte[] bigEndian = elf("/lib/ld-musl-x86_64.so.1");
        bigEndian[5] = 2;
        assertNull(Platform.libcOfInterpreter(bigEndian));
        // Truncated before the program headers end, or before the interpreter's name.
        assertNull(Platform.libcOfInterpreter(java.util.Arrays.copyOf(elf("/lib/ld-musl-x86_64.so.1"), 150)));
        assertNull(Platform.libcOfInterpreter(java.util.Arrays.copyOf(elf("/lib/ld-musl-x86_64.so.1"), 310)));
        // Offsets and sizes out of the file, also beyond a long's range when added.
        ByteBuffer hugeTable = ByteBuffer.wrap(elf("/lib/ld-musl-x86_64.so.1")).order(ByteOrder.LITTLE_ENDIAN);
        hugeTable.putLong(0x20, Long.MAX_VALUE - 10);
        assertNull(Platform.libcOfInterpreter(hugeTable.array()));
        ByteBuffer hugeName = ByteBuffer.wrap(elf("/lib/ld-musl-x86_64.so.1")).order(ByteOrder.LITTLE_ENDIAN);
        hugeName.putLong(152, Long.MAX_VALUE);
        assertNull(Platform.libcOfInterpreter(hugeName.array()));
        ByteBuffer negative = ByteBuffer.wrap(elf("/lib/ld-musl-x86_64.so.1")).order(ByteOrder.LITTLE_ENDIAN);
        negative.putLong(128, -1);
        assertNull(Platform.libcOfInterpreter(negative.array()));
        ByteBuffer small = ByteBuffer.wrap(elf("/lib/ld-musl-x86_64.so.1")).order(ByteOrder.LITTLE_ENDIAN);
        small.putShort(0x36, (short) 32);
        assertNull(Platform.libcOfInterpreter(small.array()));
        assertFalse(Platform.of("Linux", "amd64", () -> "gnu").windows());
    }
}
