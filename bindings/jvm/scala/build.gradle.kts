// The Scala 3 layer: an idiomatic Scala API over the Java binding's
// (`:java`, package io.github.cleanrules), for Scala 3.3 LTS. Gradle's own
// `scala` plugin; the Scala 3 library at runtime and MUnit for the tests only.

plugins {
    scala
    `java-library`
}

/** The repository's root: this build lives in bindings/jvm. */
val repository: File = rootDir.resolve("../..").normalize()

// One version for every package: the Java binding's (the workspace's).
evaluationDependsOn(":java")
group = "io.github.cleanrules"
version = project(":java").version

base {
    archivesName = "cleanrules-scala_3"
}

repositories {
    mavenCentral()
}

// The Scala example of docs/examples is compiled with the tests, which run it.
sourceSets.test {
    scala {
        srcDir(repository.resolve("docs/examples"))
        include("**/*.scala")
    }
}

dependencies {
    api(project(":java"))
    api("org.scala-lang:scala3-library_3:3.3.9")
    testImplementation("org.scalameta:munit_3:1.3.6")
}

tasks.withType<ScalaCompile>().configureEach {
    scalaCompileOptions.encoding = "UTF-8"
    scalaCompileOptions.additionalParameters = listOf(
        "-release", "25",
        "-feature",
        "-Wunused:all",
        "-Wvalue-discard",
        "-Werror",
    )
}

tasks.jar {
    // The licence and the NOTICE that every redistribution keeps.
    into("META-INF") {
        from(repository.resolve("LICENSE"), repository.resolve("NOTICE"))
    }
    manifest {
        attributes(
            "Implementation-Title" to "cleanrules-scala_3",
            "Implementation-Version" to project.version,
        )
    }
}

// The native library, as the Java binding's tests find it (see
// java/build.gradle.kts): the C ABI that cargo builds, or the file that
// `-Pcleanrules.library.path=<file>` names.
val givenLibrary: Provider<String> = providers.gradleProperty("cleanrules.library.path")
val cargoTarget: String =
    providers.environmentVariable("CARGO_TARGET_DIR").orNull ?: repository.resolve("target").path
val cargoLibrary: File = File(cargoTarget).resolve("debug").resolve(System.mapLibraryName("cleanrules_ffi"))

tasks.test {
    dependsOn(":java:cargoBuild")
    // MUnit runs on JUnit 4.
    useJUnit()
    val library = givenLibrary.orElse(cargoLibrary.path)
    inputs.file(library).withPropertyName("library")
    systemProperty("cleanrules.library.path", library.get())
    systemProperty("cleanrules.examples", repository.resolve("docs/examples").path)
    inputs.dir(repository.resolve("docs/examples")).withPropertyName("examples")
    jvmArgs("--enable-native-access=ALL-UNNAMED", "--illegal-native-access=deny")
    testLogging {
        events("failed", "skipped")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}

tasks.check {
    dependsOn(tasks.scaladoc)
}
