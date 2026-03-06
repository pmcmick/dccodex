#!/usr/bin/env python3
"""Stage one or more Codex npm packages for release."""

from __future__ import annotations

import argparse
import datetime
import importlib.util
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
BUILD_SCRIPT = REPO_ROOT / "codex-cli" / "scripts" / "build_npm_package.py"
INSTALL_NATIVE_DEPS = REPO_ROOT / "codex-cli" / "scripts" / "install_native_deps.py"
WORKFLOW_NAME = ".github/workflows/rust-release.yml"
GITHUB_REPO = "openai/codex"

_SPEC = importlib.util.spec_from_file_location("codex_build_npm_package", BUILD_SCRIPT)
if _SPEC is None or _SPEC.loader is None:
    raise RuntimeError(f"Unable to load module from {BUILD_SCRIPT}")
_BUILD_MODULE = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_BUILD_MODULE)
PACKAGE_NATIVE_COMPONENTS = getattr(_BUILD_MODULE, "PACKAGE_NATIVE_COMPONENTS", {})
PACKAGE_EXPANSIONS = getattr(_BUILD_MODULE, "PACKAGE_EXPANSIONS", {})
CODEX_PLATFORM_PACKAGES = getattr(_BUILD_MODULE, "CODEX_PLATFORM_PACKAGES", {})


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--release-version",
        required=True,
        help="Version to stage (e.g. 0.1.0 or 0.1.0-alpha.1).",
    )
    parser.add_argument(
        "--package",
        dest="packages",
        action="append",
        required=True,
        help="Package name to stage. May be provided multiple times.",
    )
    parser.add_argument(
        "--workflow-url",
        help="Optional workflow URL to reuse for native artifacts.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        help="Directory where npm tarballs should be written (default: dist/npm).",
    )
    parser.add_argument(
        "--keep-staging-dirs",
        action="store_true",
        help="Retain temporary staging directories instead of deleting them.",
    )
    parser.add_argument(
        "--build-metadata-file",
        type=Path,
        default=None,
        help=(
            "Path to write computed build metadata JSON. Defaults to "
            "<output-dir>/build-metadata.json."
        ),
    )
    return parser.parse_args()


def collect_native_components(packages: list[str]) -> set[str]:
    components: set[str] = set()
    for package in packages:
        components.update(PACKAGE_NATIVE_COMPONENTS.get(package, []))
    return components


def expand_packages(packages: list[str]) -> list[str]:
    expanded: list[str] = []
    for package in packages:
        for expanded_package in PACKAGE_EXPANSIONS.get(package, [package]):
            if expanded_package in expanded:
                continue
            expanded.append(expanded_package)
    return expanded


def resolve_release_workflow(version: str) -> dict:
    stdout = subprocess.check_output(
        [
            "gh",
            "run",
            "list",
            "--branch",
            f"rust-v{version}",
            "--json",
            "workflowName,url,headSha",
            "--workflow",
            WORKFLOW_NAME,
            "--jq",
            "first(.[])",
        ],
        cwd=REPO_ROOT,
        text=True,
    )
    workflow = json.loads(stdout or "null")
    if not workflow:
        raise RuntimeError(f"Unable to find rust-release workflow for version {version}.")
    return workflow


def resolve_workflow_url(version: str, override: str | None) -> tuple[str, str | None]:
    if override:
        return override, None

    workflow = resolve_release_workflow(version)
    return workflow["url"], workflow.get("headSha")


def install_native_components(
    workflow_url: str,
    components: set[str],
    vendor_root: Path,
) -> None:
    if not components:
        return

    cmd = [str(INSTALL_NATIVE_DEPS), "--workflow-url", workflow_url]
    for component in sorted(components):
        cmd.extend(["--component", component])
    cmd.append(str(vendor_root))
    run_command(cmd)


def run_command(cmd: list[str]) -> None:
    print("+", " ".join(cmd))
    subprocess.run(cmd, cwd=REPO_ROOT, check=True)


def git_output(args: list[str]) -> str:
    return subprocess.check_output(["git", *args], cwd=REPO_ROOT, text=True).strip()


def git_ref_exists(ref: str) -> bool:
    result = subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", ref],
        cwd=REPO_ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return result.returncode == 0


