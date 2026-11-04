"""Conan 2 recipe for cleanrules: the C library and the C++ header.

It builds through `bindings/cpp/CMakeLists.txt`, which builds the C ABI
(`crates/ffi`) with cargo, so the build machine needs a Rust toolchain.

    conan create bindings/cpp/conan                  # static
    conan create bindings/cpp/conan -o "&:shared=True"
"""

import os
import re
from typing import ClassVar

from conan import ConanFile
from conan.errors import ConanInvalidConfiguration
from conan.tools.build import cross_building
from conan.tools.cmake import CMake, CMakeToolchain, cmake_layout
from conan.tools.files import copy, load, rmdir, save
from conan.tools.microsoft import is_msvc

required_conan_version = ">=2.0"

# The repository root, seen from this recipe (bindings/cpp/conan).
_ROOT = os.path.join("..", "..", "..")

# What cargo needs to build `-p cleanrules-ffi --locked`: the workspace, whose
# members include the Python and Node bindings' crates (their manifests, build
# scripts and sources, for cargo to load the workspace), and the two crates
# themselves.
_SOURCES = [
    "Cargo.toml",
    "Cargo.lock",
    "LICENSE",
    "NOTICE",
    "crates/core/*",
    "crates/ffi/*",
    "bindings/python/Cargo.toml",
    "bindings/python/src/*",
    "bindings/js/Cargo.toml",
    "bindings/js/build.rs",
    "bindings/js/src/*",
    "bindings/cpp/CMakeLists.txt",
    "bindings/cpp/cmake/*",
    "bindings/cpp/include/*",
]

# Rust targets for the platforms Conan can cross-build to.
_RUST_ARCH = {"armv8": "aarch64", "x86_64": "x86_64", "x86": "i686"}

# Conan's CMakeDeps carries no compile features: the C++ target needs C++17,
# as in the package's own CMake config and vcpkg's.
_CXX17_MODULE = """# The C++ header needs C++17 (added by the Conan package).
if(TARGET cleanrules::cleanrules)
  set_property(TARGET cleanrules::cleanrules APPEND PROPERTY INTERFACE_COMPILE_FEATURES cxx_std_17)
endif()
"""
_CXX17_MODULE_PATH = os.path.join("lib", "cmake", "cleanrules-conan-cxx17.cmake")
_NATIVE_LIBS_PATH = os.path.join("res", "cleanrules-native-libs.txt")


