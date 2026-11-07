"""Check the staged Maven repository of the JVM artifacts against Maven Central's rules.

Usage: python3 check-staging.py [<repository>] [--platforms all|none|<p1>,<p2>...]
                                [--release <version>]

The repository is the one `./gradlew publishAllPublicationsToStagingRepository`
writes (bindings/jvm/build/staging by default). It must hold the two artifacts,
io.github.cleanrules:cleanrules and io.github.cleanrules:cleanrules-scala_3,
and nothing else, each as Maven Central's publishing requirements ask
(https://central.sonatype.org/publish/requirements/):

- coordinates: groupId, artifactId and version as the path says, one version,
  the same for both, which is a release (no SNAPSHOT); with --release, the
  version given, and not the workspace's placeholder 0.0.0 (only a warning
  without it);
- files: the jar, the sources jar, the javadoc jar (the Scaladoc for Scala),
  the POM and Gradle module metadata, and nothing else; each with an .md5 and
  an .sha1 that match it, and its .sha256 and .sha512 (optional for Central)
  must match too when present;
- the POM: modelVersion 4.0.0, the coordinates, packaging jar, name,
  description, project URL, licence (Apache-2.0, with its URL), developer
  (the author, with their site and ORCID), SCM (connection, developer
  connection, URL) and issue management; no repositories of its own; the
  dependencies each artifact must have and no other, each with a fixed
  release version and its scope (the Scala artifact depends on the Java one,
  at the same version, and on the Scala 3 library, both `compile`, as
  Gradle's `api`); no university or affiliation;
- Gradle module metadata: the same coordinates, the four variants (API,
  runtime, sources, javadoc) with Java 25 as `org.gradle.jvm.version`, every
  file it lists present with its size and digests, and the same dependencies
  as the POM; the POM's marker that sends Gradle to it;
- the jars: the licence and NOTICE in each; the classes (and, for Java, the
  native libraries of the platforms given, as check-jar.py checks them: all
  eight by default) in the jar, sources in the sources jar, pages in the
  javadoc jar, and no pages of the internal package;
- signatures (.asc): deferred to M.2, and the check says so; if some file has
  one, every file must.

Standard library only (it loads check-jar.py from its directory); it prints
what it checked, every problem to stderr, and exits with 1 if there is one.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import re
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path
from xml.etree import ElementTree

HERE = Path(__file__).resolve().parent
GROUP = "io.github.cleanrules"
NS = {"m": "http://maven.apache.org/POM/4.0.0"}
DIGESTS = {"md5": True, "sha1": True, "sha256": False, "sha512": False}  # name -> required
PLACEHOLDER = "0.0.0"
JVM_VERSION = 25
AUTHOR = "Jose L. Salmeron"
AUTHOR_URL = "https://joselsalmeron.github.io/"
ORCID = "https://orcid.org/0000-0001-7811-3716"
REPOSITORY = "https://github.com/cleanrules/cleanrules"
FORBIDDEN = re.compile(r"cunef|universi", re.IGNORECASE)


@dataclass(frozen=True)
class Artifact:
    """What one artifact must be."""

    artifact_id: str
    # (groupId, artifactId) -> scope; the version of a dependency of this
    # group must be the artifact's own.
    dependencies: dict[tuple[str, str], str]
    source_suffix: str
    classes: str
    natives: bool = False


ARTIFACTS = [
    Artifact(
        "cleanrules",
        {},
        ".java",
        "io/github/cleanrules/",
        natives=True,
    ),
    Artifact(
        "cleanrules-scala_3",
        {(GROUP, "cleanrules"): "compile", ("org.scala-lang", "scala3-library_3"): "compile"},
        ".scala",
        "io/github/cleanrules/scaladsl/",
    ),
]
VARIANTS = {
    "apiElements": ("library", None, ""),
    "runtimeElements": ("library", None, ""),
    "sourcesElements": ("documentation", "sources", "-sources"),
    "javadocElements": ("documentation", "javadoc", "-javadoc"),
}


def load_check_jar():
    """check-jar.py, next to this script, as a module."""
    spec = importlib.util.spec_from_file_location("check_jar", HERE / "check-jar.py")
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {HERE / 'check-jar.py'}")
    module = sys.modules.get("check_jar")
    if module is None:
        module = importlib.util.module_from_spec(spec)
        # Registered first: its dataclasses look their module up.
        sys.modules["check_jar"] = module
        spec.loader.exec_module(module)
    return module


class Report:
    """What was checked, the warnings and the problems."""

    def __init__(self) -> None:
        self.lines: list[str] = []
        self.warnings: list[str] = []
        self.problems: list[str] = []

    def line(self, text: str) -> None:
        """Something that was checked, to print."""
        self.lines.append(text)

    def warn(self, text: str) -> None:
        """Something to say that does not fail the check."""
        self.warnings.append(text)

    def problem(self, text: str) -> None:
        """Something that fails the check."""
        self.problems.append(text)


def digest(path: Path, name: str) -> str:
    """The hex digest of a file."""
    return hashlib.new(name, path.read_bytes()).hexdigest()


def check_checksums(directory: Path, file: str, report: Report) -> list[str]:
    """Check a file's checksum files; the digests it has."""
    found = []
    for name, required in DIGESTS.items():
        checksum = directory / f"{file}.{name}"
        if not checksum.is_file():
            if required:
                report.problem(f"{file}: no .{name} (Maven Central requires md5 and sha1)")
            continue
        words = checksum.read_text("ascii", errors="replace").split()
        given = words[0].lower() if words else ""
        actual = digest(directory / file, name)
        if given != actual:
            report.problem(f"{file}.{name} says {given or 'nothing'}, the file's is {actual}")
        else:
            found.append(name)
    return found


