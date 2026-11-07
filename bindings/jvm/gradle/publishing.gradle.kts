// What both JVM artifacts share to be published (task G.3b), applied by
// java/build.gradle.kts and scala/build.gradle.kts after they set their
// `base.archivesName` (the artifactId), `description` and the extra property
// `publishedName`. Gradle's own `maven-publish` plugin, no third-party one:
// the artifact (`components["java"]`: the jar, the sources and javadoc jars,
// the POM and Gradle module metadata) goes to a local repository,
// bindings/jvm/build/staging, never to a remote one. Signing and uploading to
// Maven Central are task M.2; scripts/check-staging.py checks the staged
// repository against Central's requirements.

apply(plugin = "maven-publish")

val publishedArtifactId: String = the<BasePluginExtension>().archivesName.get()
val stagingDir: Provider<Directory> = rootProject.layout.buildDirectory.dir("staging")

configure<PublishingExtension> {
    publications {
        register<MavenPublication>("maven") {
            from(components["java"])
            artifactId = publishedArtifactId
            pom {
                name = project.extra["publishedName"] as String
                description = project.description
                url = "https://cleanrules.github.io"
                inceptionYear = "2026"
                licenses {
                    license {
                        name = "Apache-2.0"
                        url = "https://www.apache.org/licenses/LICENSE-2.0.txt"
                        distribution = "repo"
                    }
                }
                developers {
                    developer {
                        id = "joselsalmeron"
                        name = "Jose L. Salmeron"
                        url = "https://joselsalmeron.github.io/"
                        roles = listOf("architect", "developer")
                        properties = mapOf("orcid" to "https://orcid.org/0000-0001-7811-3716")
                    }
                }
                scm {
                    connection = "scm:git:https://github.com/cleanrules/cleanrules.git"
                    developerConnection = "scm:git:ssh://git@github.com/cleanrules/cleanrules.git"
                    url = "https://github.com/cleanrules/cleanrules"
                }
                issueManagement {
                    system = "GitHub Issues"
                    url = "https://github.com/cleanrules/cleanrules/issues"
                }
            }
        }
    }
    repositories {
        maven {
            name = "staging"
            url = uri(stagingDir)
        }
    }
}

// Each publication replaces its own directory of the staging repository, so
// that no other version or stale file is left next to it.
val cleanStaging = tasks.register<Delete>("cleanStaging") {
    description = "Deletes this artifact's directory of the local staging repository."
    group = "publishing"
    delete(stagingDir.map { it.dir("io/github/cleanrules/$publishedArtifactId") })
}
tasks.withType<PublishToMavenRepository>().configureEach {
    if (name == "publishMavenPublicationToStagingRepository") {
        dependsOn(cleanStaging)
    }
}

// The licence and the NOTICE in every jar, the sources and docs ones too.
tasks.withType<Jar>().configureEach {
    if (name != "jar") {
        into("META-INF") {
            from(rootDir.resolve("../../LICENSE"), rootDir.resolve("../../NOTICE"))
        }
    }
}