def compute_build_metadata(release_version: str, packages: list[str]) -> dict:
    head_sha = git_output(["rev-parse", "HEAD"])
    head_sha_short = git_output(["rev-parse", "--short=12", "HEAD"])
    tree_sha_short = git_output(["rev-parse", "--short=12", "HEAD^{tree}"])
    git_describe = git_output(["describe", "--tags", "--always", "--dirty", "--abbrev=12"])
    head_epoch = int(git_output(["show", "-s", "--format=%ct", "HEAD"]))
    head_commit_timestamp_utc = datetime.datetime.fromtimestamp(
        head_epoch, tz=datetime.timezone.utc
    ).strftime("%Y%m%d.%H%M%S")

    release_tag = f"rust-v{release_version}"
    release_tag_exists = git_ref_exists(f"refs/tags/{release_tag}")
    commits_since_release_tag = (
        int(git_output(["rev-list", "--count", f"{release_tag}..HEAD"]))
        if release_tag_exists
        else None
    )

    upstream_main_exists = git_ref_exists("refs/remotes/upstream/main")
    commits_ahead_of_upstream_main = (
        int(git_output(["rev-list", "--count", "upstream/main..HEAD"]))
        if upstream_main_exists
        else None
    )
    commits_behind_upstream_main = (
        int(git_output(["rev-list", "--count", "HEAD..upstream/main"]))
        if upstream_main_exists
        else None
    )

    build_id = f"g{head_sha_short}.{head_commit_timestamp_utc}.t{tree_sha_short}"
    if commits_since_release_tag is not None:
        unique_version = (
            f"{release_version}+{commits_since_release_tag}.{build_id}"
        )
    else:
        unique_version = f"{release_version}+{build_id}"

    generated_at_utc = datetime.datetime.now(tz=datetime.timezone.utc).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )

    return {
        "releaseVersion": release_version,
        "releaseTag": release_tag,
        "releaseTagPresent": release_tag_exists,
        "commitsSinceReleaseTag": commits_since_release_tag,
        "uniqueVersion": unique_version,
        "buildId": build_id,
        "gitDescribe": git_describe,
        "headCommit": {
            "sha": head_sha,
            "shortSha": head_sha_short,
            "treeShortSha": tree_sha_short,
            "committerTimestampUtc": head_commit_timestamp_utc,
        },
        "upstreamMain": {
            "present": upstream_main_exists,
            "commitsAhead": commits_ahead_of_upstream_main,
            "commitsBehind": commits_behind_upstream_main,
        },
        "packages": packages,
        "generatedAtUtc": generated_at_utc,
    }


def write_build_metadata(metadata: dict, output_path: Path) -> Path:
    output_path = output_path.resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with open(output_path, "w", encoding="utf-8") as fh:
        json.dump(metadata, fh, indent=2)
        fh.write("\n")
    return output_path


def tarball_name_for_package(package: str, version: str) -> str:
    if package in CODEX_PLATFORM_PACKAGES:
        platform = package.removeprefix("codex-")
        return f"codex-npm-{platform}-{version}.tgz"
    return f"{package}-npm-{version}.tgz"


def main() -> int:
    args = parse_args()

    output_dir = args.output_dir or (REPO_ROOT / "dist" / "npm")
    output_dir.mkdir(parents=True, exist_ok=True)

    runner_temp = Path(os.environ.get("RUNNER_TEMP", tempfile.gettempdir()))

    packages = expand_packages(list(args.packages))
    native_components = collect_native_components(packages)
    build_metadata = compute_build_metadata(args.release_version, packages)
    metadata_path = write_build_metadata(
        build_metadata,
        args.build_metadata_file
        if args.build_metadata_file is not None
        else output_dir / "build-metadata.json",
    )
    print(f"Computed unique version: {build_metadata['uniqueVersion']}")
    print(f"Computed build id: {build_metadata['buildId']}")
    print(f"Wrote build metadata to {metadata_path}")

    vendor_temp_root: Path | None = None
    vendor_src: Path | None = None
    resolved_head_sha: str | None = None

    final_messages = []

    try:
        if native_components:
            workflow_url, resolved_head_sha = resolve_workflow_url(
                args.release_version, args.workflow_url
            )
            vendor_temp_root = Path(tempfile.mkdtemp(prefix="npm-native-", dir=runner_temp))
            install_native_components(workflow_url, native_components, vendor_temp_root)
            vendor_src = vendor_temp_root / "vendor"

        if resolved_head_sha:
            print(f"should `git checkout {resolved_head_sha}`")

        for package in packages:
            staging_dir = Path(tempfile.mkdtemp(prefix=f"npm-stage-{package}-", dir=runner_temp))
            pack_output = output_dir / tarball_name_for_package(package, args.release_version)

            cmd = [
                str(BUILD_SCRIPT),
                "--package",
                package,
                "--release-version",
                args.release_version,
                "--staging-dir",
                str(staging_dir),
                "--pack-output",
                str(pack_output),
            ]

            if vendor_src is not None:
                cmd.extend(["--vendor-src", str(vendor_src)])

            try:
                run_command(cmd)
            finally:
                if not args.keep_staging_dirs:
                    shutil.rmtree(staging_dir, ignore_errors=True)

            final_messages.append(f"Staged {package} at {pack_output}")
    finally:
        if vendor_temp_root is not None and not args.keep_staging_dirs:
            shutil.rmtree(vendor_temp_root, ignore_errors=True)

    for msg in final_messages:
        print(msg)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
