from importlib.metadata import version

import rulepy


def test_version_matches_installed_metadata() -> None:
    assert rulepy.__version__ == version("rulepy")


def test_public_api_is_declared() -> None:
    assert "__version__" in rulepy.__all__
