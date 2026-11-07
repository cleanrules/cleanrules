// The Java binding: hand-written bindings of the C ABI (crates/ffi, header
// crates/ffi/include/cleanrules.h) with the Foreign Function & Memory API, for
// Java 25 or later. No dependencies; JUnit 5 for the tests only.

import java.security.MessageDigest
import java.util.HexFormat

plugins {
    `java-library`
}

/** The repository's root: this build lives in bindings/jvm. */
val repository: File = rootDir.resolve("../..").normalize()

/** The workspace's version (`[workspace.package]` of Cargo.toml), shared by every package. */
val workspaceVersion: String =
    Regex("""(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"""")
        .find(repository.resolve("Cargo.toml").readText())
        ?.groupValues
        ?.get(1)
        ?: error("no version in [workspace.package] of ${repository.resolve("Cargo.toml")}")

group = "io.github.cleanrules"
version = workspaceVersion
description = "Explainable rules engine for Java: one Rust core with the same semantics in every language"
extra["publishedName"] = "cleanrules for Java"

base {
    archivesName = "cleanrules"
}

// The sources and javadoc jars that Maven Central asks for, next to the jar.
java {
    withSourcesJar()
    withJavadocJar()
}

// Publishing to the local staging repository: gradle/publishing.gradle.kts.
apply(from = rootDir.resolve("gradle/publishing.gradle.kts"))

repositories {
    mavenCentral()
}

dependencies {
    testImplementation(platform("org.junit:junit-bom:5.14.4"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

tasks.withType<JavaCompile>().configureEach {
    // Java 25 is the minimum (an LTS with the final FFM API), whatever JDK runs Gradle.
    options.release = 25
    options.encoding = "UTF-8"
    options.compilerArgs.addAll(listOf("-Xlint:all", "-Werror"))
}

tasks.javadoc {
    title = "cleanrules $workspaceVersion API"
    // The pages document the exported packages only (the module's API): the
    // internal package is read from the compiled classes, not documented.
    exclude("io/github/cleanrules/internal/**")
    val classes = sourceSets.main.get().output.classesDirs
    (options as StandardJavadocDocletOptions).apply {
        addStringOption("-patch-module", "io.github.cleanrules=${classes.asPath}")
        encoding = "UTF-8"
        addBooleanOption("Xdoclint:all", true)
        // The internals are not exported, nor linted.
        addBooleanOption("Xdoclint/package:-io.github.cleanrules.internal", true)
        addBooleanOption("Werror", true)
        addBooleanOption("quiet", true)
    }
}

// The native library. The tests load the C ABI that cargo builds
// (target/debug, or $CARGO_TARGET_DIR/debug), or the library that
// `-Pcleanrules.library.path=<file>` names, without cargo.
val givenLibrary: Provider<String> = providers.gradleProperty("cleanrules.library.path")
val cargoTarget: String =
    providers.environmentVariable("CARGO_TARGET_DIR").orNull ?: repository.resolve("target").path
val cargoLibrary: File = File(cargoTarget).resolve("debug").resolve(System.mapLibraryName("cleanrules_ffi"))

// The jar holds the library of each platform it supports at
// /native/<platform>/<file>, with <file>.sha256, and the binding extracts this
// platform's when it loads (internal/Bundled.java). The platforms are those of
// the npm packages, named alike (internal/Platform.java). A local `./gradlew
// jar` bundles the host's library, built by `cargo build --release --target
// <host's triple>` (in target/<triple>/release);
// `-Pcleanrules.natives.dir=<dir>` bundles instead the prebuilt libraries in
// <dir>/<platform>/<file>, without cargo (CI bundles all eight that way:
// .github/workflows/jvm-natives.yml).
val nativePlatforms = listOf(
    "darwin-x64",
    "darwin-arm64",
    "linux-x64-gnu",
    "linux-arm64-gnu",
    "linux-x64-musl",
    "linux-arm64-musl",
    "win32-x64-msvc",
    "win32-arm64-msvc",
)

/** The Rust target of each platform. */
val rustTargets = mapOf(
    "darwin-x64" to "x86_64-apple-darwin",
    "darwin-arm64" to "aarch64-apple-darwin",
    "linux-x64-gnu" to "x86_64-unknown-linux-gnu",
    "linux-arm64-gnu" to "aarch64-unknown-linux-gnu",
    "linux-x64-musl" to "x86_64-unknown-linux-musl",
    "linux-arm64-musl" to "aarch64-unknown-linux-musl",
    "win32-x64-msvc" to "x86_64-pc-windows-msvc",
    "win32-arm64-msvc" to "aarch64-pc-windows-msvc",
)

/** The library's file name on a platform. */
fun nativeFileName(platform: String): String = when {
    platform.startsWith("darwin-") -> "libcleanrules_ffi.dylib"
    platform.startsWith("win32-") -> "cleanrules_ffi.dll"
    else -> "libcleanrules_ffi.so"
}

/**
 * The platform Gradle runs on, named as Platform.java names it (on Linux, musl
 * when the process has musl's loader mapped). A value source, so that reading
 * /proc/self/maps is not an input of the configuration cache.
 */
abstract class HostPlatform : ValueSource<String, ValueSourceParameters.None> {
    override fun obtain(): String {
        val os = System.getProperty("os.name").lowercase()
        val arch = when (val name = System.getProperty("os.arch").lowercase()) {
            "amd64", "x86_64" -> "x64"
            "aarch64", "arm64" -> "arm64"
            else -> name
        }
        return when {
            os.startsWith("mac") -> "darwin-$arch"
            os.startsWith("windows") -> "win32-$arch-msvc"
            os.startsWith("linux") -> {
                val maps = File("/proc/self/maps")
                val musl = maps.canRead() && maps.useLines { lines ->
                    lines.any { line ->
                        val file = line.substringAfterLast('/')
                        file.startsWith("ld-musl-") || file.startsWith("libc.musl-")
                    }
                }
                "linux-$arch-${if (musl) "musl" else "gnu"}"
            }
            else -> "$os-$arch"
        }
    }
}

/** Lays the libraries out as the jar holds them: native/<platform>/<file> and <file>.sha256. */
abstract class BundleNatives : DefaultTask() {
    /** The libraries, each the file of its platform. */
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.ABSOLUTE)
    abstract val libraries: ConfigurableFileCollection

    /** The platform of each library, by its absolute path. */
    @get:Input
    abstract val platformOf: MapProperty<String, String>

    /** The expected file name of each platform's library. */
    @get:Input
    abstract val fileNames: MapProperty<String, String>

    /** How to make a library that is missing. */
    @get:Input
    abstract val hint: Property<String>

    @get:OutputDirectory
    abstract val destination: DirectoryProperty

    @TaskAction
    fun bundle() {
        val out = destination.get().asFile
        out.deleteRecursively()
        for (library in libraries.files) {
            val platform = platformOf.get().getValue(library.absolutePath)
            val name = fileNames.get().getValue(platform)
            if (library.name != name || !library.isFile) {
                val holds = library.parentFile.list()?.sorted() ?: emptyList()
                throw GradleException(
                    "no library for $platform at $library (its directory holds $holds): ${hint.get()}",
                )
            }
            val directory = out.resolve("native/$platform")
            directory.mkdirs()
            library.copyTo(directory.resolve(name))
            val digest = MessageDigest.getInstance("SHA-256").digest(library.readBytes())
            // As sha256sum writes it, so that `sha256sum -c` checks an unzipped jar.
            directory.resolve("$name.sha256").writeText("${HexFormat.of().formatHex(digest)}  $name\n")
        }
    }
}

val nativesDir: Provider<String> = providers.gradleProperty("cleanrules.natives.dir")
val hostPlatform: String = providers.of(HostPlatform::class) {}.get()

/** platform -> library: the prebuilt ones of -Pcleanrules.natives.dir, or the host's release build. */
val bundledLibraries: Map<String, File> =
    if (nativesDir.isPresent) {
        val root = File(nativesDir.get()).absoluteFile
        val found = root.listFiles { file -> file.isDirectory }?.sortedBy { it.name }
            ?: error("-Pcleanrules.natives.dir=$root is not a directory")
        val unknown = found.map { it.name }.filterNot { it in nativePlatforms }
        if (unknown.isNotEmpty()) {
            error("$root holds $unknown, which are not platforms of the jar: $nativePlatforms")
        }
        if (found.isEmpty()) {
            error("$root holds no platform directory (<platform>/<library>, platforms $nativePlatforms)")
        }
        nativePlatforms.filter { platform -> found.any { it.name == platform } }
            .associateWith { root.resolve(it).resolve(nativeFileName(it)) }
    } else {
        if (hostPlatform !in nativePlatforms) {
            logger.warn("cleanrules: the jar bundles no library for $hostPlatform, which is not one of $nativePlatforms")
        }
        nativePlatforms.filter { it == hostPlatform }
            .associateWith {
                File(cargoTarget).resolve(rustTargets.getValue(it)).resolve("release").resolve(nativeFileName(it))
            }
    }

/** The host's Rust target, given explicitly (on Alpine, rustup's default would link musl statically). */
val hostTarget: String? = rustTargets[hostPlatform]

/** The flags for the host's target: on musl, the C library linked dynamically, or Rust builds no shared library. */
val hostRustflags: String =
    listOfNotNull(
        providers.environmentVariable("RUSTFLAGS").orNull?.takeIf { it.isNotBlank() },
        if (hostPlatform.endsWith("-musl")) "-C target-feature=-crt-static" else null,
    ).joinToString(" ")

val cargoBuildRelease = tasks.register<Exec>("cargoBuildRelease") {
    description = "Builds the C ABI (crates/ffi) with cargo in release mode, for the jar."
    group = "build"
    // Not needed when -Pcleanrules.natives.dir gives the libraries.
    enabled = !nativesDir.isPresent && bundledLibraries.isNotEmpty()
    workingDir = repository
    commandLine("cargo", "build", "--release", "-p", "cleanrules-ffi", "--target", hostTarget ?: "unknown")
    if (hostRustflags.isNotEmpty()) {
        environment("RUSTFLAGS", hostRustflags)
    }
}

val bundleNatives = tasks.register<BundleNatives>("bundleNatives") {
    description = "Lays out the native libraries the jar bundles, with their SHA-256."
    group = "build"
    dependsOn(cargoBuildRelease)
    libraries.from(bundledLibraries.values)
    platformOf = bundledLibraries.entries.associate { (platform, file) -> file.absolutePath to platform }
    fileNames = nativePlatforms.associateWith { nativeFileName(it) }
    hint =
        if (nativesDir.isPresent) {
            "put each prebuilt library at <dir>/<platform>/<file>, as the JVM natives workflow does"
        } else {
            "build it with `rustup target add $hostTarget` and `cargo build --release -p cleanrules-ffi --target" +
                " $hostTarget`" + (if (hostPlatform.endsWith("-musl")) " with RUSTFLAGS=\"-C target-feature=-crt-static\"" else "") +
                " (the cargoBuildRelease task), or give prebuilt ones with -Pcleanrules.natives.dir"
        }
    destination = layout.buildDirectory.dir("natives")
}

tasks.jar {
    // The licence and the NOTICE that every redistribution keeps.
    into("META-INF") {
        from(repository.resolve("LICENSE"), repository.resolve("NOTICE"))
    }
    from(bundleNatives)
    manifest {
        attributes(
            "Implementation-Title" to "cleanrules",
            "Implementation-Version" to workspaceVersion,
            // The platforms whose native library the jar holds, under /native.
            "Cleanrules-Native-Platforms" to bundledLibraries.keys.joinToString(" ").ifEmpty { "none" },
        )
    }
}

val cargoBuild = tasks.register<Exec>("cargoBuild") {
    description = "Builds the C ABI (crates/ffi) with cargo, for the tests."
    group = "build"
    // Not needed when -Pcleanrules.library.path gives the library.
    enabled = !givenLibrary.isPresent
    workingDir = repository
    commandLine("cargo", "build", "-p", "cleanrules-ffi")
}

tasks.test {
    dependsOn(cargoBuild)
    useJUnitPlatform()
    val library = givenLibrary.orElse(cargoLibrary.path)
    // The library itself, so that a new build of it runs the tests again.
    inputs.file(library).withPropertyName("library")
    inputs.dir(repository.resolve("conformance/vectors")).withPropertyName("vectors")
    systemProperty("cleanrules.library.path", library.get())
    systemProperty("cleanrules.conformance.vectors", repository.resolve("conformance/vectors").path)
    // The conformance runner's report: the counts per route, the skips with
    // their reasons, the time. An output, so that a cached run restores it.
    val conformanceReport = layout.buildDirectory.file("reports/conformance/conformance.txt")
    outputs.file(conformanceReport).withPropertyName("conformanceReport")
    systemProperty("cleanrules.conformance.report", conformanceReport.get().asFile.path)
    systemProperty("cleanrules.examples", repository.resolve("docs/examples").path)
    // The compiled module, for the test that runs the example on the module path.
    systemProperty("cleanrules.module", sourceSets.main.get().output.classesDirs.asPath)
    inputs.dir(repository.resolve("docs/examples")).withPropertyName("examples")
    // The binding calls restricted methods (the FFM linker): allowed for it,
    // and any other illegal native access fails.
    jvmArgs("--enable-native-access=ALL-UNNAMED", "--illegal-native-access=deny")
    // Java's default thread stack on Linux and Windows (macOS gives 2 MiB), so
    // every platform checks that the core fits in it (spec/json-format.md,
    // *Errors and limits*).
    jvmArgs("-Xss1m")
    testLogging {
        events("failed", "skipped")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}

tasks.check {
    dependsOn(tasks.javadoc)
}