def text(element: ElementTree.Element | None, path: str) -> str:
    """The stripped text of a child of a POM element, or ''."""
    if element is None:
        return ""
    child = element.find(path, NS)
    return (child.text or "").strip() if child is not None else ""


def is_release(version: str) -> bool:
    """Whether a version is a fixed release one: no SNAPSHOT, no range."""
    return (
        bool(version) and "SNAPSHOT" not in version.upper() and not re.search(r"[\[\](),]", version)
    )


def check_pom(path: Path, spec: Artifact, version: str, has_module: bool, report: Report):
    """Check the POM; its dependencies as {(group, artifact): (version, scope)}."""
    raw = path.read_text("utf-8")
    name = path.name
    try:
        pom = ElementTree.fromstring(raw)
    except ElementTree.ParseError as error:
        report.problem(f"{name}: not XML ({error})")
        return {}
    if pom.tag != "{http://maven.apache.org/POM/4.0.0}project":
        report.problem(f"{name}: the root is {pom.tag}, not a Maven 4.0.0 project")
        return {}
    expected = {
        "modelVersion": "4.0.0",
        "groupId": GROUP,
        "artifactId": spec.artifact_id,
        "version": version,
    }
    for tag, want in expected.items():
        if text(pom, f"m:{tag}") != want:
            report.problem(f"{name}: <{tag}> is {text(pom, f'm:{tag}')!r}, not {want!r}")
    if text(pom, "m:packaging") not in ("", "jar"):
        report.problem(f"{name}: <packaging> is {text(pom, 'm:packaging')!r}, not jar")
    if pom.find("m:parent", NS) is not None:
        report.problem(f"{name}: has a <parent>, which Central would have to resolve")
    for tag in ("name", "description", "url"):
        if not text(pom, f"m:{tag}"):
            report.problem(f"{name}: no <{tag}> (Maven Central requires it)")
    if not text(pom, "m:url").startswith("https://"):
        report.problem(f"{name}: <url> {text(pom, 'm:url')!r} is not an https URL")

    licenses = pom.findall("m:licenses/m:license", NS)
    if not licenses:
        report.problem(f"{name}: no <licenses> (Maven Central requires them)")
    for license_ in licenses:
        if text(
            license_, "m:name"
        ) != "Apache-2.0" or "apache.org/licenses/LICENSE-2.0" not in text(license_, "m:url"):
            report.problem(
                f"{name}: licence {text(license_, 'm:name')!r} at {text(license_, 'm:url')!r},"
                " not Apache-2.0 with its URL"
            )

    developers = pom.findall("m:developers/m:developer", NS)
    if not developers:
        report.problem(f"{name}: no <developers> (Maven Central requires them)")
    for developer in developers:
        if not text(developer, "m:name"):
            report.problem(f"{name}: a <developer> has no <name>")
    author = [d for d in developers if text(d, "m:name") == AUTHOR]
    if not author:
        report.problem(f"{name}: {AUTHOR} is not a <developer>")
    else:
        if text(author[0], "m:url") != AUTHOR_URL:
            report.problem(f"{name}: the author's <url> is not {AUTHOR_URL}")
        if text(author[0], "m:properties/m:orcid") != ORCID:
            report.problem(f"{name}: the author's <properties><orcid> is not {ORCID}")
        if text(author[0], "m:email"):
            report.problem(f"{name}: the author has an <email>; the POM gives their site and ORCID")
    if FORBIDDEN.search(raw):
        report.problem(
            f"{name}: names a university or affiliation; the author is given by site and ORCID"
        )

    scm = pom.find("m:scm", NS)
    for tag in ("connection", "developerConnection", "url"):
        if not text(scm, f"m:{tag}"):
            report.problem(f"{name}: no <scm><{tag}> (Maven Central requires the SCM)")
    if text(scm, "m:url") != REPOSITORY:
        report.problem(f"{name}: <scm><url> is {text(scm, 'm:url')!r}, not {REPOSITORY}")
    if not text(pom, "m:issueManagement/m:url").startswith(f"{REPOSITORY}/issues"):
        report.problem(f"{name}: <issueManagement><url> is not {REPOSITORY}/issues")
    for tag in ("repositories", "pluginRepositories"):
        if pom.find(f"m:{tag}", NS) is not None:
            report.problem(f"{name}: has <{tag}>; dependencies must come from Maven Central")
    marker = "published-with-gradle-metadata" in raw
    if marker != has_module:
        there = {True: "there", False: "missing"}
        report.problem(
            f"{name}: Gradle's metadata marker is {there[marker]}, the .module {there[has_module]}"
        )

    dependencies: dict[tuple[str, str], tuple[str, str]] = {}
    if pom.find("m:dependencyManagement", NS) is not None:
        report.problem(
            f"{name}: has <dependencyManagement>, which Gradle writes for platforms only"
        )
    for dependency in pom.findall("m:dependencies/m:dependency", NS):
        key = (text(dependency, "m:groupId"), text(dependency, "m:artifactId"))
        dep_version = text(dependency, "m:version")
        scope = text(dependency, "m:scope") or "compile"
        dependencies[key] = (dep_version, scope)
        coordinates = f"{key[0]}:{key[1]}:{dep_version}"
        if not is_release(dep_version):
            report.problem(f"{name}: {coordinates} is not a fixed release version")
        if text(dependency, "m:optional") == "true" or text(dependency, "m:classifier"):
            report.problem(f"{name}: {coordinates} is optional or has a classifier")
        if key not in spec.dependencies:
            report.problem(f"{name}: depends on {coordinates} ({scope}), which it must not")
            continue
        if scope != spec.dependencies[key]:
            report.problem(f"{name}: {coordinates} has scope {scope}, not {spec.dependencies[key]}")
        if key[0] == GROUP and dep_version != version:
            report.problem(f"{name}: {coordinates} is not at the artifact's version {version}")
    for key in spec.dependencies:
        if key not in dependencies:
            report.problem(f"{name}: lacks the dependency {key[0]}:{key[1]}")
    return dependencies


