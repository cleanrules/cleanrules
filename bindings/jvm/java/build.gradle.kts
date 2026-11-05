// The Java binding: hand-written bindings of the C ABI (crates/ffi, header
// crates/ffi/include/cleanrules.h) with the Foreign Function & Memory API, for
// Java 25 or later. No dependencies; JUnit 5 for the tests only.

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

base {
    archivesName = "cleanrules"
}

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
    (options as StandardJavadocDocletOptions).apply {
        encoding = "UTF-8"
        addBooleanOption("Xdoclint:all", true)
        // The internals are not exported, nor linted (Gradle lists the
        // sources, so javadoc still writes their pages).
        addBooleanOption("Xdoclint/package:-io.github.cleanrules.internal", true)
        addBooleanOption("Werror", true)
        addBooleanOption("quiet", true)
    }
}

tasks.jar {
    // The licence and the NOTICE that every redistribution keeps.
    into("META-INF") {
        from(repository.resolve("LICENSE"), repository.resolve("NOTICE"))
    }
    manifest {
        attributes(
            "Implementation-Title" to "cleanrules",
            "Implementation-Version" to workspaceVersion,
        )
    }
}

// The native library. Until task G.1c puts it inside the jar, the tests load
// the C ABI that cargo builds (target/debug, or $CARGO_TARGET_DIR/debug), or
// the library that `-Pcleanrules.library.path=<file>` names, without cargo.
val givenLibrary: Provider<String> = providers.gradleProperty("cleanrules.library.path")
val cargoTarget: String =
    providers.environmentVariable("CARGO_TARGET_DIR").orNull ?: repository.resolve("target").path
val cargoLibrary: File = File(cargoTarget).resolve("debug").resolve(System.mapLibraryName("cleanrules_ffi"))

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
    systemProperty("cleanrules.examples", repository.resolve("docs/examples").path)
    // The compiled module, for the test that runs the example on the module path.
    systemProperty("cleanrules.module", sourceSets.main.get().output.classesDirs.asPath)
    inputs.dir(repository.resolve("docs/examples")).withPropertyName("examples")
    // The binding calls restricted methods (the FFM linker): allowed for it,
    // and any other illegal native access fails.
    jvmArgs("--enable-native-access=ALL-UNNAMED", "--illegal-native-access=deny")
    testLogging {
        events("failed", "skipped")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}

tasks.check {
    dependsOn(tasks.javadoc)
}
