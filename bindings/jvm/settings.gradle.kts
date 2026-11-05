// The JVM bindings of cleanrules, one build: the Java binding (`java`, artifact
// io.github.cleanrules:cleanrules) and, from task G.2, the Scala 3 layer on top
// of it (`scala`, artifact io.github.cleanrules:cleanrules-scala_3).
rootProject.name = "cleanrules-jvm"

include("java")
