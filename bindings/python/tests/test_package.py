"""The package imports, reports its versions and supports free threading."""

import importlib.metadata
import subprocess
import sys
import sysconfig

import cleanrules


def test_versions_come_from_the_core() -> None:
    assert cleanrules.__version__ == importlib.metadata.version("cleanrules")
    assert cleanrules.FORMAT_VERSION == 1


def test_the_native_module_lives_inside_the_package() -> None:
    from cleanrules import _cleanrules

    assert _cleanrules.__name__ == "cleanrules._cleanrules"


def test_importing_keeps_the_gil_disabled_on_free_threaded_python() -> None:
    if not sysconfig.get_config_var("Py_GIL_DISABLED"):
        return
    # A fresh interpreter: importing a module that needs the GIL re-enables it
    # and warns, which the flag turns into an error.
    code = "import cleanrules, sys; assert not sys._is_gil_enabled()"
    subprocess.run([sys.executable, "-W", "error", "-c", code], check=True)
