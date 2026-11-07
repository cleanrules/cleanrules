#!/usr/bin/env bash
# Resolves the staged JVM artifacts from a clean project and runs them (task
# G.3b; CI: the resolve jobs of jvm-natives.yml), as a user would:
#
#   bash scripts/resolve-staging.sh <staged repository> <new directory>
#
# It copies scripts/consumer (a Java and a Scala 3 application), JarSmoke.java
# and the Gradle wrapper into <new directory>/module and <new directory>/pom,
# outside the build, checks the wrapper, and runs both: the first resolves
# with Gradle module metadata, the second with the POMs only, as Maven does.
# Each checks that cleanrules came from the staged repository (and never from
# Maven Central), with its sources and javadoc jars, and runs the smoke
# programs, which load the native library from the jar. The Scala 3 library
# and compiler come from Maven Central.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $0 <staged repository> <new directory>" >&2
  exit 2
fi
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$1" && pwd)
# The path as the JVM reads it (on Windows, D:/a/... rather than /d/a/...).
staging=$(cd "$1" && { pwd -W 2> /dev/null || pwd; })
if [ -e "$2" ]; then
  echo "$2 exists: give a new directory" >&2
  exit 2
fi
# The one version (a directory; maven-metadata.xml sits next to it).
shopt -s nullglob
versions=()
for directory in "$root"/io/github/cleanrules/cleanrules/*/; do
  versions+=("$(basename "$directory")")
done
if [ "${#versions[@]}" != 1 ]; then
  echo "$1 holds the versions ${versions[*]} of io.github.cleanrules:cleanrules, not one" >&2
  exit 1
fi
version=${versions[0]}

for mode in module pom; do
  work="$2/$mode"
  mkdir -p "$work/java-app/src/main/java" "$work/gradle"
  cp -R "$here/consumer/." "$work/"
  cp "$here/JarSmoke.java" "$work/java-app/src/main/java/"
  cp "$here/../gradlew" "$here/../gradlew.bat" "$work/"
  cp -R "$here/../gradle/wrapper" "$work/gradle/"
  (
    cd "$work"
    bash "$here/check-wrapper.sh"
    # No cleanrules.library.path or CLEANRULES_LIBRARY_PATH: the library
    # must come from the jar.
    env -u CLEANRULES_LIBRARY_PATH ./gradlew --no-daemon --console=plain \
      "-Pstaging=$staging" "-PcleanrulesVersion=$version" "-PpomOnly=$([ "$mode" = pom ] && echo true || echo false)" \
      run
  )
done