def check_module(path: Path, spec: Artifact, version: str, pom_deps, report: Report) -> None:
    """Check Gradle module metadata against the files and the POM."""
    name = path.name
    try:
        module = json.loads(path.read_text("utf-8"))
    except (ValueError, UnicodeDecodeError) as error:
        report.problem(f"{name}: not JSON ({error})")
        return
    if module.get("formatVersion") != "1.1":
        report.problem(f"{name}: formatVersion {module.get('formatVersion')!r}, not 1.1")
    component = module.get("component", {})
    want = {"group": GROUP, "module": spec.artifact_id, "version": version}
    if {key: component.get(key) for key in want} != want:
        report.problem(f"{name}: component {component}, not {want}")
    variants = {variant.get("name"): variant for variant in module.get("variants", [])}
    if set(variants) != set(VARIANTS):
        report.problem(f"{name}: variants {sorted(variants)}, not {sorted(VARIANTS)}")
    compile_deps = {key: value for key, value in pom_deps.items() if value[1] == "compile"}
    for variant_name, (category, docs, suffix) in VARIANTS.items():
        variant = variants.get(variant_name)
        if variant is None:
            continue
        attributes = variant.get("attributes", {})
        if attributes.get("org.gradle.category") != category:
            report.problem(f"{name}: {variant_name} is not of category {category}")
        if docs is not None and attributes.get("org.gradle.docstype") != docs:
            report.problem(f"{name}: {variant_name} is not of docstype {docs}")
        if docs is None and attributes.get("org.gradle.jvm.version") != JVM_VERSION:
            report.problem(
                f"{name}: {variant_name} asks for Java {attributes.get('org.gradle.jvm.version')},"
                f" not {JVM_VERSION}"
            )
        files = variant.get("files", [])
        want_file = f"{spec.artifact_id}-{version}{suffix}.jar"
        if [entry.get("name") for entry in files] != [want_file]:
            report.problem(
                f"{name}: {variant_name} lists {[e.get('name') for e in files]}, not {want_file}"
            )
        for entry in files:
            file = path.parent / str(entry.get("url"))
            if not file.is_file():
                report.problem(
                    f"{name}: {variant_name} lists {entry.get('url')}, which is not there"
                )
                continue
            if entry.get("size") != file.stat().st_size:
                report.problem(
                    f"{name}: {entry.get('url')} has size {file.stat().st_size},"
                    f" not {entry.get('size')}"
                )
            for algorithm in ("md5", "sha1", "sha256", "sha512"):
                if entry.get(algorithm) != digest(file, algorithm):
                    report.problem(f"{name}: the {algorithm} of {entry.get('url')} does not match")
        declared = {
            (dep.get("group"), dep.get("module")): dep.get("version", {}).get("requires")
            for dep in variant.get("dependencies", [])
        }
        if docs is not None:
            if declared:
                report.problem(f"{name}: {variant_name} has dependencies")
            continue
        expected = compile_deps if variant_name == "apiElements" else pom_deps
        if declared != {key: value[0] for key, value in expected.items()}:
            report.problem(f"{name}: {variant_name} depends on {declared}, the POM on {expected}")