class CleanRulesConan(ConanFile):
    """The C ABI and the header-only C++ layer over it."""

    name = "cleanrules"
    description = (
        "Rules engine with explanations: business rules, decision tables, validation "
        "and guardrails, with one Rust core for every language"
    )
    license = "Apache-2.0"
    author = "Jose L. Salmeron"
    url = "https://github.com/cleanrules/cleanrules"
    homepage = "https://cleanrules.github.io"
    topics = ("rules-engine", "decision-tables", "explainable", "business-rules")
    package_type = "library"
    settings = "os", "arch", "compiler", "build_type"
    # fPIC follows Conan's convention; Rust builds position-independent code
    # whatever its value.
    options: ClassVar = {"shared": [True, False], "fPIC": [True, False]}
    default_options: ClassVar = {"shared": False, "fPIC": True}
    implements = ("auto_shared_fpic",)

    def set_version(self):
        """Take the version of every package from the workspace's Cargo.toml."""
        text = load(self, os.path.join(self.recipe_folder, _ROOT, "Cargo.toml"))
        self.version = re.search(r'^version = "([^"]+)"', text, re.MULTILINE).group(1)

    def export_sources(self):
        """Copy the parts of the repository the build needs, keeping its layout."""
        root = os.path.join(self.recipe_folder, _ROOT)
        for pattern in _SOURCES:
            copy(self, pattern, root, self.export_sources_folder, excludes=("target/*",))

    def layout(self):
        """Build the CMake project in bindings/cpp, inside the exported tree."""
        cmake_layout(self, src_folder="bindings/cpp")

    def package_id(self):
        """One package for every C++ standard and library: the C ABI is the same.

        The C++ header is compiled by the consumer, in C++17 (the build module).
        """
        self.info.settings.rm_safe("compiler.cppstd")
        self.info.settings.rm_safe("compiler.libcxx")

    def _msvc_runtime(self):
        """Whether the compiler uses MSVC's C runtime (msvc, or clang with it)."""
        return is_msvc(self) or bool(self.settings.get_safe("compiler.runtime"))

    def validate(self):
        """Platforms the recipe does not package yet."""
        if self.settings.os in ("iOS", "Android"):
            raise ConanInvalidConfiguration(
                f"cleanrules is not packaged for {self.settings.os} yet: build bindings/cpp with "
                "CMake and CLEANRULES_RUST_TARGET"
            )
        if self.settings.os == "Windows" and self.options.shared and not self._msvc_runtime():
            raise ConanInvalidConfiguration("a shared cleanrules with MinGW is not packaged yet")

    def _rust_target(self):
        """The Rust target triple when cross-building, else None (the host's)."""
        if not cross_building(self):
            return None
        arch = _RUST_ARCH.get(str(self.settings.arch))
        os_name = str(self.settings.os)
        if arch is None:
            raise ConanInvalidConfiguration(
                f"cleanrules has no Rust target for arch={self.settings.arch}"
            )
        if os_name == "Macos":
            return f"{arch}-apple-darwin"
        if os_name == "Linux":
            return f"{arch}-unknown-linux-gnu"
        if os_name == "Windows":
            abi = "msvc" if self._msvc_runtime() else "gnu"
            return f"{arch}-pc-windows-{abi}"
        raise ConanInvalidConfiguration(f"cleanrules has no Rust target for os={os_name}")

    def generate(self):
        """Configure the CMake project: no tests, an install, cargo in the build folder."""
        toolchain = CMakeToolchain(self)
        toolchain.cache_variables["CLEANRULES_SHARED"] = bool(self.options.shared)
        toolchain.cache_variables["CLEANRULES_BUILD_TESTS"] = False
        toolchain.cache_variables["CLEANRULES_INSTALL"] = True
        toolchain.cache_variables["CLEANRULES_CARGO_TARGET_DIR"] = os.path.join(
            self.build_folder, "cargo"
        ).replace("\\", "/")
        if self._msvc_runtime():
            static_crt = self.settings.get_safe("compiler.runtime") == "static"
            toolchain.cache_variables["CLEANRULES_MSVC_STATIC_CRT"] = static_crt
        target = self._rust_target()
        if target:
            toolchain.cache_variables["CLEANRULES_RUST_TARGET"] = target
        toolchain.generate()

    def build(self):
        """Configure, then build: cargo runs as part of the CMake build."""
        cmake = CMake(self)
        cmake.configure()
        cmake.build()

    def package(self):
        """Install, then keep Conan's view of the package.

        The package's own CMake config gives way to CMakeDeps (and a build
        module for C++17), its licenses go in licenses/, and a file records the
        system libraries a static library needs.
        """
        CMake(self).install()
        rmdir(self, os.path.join(self.package_folder, "lib", "cmake"))
        rmdir(self, os.path.join(self.package_folder, "share"))
        save(self, os.path.join(self.package_folder, _CXX17_MODULE_PATH), _CXX17_MODULE)
        for name in ("LICENSE", "NOTICE"):
            copy(
                self,
                name,
                os.path.join(self.source_folder, "..", ".."),
                os.path.join(self.package_folder, "licenses"),
            )
        cache = load(self, os.path.join(self.build_folder, "CMakeCache.txt"))
        match = re.search(r"^CLEANRULES_NATIVE_LIBS:[A-Z]+=(.*)$", cache, re.MULTILINE)
        native = match.group(1).strip() if match and not self.options.shared else ""
        save(self, os.path.join(self.package_folder, _NATIVE_LIBS_PATH), native)

    def package_info(self):
        """Two targets, as the package's own CMake config gives them."""
        self.cpp_info.set_property("cmake_file_name", "cleanrules")
        self.cpp_info.set_property("cmake_config_version_compat", "SameMinorVersion")
        self.cpp_info.set_property("cmake_build_modules", [_CXX17_MODULE_PATH])

        c = self.cpp_info.components["c"]
        c.set_property("cmake_target_name", "cleanrules::c")
        shared_msvc = self.options.shared and self.settings.os == "Windows" and self._msvc_runtime()
        c.libs = ["cleanrules_ffi.dll" if shared_msvc else "cleanrules_ffi"]
        native_path = os.path.join(self.package_folder, _NATIVE_LIBS_PATH)
        native = load(self, native_path) if os.path.exists(native_path) else ""
        for item in filter(None, native.split(";")):
            if item.startswith("-framework "):
                c.frameworks.append(item.split(" ", 1)[1])
            else:
                c.system_libs.append(item)

        cpp = self.cpp_info.components["cleanrules"]
        cpp.set_property("cmake_target_name", "cleanrules::cleanrules")
        cpp.requires = ["c"]
        cpp.libdirs = []
        cpp.bindirs = []
