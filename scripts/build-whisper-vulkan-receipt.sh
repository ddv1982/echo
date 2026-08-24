#!/usr/bin/env bash
set -euo pipefail

# Build the one pinned, receipt-capable Vulkan candidate used by qualification.
# This never selects a production runtime.

readonly expected_commit="306c88f4d1286aec1bf96e544632897886af5501"
readonly expected_tag="v1.9.2"
readonly script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly repo_root="$(cd -- "${script_dir}/.." && pwd)"
readonly receipt_patch="${repo_root}/patches/whisper.cpp/v1.9.2-runtime-receipt.patch"
readonly required_libraries=(libwhisper libggml libggml-base libggml-cpu libggml-vulkan)

die() {
    printf 'build-whisper-vulkan-receipt: %s\n' "$*" >&2
    exit 2
}

usage() {
    cat <<'EOF'
Usage:
  scripts/build-whisper-vulkan-receipt.sh SOURCE_DIR OUTPUT_DIR
  scripts/build-whisper-vulkan-receipt.sh --check-source SOURCE_DIR

SOURCE_DIR must be a clean checkout at whisper.cpp v1.9.2 commit
306c88f4d1286aec1bf96e544632897886af5501. OUTPUT_DIR must not exist.
The resulting directory places whisper-cli and every built shared library beside
one another; invoke it with that directory first in LD_LIBRARY_PATH.
EOF
}

check_source() {
    local source_dir="$1"
    [[ -d "${source_dir}/.git" || -f "${source_dir}/.git" ]] || die "source is not a Git worktree: ${source_dir}"
    [[ -f "${source_dir}/CMakeLists.txt" ]] || die "source does not look like whisper.cpp: ${source_dir}"
    [[ "$(git -C "${source_dir}" rev-parse HEAD)" == "${expected_commit}" ]] || die "source HEAD is not ${expected_commit}"
    [[ "$(git -C "${source_dir}" rev-parse "${expected_tag}^{commit}")" == "${expected_commit}" ]] || die "${expected_tag} does not resolve to ${expected_commit}"
    [[ -z "$(git -C "${source_dir}" status --porcelain)" ]] || die "source worktree is not clean"
    git -C "${source_dir}" apply --check "${receipt_patch}" || die "receipt patch does not apply to source"
}

check_staged_runtime() {
    local stage_dir="$1"
    local stage_real
    local artifact
    local library
    local ldd_output
    local resolved
    local vulkan_path

    command -v readelf >/dev/null || die "readelf is required to verify runtime paths"
    stage_real="$(readlink -f -- "${stage_dir}")"
    [[ -x "${stage_dir}/whisper-cli" ]] || die "whisper-cli was not produced"
    for library in "${required_libraries[@]}"; do
        find "${stage_dir}" -maxdepth 1 -type f -name "${library}.so*" -print -quit | grep -q . || die "${library} was not produced beside whisper-cli"
    done
    while IFS= read -r -d '' artifact; do
        if readelf -d "${artifact}" | grep -Eq '\((RPATH|RUNPATH)\)'; then
            die "staged runtime has an RPATH or RUNPATH: ${artifact}"
        fi
    done < <(find "${stage_dir}" -maxdepth 1 -type f \( -name 'whisper-cli' -o -name '*.so*' \) -print0)

    ldd_output="$(LD_LIBRARY_PATH="${stage_dir}" ldd "${stage_dir}/whisper-cli")" || die "could not inspect whisper-cli dependencies"
    if grep -q 'not found' <<<"${ldd_output}"; then
        die "whisper-cli has unresolved shared-library dependencies"
    fi
    for library in "${required_libraries[@]}"; do
        resolved="$(awk -v library="${library}" '$1 ~ ("^" library "\\.so") && $2 == "=>" { print $3; exit }' <<<"${ldd_output}")"
        [[ -n "${resolved}" ]] || die "ldd did not report ${library}"
        resolved="$(readlink -f -- "${resolved}")"
        [[ "${resolved}" == "${stage_real}/"* ]] || die "${library} did not resolve from the staged runtime: ${resolved}"
    done
    vulkan_path="$(awk '$1 ~ /^libvulkan\.so\.1/ && $2 == "=>" { print $3; exit }' <<<"${ldd_output}")"
    [[ -n "${vulkan_path}" ]] || die "ldd did not report libvulkan.so.1"
    vulkan_path="$(readlink -f -- "${vulkan_path}")"
    case "${vulkan_path}" in
        /lib/*|/usr/lib/*) ;;
        *) die "libvulkan.so.1 did not resolve from a host system library: ${vulkan_path}" ;;
    esac
}

[[ -f "${receipt_patch}" ]] || die "receipt patch is missing: ${receipt_patch}"

if [[ "${1:-}" == "--check-source" ]]; then
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    check_source "$2"
    printf 'build-whisper-vulkan-receipt: source and patch verified\n'
    exit 0
fi

[[ $# -eq 2 ]] || { usage >&2; exit 2; }
source_dir="$(cd -- "$1" && pwd)"
output_dir="$2"
[[ ! -e "${output_dir}" && ! -L "${output_dir}" ]] || die "output must not already exist: ${output_dir}"
output_parent="$(cd -- "$(dirname -- "${output_dir}")" && pwd)"
output_dir="${output_parent}/$(basename -- "${output_dir}")"
check_source "${source_dir}"

scratch_dir="$(mktemp -d "${TMPDIR:-/tmp}/echo-whisper-vulkan-receipt.XXXXXX")"
worktree_dir="${scratch_dir}/source"
build_dir="${scratch_dir}/build"
stage_dir="${scratch_dir}/runtime"
cleanup() {
    git -C "${source_dir}" worktree remove --force "${worktree_dir}" >/dev/null 2>&1 || true
    rm -rf -- "${scratch_dir}"
}
trap cleanup EXIT

git -C "${source_dir}" worktree add --detach "${worktree_dir}" "${expected_commit}" >/dev/null
git -C "${worktree_dir}" apply --check "${receipt_patch}"
git -C "${worktree_dir}" apply "${receipt_patch}"

cmake -S "${worktree_dir}" -B "${build_dir}" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_SKIP_RPATH=ON \
    -DBUILD_SHARED_LIBS=ON \
    -DGGML_VULKAN=ON \
    -DWHISPER_BUILD_TESTS=OFF \
    -DWHISPER_BUILD_EXAMPLES=ON \
    -DWHISPER_BUILD_SERVER=OFF
cmake --build "${build_dir}" --config Release --target whisper-cli --parallel

mv -- "${build_dir}/bin" "${stage_dir}"
check_staged_runtime "${stage_dir}"

mv -- "${stage_dir}" "${output_dir}"
trap - EXIT
cleanup
printf 'build-whisper-vulkan-receipt: built %s from %s\n' "${output_dir}" "${expected_commit}"
