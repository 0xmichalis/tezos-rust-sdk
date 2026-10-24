"""Validate the shared SDK version before tagging or publishing a release."""

import argparse
from pathlib import Path
import re
import tomllib


def sdk_version(root: Path, tag: str | None = None) -> str:
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    version = workspace["package"]["version"]
    number = r"(?:0|[1-9][0-9]*)"
    pattern = rf"{number}\.{number}\.{number}(?:-[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*)?"
    if not isinstance(version, str) or not re.fullmatch(pattern, version):
        raise ValueError("SDK version must be a semantic version")
    for member in workspace["members"]:
        package = tomllib.loads((root / member / "Cargo.toml").read_text())["package"]
        if package["version"] not in (version, {"workspace": True}):
            raise ValueError(f"{member} does not match SDK version {version}")
    if tag is not None and tag != f"v{version}":
        raise ValueError(f"release tag must be v{version}")
    return version


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", help="Require this tag to match the SDK version")
    arguments = parser.parse_args()
    try:
        print(sdk_version(Path(__file__).resolve().parents[2], arguments.tag))
    except (KeyError, OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
