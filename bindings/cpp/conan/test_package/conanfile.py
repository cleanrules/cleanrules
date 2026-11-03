"""Build and run a program against the cleanrules package, as a user would."""

import os

from conan import ConanFile
from conan.tools.build import can_run
from conan.tools.cmake import CMake, cmake_layout


class TestPackageConan(ConanFile):
    """A consumer of cleanrules::cleanrules, found with find_package."""

    settings = "os", "arch", "compiler", "build_type"
    generators = "CMakeDeps", "CMakeToolchain"

    def requirements(self):
        """The package under test."""
        self.requires(self.tested_reference_str)

    def layout(self):
        """The usual CMake layout."""
        cmake_layout(self)

    def build(self):
        """Configure and build the consumer."""
        cmake = CMake(self)
        cmake.configure()
        cmake.build()

    def test(self):
        """Run it, with the package's shared library on the loader's path."""
        if can_run(self):
            for name in ("test_package", "test_package_c"):
                self.run(os.path.join(self.cpp.build.bindir, name), env="conanrun")
