// A Java application on io.github.cleanrules:cleanrules: scripts/JarSmoke.java,
// which the run checks loads the native library from the jar.
dependencies {
    "implementation"("io.github.cleanrules:cleanrules:${providers.gradleProperty("cleanrulesVersion").get()}")
}

configure<JavaApplication> {
    mainClass = "JarSmoke"
}
