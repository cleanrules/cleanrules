// A clean project that uses the staged JVM artifacts as a user would (task
// G.3b): scripts/resolve-staging.sh copies it out of the repository, with the
// Gradle wrapper and JarSmoke.java, and runs it. `java-app` depends on
// io.github.cleanrules:cleanrules only; `scala-app` on
// io.github.cleanrules:cleanrules-scala_3 only, and reaches the Java artifact
// and the Scala 3 library through its dependencies.
//
//   -Pstaging=<dir>              the staged repository (required)
//   -PcleanrulesVersion=<v>      the version to resolve (required)
//   -PpomOnly=true               read the POMs only, as Maven does, and not
//                                Gradle module metadata
rootProject.name = "cleanrules-consumer"

include("java-app", "scala-app")

val staging: String =
    providers.gradleProperty("staging").orNull ?: error("give the staged repository with -Pstaging=<dir>")
val pomOnly: Boolean = providers.gradleProperty("pomOnly").orNull == "true"

dependencyResolutionManagement {
    repositoriesMode = RepositoriesMode.FAIL_ON_PROJECT_REPOS
    repositories {
        // cleanrules from the staged repository only, and never from Maven
        // Central (where nothing is published yet); the rest (the Scala 3
        // library and compiler) from Maven Central.
        exclusiveContent {
            forRepository {
                maven {
                    name = "staging"
                    url = File(staging).absoluteFile.toURI()
                    if (pomOnly) {
                        metadataSources {
                            mavenPom()
                            ignoreGradleMetadataRedirection()
                        }
                    }
                }
            }
            filter {
                includeGroup("io.github.cleanrules")
            }
        }
        mavenCentral()
    }
}
