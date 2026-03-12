#!/usr/bin/env bash
set -euo pipefail

runtime="${CONTAINER_RUNTIME:-}"
target="${1:-x86_64-unknown-linux-musl}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
image_name="${MUSL_RELEASE_IMAGE_NAME:-dccodex-musl-release}"
target_dir_name="${CARGO_TARGET_DIR_NAME:-target-musl-container}"

if [[ -z "${runtime}" ]]; then
  if command -v podman >/dev/null 2>&1; then
    runtime="podman"
  elif command -v docker >/dev/null 2>&1; then
    runtime="docker"
  else
    echo "Neither podman nor docker is available." >&2
    exit 1
  fi
fi

case "${target}" in
  x86_64-unknown-linux-musl|aarch64-unknown-linux-musl)
    ;;
  *)
    echo "Unsupported musl target: ${target}" >&2
    exit 1
    ;;
esac

artifact_name="dccodex-${target}"

"${runtime}" build -t "${image_name}" -f "${repo_root}/Containerfile.musl-release" "${repo_root}"

"${runtime}" run --rm -t \
  -v "${repo_root}:/workspace" \
  -w /workspace/codex-rs \
  -e TARGET="${target}" \
  -e GITHUB_ENV=/tmp/github-env \
  -e RUNNER_TEMP=/tmp \
  -e CARGO_HOME=/workspace/.cargo-home-musl \
  -e CARGO_TARGET_DIR="/workspace/codex-rs/${target_dir_name}" \
  "${image_name}" \
  bash -lc '
    set -euo pipefail

    mkdir -p "${CARGO_HOME}/bin"
    : > "${CARGO_HOME}/config.toml"

    rustup target add "${TARGET}"
    : > "${GITHUB_ENV}"
    bash /workspace/.github/scripts/install-musl-build-tools.sh
    while IFS= read -r line; do
      export "${line}"
    done < "${GITHUB_ENV}"

    ubsan=""
    if command -v ldconfig >/dev/null 2>&1; then
      ubsan="$(ldconfig -p | grep -m1 "libubsan\\.so\\.1" | sed -E "s/.*=> (.*)$/\\1/" || true)"
    fi
    if [[ -n "${ubsan}" ]]; then
      wrapper="${RUNNER_TEMP}/rustc-ubsan-wrapper"
      cat > "${wrapper}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export LD_PRELOAD="${ubsan}\${LD_PRELOAD:+:\${LD_PRELOAD}}"
exec "\$1" "\${@:2}"
EOF
      chmod +x "${wrapper}"
      export RUSTC_WRAPPER="${wrapper}"
      unset RUSTC_WORKSPACE_WRAPPER || true
    fi

    sanitize_flags() {
      local input="${1:-}"
      input="${input//-fsanitize=undefined/}"
      input="${input//-fno-sanitize-recover=undefined/}"
      input="${input//-fno-sanitize-trap=undefined/}"
      echo "${input}"
    }

    host_linux_headers=()
    if [[ -d /usr/include ]]; then
      host_linux_headers+=("-idirafter" "/usr/include")
    fi
    if [[ -d /usr/include/x86_64-linux-gnu ]]; then
      host_linux_headers+=("-idirafter" "/usr/include/x86_64-linux-gnu")
    fi
    if [[ ${#host_linux_headers[@]} -gt 0 ]]; then
      export CFLAGS="${CFLAGS} ${host_linux_headers[*]}"
      export CXXFLAGS="${CXXFLAGS} ${host_linux_headers[*]}"
    fi

    export AWS_LC_SYS_NO_JITTER_ENTROPY=1
    export LZMA_API_STATIC=1
    export LIBLZMA_NO_PKG_CONFIG=1
    target_no_jitter="AWS_LC_SYS_NO_JITTER_ENTROPY_${TARGET//-/_}"
    export "${target_no_jitter}=1"
    export RUSTFLAGS=
    export CARGO_ENCODED_RUSTFLAGS=
    export RUSTDOCFLAGS=
    export CARGO_BUILD_RUSTFLAGS=
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS=
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS=
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS=
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS=
    export CFLAGS="$(sanitize_flags "${CFLAGS}")"
    export CXXFLAGS="$(sanitize_flags "${CXXFLAGS}")"
    # rustc links musl binaries with -nodefaultlibs, so the final link still
    # needs explicit math/GCC runtime libraries even though we use musl-gcc.
    link_args=(
      "-C" "link-arg=-lm"
      "-C" "link-arg=-lgcc"
      "-C" "link-arg=-lgcc_eh"
    )
    encoded_rustflags=""
    separator="$(printf "\037")"
    for arg in "${link_args[@]}"; do
      encoded_rustflags+="${arg}${separator}"
    done
    encoded_rustflags="${encoded_rustflags%${separator}}"
    export CARGO_ENCODED_RUSTFLAGS="${encoded_rustflags}"
    export CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-thin}"
    export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
    cargo build --release -p codex-cli --target "${TARGET}"

    binary="/workspace/codex-rs/${CARGO_TARGET_DIR##/workspace/codex-rs/}/${TARGET}/release/codex"
    if [[ ! -x "${binary}" ]]; then
      echo "Expected binary not found: ${binary}" >&2
      exit 1
    fi

    "${binary}" --help >/dev/null

    mkdir -p /workspace/codex-rs/dist/release
    cp "${binary}" "/workspace/codex-rs/dist/release/'"${artifact_name}"'"
    tar -C /workspace/codex-rs/dist/release -czf "/workspace/codex-rs/dist/release/'"${artifact_name}"'.tar.gz" "'"${artifact_name}"'"
  '

echo "Built /home/pat/projects/dccodex/codex-rs/dist/release/${artifact_name}.tar.gz"
