#!/usr/bin/env python3
"""Stage DCCodex npm tarballs from a local musl release artifact."""

from __future__ import annotations

import importlib.util
import argparse
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
BUILD_SCRIPT = REPO_ROOT / "codex-cli" / "scripts" / "build_npm_package.py"
INSTALL_NATIVE_DEPS = REPO_ROOT / "codex-cli" / "scripts" / "install_native_deps.py"
DEFAULT_ARTIFACT = (
    REPO_ROOT / "codex-rs" / "dist" / "release" / "dccodex-x86_64-unknown-linux-musl.tar.gz"
)
TARGET_TRIPLE = "x86_64-unknown-linux-musl"

_spec = importlib.util.spec_from_file_location(
    "dccodex_install_native_deps", INSTALL_NATIVE_DEPS
)
if _spec is None or _spec.loader is None:
    raise RuntimeError(f"Unable to load module from {INSTALL_NATIVE_DEPS}")
_install_native_deps = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_install_native_deps)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--release-version",
        required=True,
        help="Version to stage (for example 0.114.0).",
    )
    parser.add_argument(
        "--artifact",
        type=Path,
        default=DEFAULT_ARTIFACT,
        help="Path to the local DCCodex musl release artifact tar.gz.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=REPO_ROOT / "dist" / "npm",
        help="Directory where npm tarballs should be written.",
    )
    parser.add_argument(
        "--skip-rg",
        action="store_true",
        help="Do not vendor ripgrep into the platform package.",
    )
    return parser.parse_args()


def run_command(cmd: list[str]) -> None:
    print("+", " ".join(map(str, cmd)))
    subprocess.run(cmd, cwd=REPO_ROOT, check=True)


def extract_binary(artifact: Path, vendor_root: Path) -> None:
    if not artifact.exists():
        raise RuntimeError(f"Artifact not found: {artifact}")

    target_root = vendor_root / "vendor" / TARGET_TRIPLE / "codex"
    target_root.mkdir(parents=True, exist_ok=True)
    destination = target_root / "codex"

    with tarfile.open(artifact, "r:gz") as archive:
        members = [member for member in archive.getmembers() if member.isfile()]
        if len(members) != 1:
            raise RuntimeError(f"Expected exactly one file in {artifact}, found {len(members)}")
        extracted = archive.extractfile(members[0])
        if extracted is None:
            raise RuntimeError(f"Unable to extract {members[0].name} from {artifact}")
        with extracted, open(destination, "wb") as out:
            shutil.copyfileobj(extracted, out)

    destination.chmod(0o755)


def install_rg(vendor_root: Path) -> None:
    rg_binary = shutil.which("rg")
    if rg_binary is not None:
        target_root = vendor_root / "vendor" / TARGET_TRIPLE / "path"
        target_root.mkdir(parents=True, exist_ok=True)
        destination = target_root / "rg"
        shutil.copy2(rg_binary, destination)
        destination.chmod(0o755)
        return

    _install_native_deps.fetch_rg(
        vendor_root / "vendor",
        targets=[TARGET_TRIPLE],
        manifest_path=_install_native_deps.RG_MANIFEST,
    )


def stage_package(
    package: str,
    version: str,
    output_dir: Path,
    vendor_root: Path | None,
) -> None:
    pack_output = output_dir / f"{package}-npm-{version}.tgz"
    cmd = [
        str(BUILD_SCRIPT),
        "--package",
        package,
        "--release-version",
        version,
        "--pack-output",
        str(pack_output),
    ]
    if vendor_root is not None:
        cmd.extend(["--vendor-src", str(vendor_root / "vendor")])
    run_command(cmd)


def main() -> int:
    args = parse_args()
    artifact = args.artifact.resolve()
    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="dccodex-npm-vendor-") as vendor_root_str:
        vendor_root = Path(vendor_root_str)
        extract_binary(artifact, vendor_root)
        if not args.skip_rg:
            install_rg(vendor_root)

        stage_package("dccodex-linux-x64", args.release_version, output_dir, vendor_root)
        stage_package("dccodex", args.release_version, output_dir, None)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
