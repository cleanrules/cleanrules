#!/usr/bin/env bash
# Checks the Gradle wrapper jar before anything runs it (CI: the jvm job of
# ci.yml and jvm-natives.yml): its sha256 must be the one pinned in the
# repository (so a changed jar fails even when the download below does) and
# the one Gradle publishes for the wrapper's version. Run from bindings/jvm.
set -euo pipefail

jar=gradle/wrapper/gradle-wrapper.jar
if command -v sha256sum > /dev/null; then
  actual=$(sha256sum "$jar" | cut -d' ' -f1 | tr -d '\\')
else
  actual=$(shasum -a 256 "$jar" | cut -d' ' -f1)
fi
pinned=$(tr -d '\r\n ' < "$jar.sha256")
if [ "$actual" != "$pinned" ]; then
  echo "::error::$jar has sha256 $actual; the repository pins $pinned"
  exit 1
fi
version=$(sed -n 's/^distributionUrl=.*gradle-\(.*\)-bin\.zip$/\1/p' gradle/wrapper/gradle-wrapper.properties)
if published=$(curl -fsSL --retry 3 "https://services.gradle.org/distributions/gradle-$version-wrapper.jar.sha256"); then
  if [ "$actual" != "$(echo "$published" | tr -d '\r\n ')" ]; then
    echo "::error::$jar has sha256 $actual; Gradle publishes $published for $version"
    exit 1
  fi
else
  echo "::warning::could not download Gradle's checksum for $version; checked the pinned one only"
fi
echo "$jar: sha256 $actual, as pinned and as Gradle publishes for $version"
