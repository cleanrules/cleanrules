// What both applications check (task G.3b): that they resolved the cleanrules
// artifacts they should, from the staged repository and nowhere else, with
// their sources and javadoc jars; then `run` runs each smoke program, which
// loads the native library from the jar.

subprojects {
    apply(plugin = "application")

    val cleanrulesVersion: String =
        providers.gradleProperty("cleanrulesVersion").orNull
            ?: error("give the version to resolve with -PcleanrulesVersion=<v>")
    val staging: File = File(providers.gradleProperty("staging").get()).canonicalFile
    val pomOnly: Boolean = providers.gradleProperty("pomOnly").orNull == "true"

    tasks.withType<JavaCompile>().configureEach {
        options.release = 25
    }

    configure<JavaApplication> {
        // The binding calls restricted methods (the FFM linker) from the class
        // path; any other illegal native access fails.
        applicationDefaultJvmArgs = listOf("--enable-native-access=ALL-UNNAMED", "--illegal-native-access=deny")
    }

    // The artifacts each application must get: the one it depends on, and
    // the Java one through the Scala one.
    val expected: List<String> =
        if (name == "scala-app") listOf("cleanrules", "cleanrules-scala_3") else listOf("cleanrules")
    val runtime = configurations.named("runtimeClasspath")
    val ours: (ComponentIdentifier) -> Boolean = { id ->
        id is ModuleComponentIdentifier && id.group == "io.github.cleanrules"
    }
    val jars = runtime.map { configuration ->
        configuration.incoming.artifactView { componentFilter(ours) }.artifacts
    }
    // The documentation variants: from Gradle module metadata, or derived by
    // Gradle from the POM and the classifier when it reads the POM only.
    val docs = listOf(DocsType.SOURCES, DocsType.JAVADOC).associateWith { type ->
        runtime.map { configuration ->
            configuration.incoming.artifactView {
                withVariantReselection()
                componentFilter(ours)
                attributes {
                    attribute(Category.CATEGORY_ATTRIBUTE, objects.named(Category.DOCUMENTATION))
                    attribute(DocsType.DOCS_TYPE_ATTRIBUTE, objects.named(type))
                    attribute(Bundling.BUNDLING_ATTRIBUTE, objects.named(Bundling.EXTERNAL))
                }
            }.artifacts
        }
    }

    val verifyResolution = tasks.register("verifyResolution") {
        description = "Checks that the cleanrules artifacts came from the staged repository, with their docs."
        group = "verification"
        doLast {
            val mode = if (pomOnly) "POM only" else "Gradle module metadata"
            // The variant each artifact came from: one that Gradle module
            // metadata declares (the publication's), or one that Gradle
            // derives from the POM.
            fun check(artifacts: ArtifactCollection, suffix: String, published: String) {
                val found = artifacts.artifacts.map { it.file.canonicalFile }
                val names = found.map { it.name }.sorted()
                val want = expected.map { "$it-$cleanrulesVersion$suffix.jar" }.sorted()
                if (names != want) {
                    throw GradleException("$mode: resolved $names, not $want")
                }
                val variants = artifacts.artifacts.map { it.variant.displayName.substringAfterLast(" ") }.toSet()
                if ((variants == setOf(published)) == pomOnly) {
                    throw GradleException("$mode: the artifacts came from the variants $variants")
                }
                for (file in found) {
                    if (!file.toPath().startsWith(staging.toPath())) {
                        throw GradleException("$mode: $file is not from the staged repository $staging")
                    }
                }
                println("$mode: ${names.joinToString()} from $staging (variants ${variants.joinToString()})")
            }
            check(jars.get(), "", "runtimeElements")
            check(docs.getValue(DocsType.SOURCES).get(), "-sources", "sourcesElements")
            check(docs.getValue(DocsType.JAVADOC).get(), "-javadoc", "javadocElements")
        }
    }
    tasks.named("run") {
        dependsOn(verifyResolution)
    }
}