def check_jars(directory: Path, spec: Artifact, version: str, platforms, report: Report) -> None:
    """Check what each jar holds."""
    base = f"{spec.artifact_id}-{version}"
    for suffix in ("", "-sources", "-javadoc"):
        file = directory / f"{base}{suffix}.jar"
        if not file.is_file():
            continue
        try:
            with zipfile.ZipFile(file) as jar:
                names = jar.namelist()
                broken = jar.testzip()
                manifest = (
                    jar.read("META-INF/MANIFEST.MF").decode("utf-8")
                    if "META-INF/MANIFEST.MF" in names
                    else ""
                )
        except zipfile.BadZipFile as error:
            report.problem(f"{file.name}: not a zip ({error})")
            continue
        if broken:
            report.problem(f"{file.name}: {broken} is damaged")
        for required in ("META-INF/MANIFEST.MF", "META-INF/LICENSE", "META-INF/NOTICE"):
            if required not in names:
                report.problem(f"{file.name}: lacks {required}")
        if suffix == "":
            classes = [n for n in names if n.startswith(spec.classes) and n.endswith(".class")]
            if not classes:
                report.problem(f"{file.name}: no classes under {spec.classes}")
            if f"Implementation-Version: {version}" not in manifest:
                report.problem(
                    f"{file.name}: the manifest's Implementation-Version is not {version}"
                )
            if not spec.natives and any(n.startswith("native/") for n in names):
                report.problem(f"{file.name}: holds native libraries, which only the Java jar has")
            report.line(f"  {file.name}: {len(classes)} classes")
        elif suffix == "-sources":
            sources = [n for n in names if n.endswith(spec.source_suffix)]
            if not sources:
                report.problem(f"{file.name}: no {spec.source_suffix} sources")
            if any(n.endswith(".class") for n in names):
                report.problem(f"{file.name}: holds classes")
            report.line(f"  {file.name}: {len(sources)} {spec.source_suffix} files")
        else:
            if "index.html" not in names:
                report.problem(f"{file.name}: no index.html")
            # The internal packages (io.github.cleanrules.internal, scaladsl.internal).
            internal = [n for n in names if "internal" in n.split("/")]
            if internal:
                report.problem(f"{file.name}: documents the internal package ({internal[0]}...)")
            pages = [n for n in names if n.endswith(".html")]
            report.line(f"  {file.name}: {len(pages)} pages")
    if spec.natives and (directory / f"{base}.jar").is_file():
        lines, problems = load_check_jar().check_jar(str(directory / f"{base}.jar"), platforms)
        for line in lines:
            report.line(f"  {line}")
        for problem in problems:
            report.problem(f"{base}.jar: {problem}")


