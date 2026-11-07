// A Scala 3 application on io.github.cleanrules:cleanrules-scala_3 alone: it
// compiles against the Java API and the Scala 3 library too, which come
// through that artifact's `compile` dependencies.
plugins {
    scala
}

dependencies {
    implementation("io.github.cleanrules:cleanrules-scala_3:${providers.gradleProperty("cleanrulesVersion").get()}")
}

configure<JavaApplication> {
    mainClass = "scalaSmoke"
}