def check_artifact(root: Path, spec: Artifact, platforms, report: Report) -> str | None:
    """Check one artifact's directory; its version."""
    directory = root / Path(*GROUP.split(".")) / spec.artifact_id
    if not directory.is_dir():
        report.problem(f"{GROUP}:{spec.artifact_id}: not in the repository ({directory})")
        return None
    versions = sorted(p.name for p in directory.iterdir() if p.is_dir())
    others = sorted(
        p.name
        for p in directory.iterdir()
        if p.is_file() and not p.name.startswith("maven-metadata.xml")
    )
    if others:
        report.problem(f"{GROUP}:{spec.artifact_id}: unexpected files {others}")
    if len(versions) != 1:
        report.problem(f"{GROUP}:{spec.artifact_id}: versions {versions}, not exactly one")
        return None
    version = versions[0]
    coordinates = f"{GROUP}:{spec.artifact_id}:{version}"
    report.line(coordinates)
    if not is_release(version):
        report.problem(f"{coordinates}: a SNAPSHOT or a range is not a release version")
    elif not re.fullmatch(r"\d+\.\d+\.\d+(?:[-.][0-9A-Za-z.-]+)?", version):
        report.problem(f"{coordinates}: the version is not MAJOR.MINOR.PATCH[-qualifier]")
    folder = directory / version
    base = f"{spec.artifact_id}-{version}"
    primary = [
        f"{base}.jar",
        f"{base}-sources.jar",
        f"{base}-javadoc.jar",
        f"{base}.pom",
        f"{base}.module",
    ]
    allowed = set()
    for file in primary:
        allowed |= {file, f"{file}.asc"} | {f"{file}.{name}" for name in DIGESTS}
        if not (folder / file).is_file():
            report.problem(f"{coordinates}: lacks {file}")
            continue
        found = check_checksums(folder, file, report)
        report.line(f"  {file}: {(folder / file).stat().st_size} bytes, {' '.join(found)} match")
    extra = sorted(p.name for p in folder.iterdir() if p.name not in allowed)
    if extra:
        report.problem(f"{coordinates}: unexpected files {extra}")
    pom_deps = {}
    if (folder / f"{base}.pom").is_file():
        pom_deps = check_pom(
            folder / f"{base}.pom", spec, version, (folder / f"{base}.module").is_file(), report
        )
        listed = ", ".join(f"{g}:{a}:{v} ({s})" for (g, a), (v, s) in pom_deps.items()) or "none"
        report.line(f"  POM: dependencies {listed}")
    if (folder / f"{base}.module").is_file():
        check_module(folder / f"{base}.module", spec, version, pom_deps, report)
    check_jars(folder, spec, version, platforms, report)
    return version


def check_signatures(root: Path, report: Report) -> None:
    """Signatures are M.2's: none yet, or one for every file."""
    files = [p for p in root.rglob("*") if p.is_file() and "maven-metadata.xml" not in p.name]
    primary = [p for p in files if p.suffix in (".jar", ".pom", ".module")]
    signed = [p for p in primary if p.with_name(p.name + ".asc").is_file()]
    if not signed:
        report.line("signatures: deferred (M.2)")
    elif len(signed) != len(primary):
        unsigned = sorted(p.name for p in primary if p not in signed)
        report.problem(f"some files are signed and others are not: {unsigned}")
    else:
        report.line(f"signatures: {len(signed)} .asc files, not verified here (M.2)")


def main() -> int:
    """Check the repository given on the command line."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("repository", nargs="?", default=str(HERE.parent / "build" / "staging"))
    parser.add_argument(
        "--platforms",
        default="all",
        help="the native libraries the Java jar must hold: all (the default), none, or a list",
    )
    parser.add_argument("--release", metavar="VERSION", help="the version being released")
    arguments = parser.parse_args()
    try:
        platforms = load_check_jar().parse_platforms(arguments.platforms)
    except ValueError as error:
        parser.error(str(error))
    root = Path(arguments.repository)
    if not root.is_dir():
        parser.error(
            f"{root} is not a directory: run ./gradlew publishAllPublicationsToStagingRepository"
        )

    report = Report()
    known = {root / Path(*GROUP.split(".")) / spec.artifact_id for spec in ARTIFACTS}
    strays = sorted(
        str(p.relative_to(root))
        for p in root.rglob("*")
        if p.is_file() and not any(parent in known for parent in p.parents)
    )
    if strays:
        report.problem(f"the repository holds files of other artifacts: {strays}")
    versions = {check_artifact(root, spec, platforms, report) for spec in ARTIFACTS}
    versions.discard(None)
    if len(versions) > 1:
        report.problem(f"the artifacts have different versions: {sorted(versions)}")
    for version in versions:
        if arguments.release is not None and version != arguments.release:
            report.problem(f"the version is {version}, not the release's {arguments.release}")
        if version == PLACEHOLDER:
            message = (
                f"version {version} is the workspace's placeholder: set the release's in Cargo.toml"
            )
            if arguments.release is not None:
                report.problem(message)
            else:
                report.warn(message)
    check_signatures(root, report)

    for line in report.lines:
        print(line)
    for warning in report.warnings:
        print(f"warning: {warning}")
    for problem in report.problems:
        print(problem, file=sys.stderr)
    if not report.problems:
        print(f"{root}: {len(ARTIFACTS)} artifacts, as Maven Central requires (but the signatures)")
    return 1 if report.problems else 0


if __name__ == "__main__":
    sys.exit(main())
