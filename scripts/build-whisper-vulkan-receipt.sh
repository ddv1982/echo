#!/usr/bin/env bash
set -euo pipefail

# Build a pinned, receipt-capable Vulkan candidate used by qualification.
# This never selects a production runtime. v1.9.3 is investigative only.

readonly script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly repo_root="$(cd -- "${script_dir}/.." && pwd)"
readonly receipt_patch="${repo_root}/patches/whisper.cpp/runtime-receipt.patch"
readonly probe_patch="${repo_root}/patches/whisper.cpp/runtime-probe.patch"
readonly runtime_verifier="${repo_root}/scripts/verify-whisper-vulkan-runtime.sh"
readonly required_libraries=(libwhisper libggml libggml-base)

die() {
    printf 'build-whisper-vulkan-receipt: %s\n' "$*" >&2
    exit 2
}

usage() {
    cat <<'EOF'
Usage:
  scripts/build-whisper-vulkan-receipt.sh SOURCE_DIR OUTPUT_DIR
  scripts/build-whisper-vulkan-receipt.sh --check-source SOURCE_DIR
  scripts/build-whisper-vulkan-receipt.sh --revision v1.9.3 SOURCE_DIR OUTPUT_DIR
  scripts/build-whisper-vulkan-receipt.sh --revision v1.9.3 --check-source SOURCE_DIR

SOURCE_DIR must be a clean checkout at whisper.cpp v1.9.2 commit
306c88f4d1286aec1bf96e544632897886af5501 by default, or the explicitly
requested supported revision. OUTPUT_DIR must not exist. The shared
runtime-receipt.patch applies unchanged to v1.9.2 and v1.9.3.
The resulting directory places whisper-cli and every built shared library beside
one another; invoke it with that directory first in LD_LIBRARY_PATH.
EOF
}

revision="v1.9.2"
if [[ "${1:-}" == "--revision" ]]; then
    [[ $# -ge 3 ]] || { usage >&2; exit 2; }
    revision="$2"
    shift 2
fi

case "${revision}" in
    v1.9.2)
        expected_commit="306c88f4d1286aec1bf96e544632897886af5501"
        ;;
    v1.9.3)
        expected_commit="371b5a7561823ab2bb32142d2751e35e7534727b"
        ;;
    *) die "unsupported revision: ${revision} (supported: v1.9.2, v1.9.3)" ;;
esac

check_source() {
    local source_dir="$1"
    [[ -d "${source_dir}/.git" || -f "${source_dir}/.git" ]] || die "source is not a Git worktree: ${source_dir}"
    [[ -f "${source_dir}/CMakeLists.txt" ]] || die "source does not look like whisper.cpp: ${source_dir}"
    [[ "$(git -C "${source_dir}" rev-parse HEAD)" == "${expected_commit}" ]] || die "source HEAD is not ${expected_commit}"
    [[ "$(git -C "${source_dir}" rev-parse "${revision}^{commit}")" == "${expected_commit}" ]] || die "${revision} does not resolve to ${expected_commit}"
    [[ -z "$(git -C "${source_dir}" status --porcelain)" ]] || die "source worktree is not clean"
    git -C "${source_dir}" apply --check "${receipt_patch}" || die "receipt patch does not apply to source"
    git -C "${source_dir}" apply --check "${probe_patch}" || die "runtime probe patch does not apply to source"
}

check_staged_runtime() {
    local stage_dir="$1"
    local stage_real
    local artifact
    local library
    local ldd_output
    local resolved

    command -v readelf >/dev/null || die "readelf is required to verify runtime paths"
    stage_real="$(readlink -f -- "${stage_dir}")"
    [[ -x "${stage_dir}/whisper-cli" ]] || die "whisper-cli was not produced"
    [[ -x "${stage_dir}/echo-whisper-runtime-probe" ]] || die "echo-whisper-runtime-probe was not produced"
    for library in "${required_libraries[@]}"; do
        find "${stage_dir}" -maxdepth 1 -type f -name "${library}.so*" -print -quit | grep -q . || die "${library} was not produced beside whisper-cli"
    done
    while IFS= read -r -d '' artifact; do
        if readelf -d "${artifact}" | grep -Eq '\((RPATH|RUNPATH)\)'; then
            die "staged runtime has an RPATH or RUNPATH: ${artifact}"
        fi
    done < <(find "${stage_dir}" -maxdepth 1 -type f \( -name 'whisper-cli' -o -name 'echo-whisper-runtime-probe' -o -name '*.so*' \) -print0)

    if LD_LIBRARY_PATH="${stage_dir}" ldd "${stage_dir}/echo-whisper-runtime-probe" | grep -q 'not found'; then
        die "echo-whisper-runtime-probe has unresolved shared-library dependencies"
    fi

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
}

[[ -f "${receipt_patch}" ]] || die "receipt patch is missing: ${receipt_patch}"
[[ -f "${probe_patch}" ]] || die "runtime probe patch is missing: ${probe_patch}"
[[ -x "${runtime_verifier}" ]] || die "runtime verifier is missing or not executable: ${runtime_verifier}"

if [[ "${1:-}" == "--check-source" ]]; then
    [[ $# -eq 2 ]] || { usage >&2; exit 2; }
    check_source "$2"
    printf 'build-whisper-vulkan-receipt: %s source and patch verified\n' "${revision}"
    exit 0
fi

[[ $# -eq 2 ]] || { usage >&2; exit 2; }
source_dir="$(cd -- "$1" && pwd)"
output_dir="$2"
[[ ! -e "${output_dir}" && ! -L "${output_dir}" ]] || die "output must not already exist: ${output_dir}"
output_parent="$(cd -- "$(dirname -- "${output_dir}")" && pwd)"
output_dir="${output_parent}/$(basename -- "${output_dir}")"
check_source "${source_dir}"
source_date_epoch="$(git -C "${source_dir}" show -s --format=%ct "${expected_commit}")"
[[ "${source_date_epoch}" =~ ^[0-9]+$ ]] || die "could not derive SOURCE_DATE_EPOCH"

scratch_dir="$(mktemp -d "${TMPDIR:-/tmp}/echo-whisper-vulkan-receipt.XXXXXX")"
worktree_dir="${scratch_dir}/source"
build_dir="${scratch_dir}/build"
stage_dir="${scratch_dir}/runtime"
compiler_path_flags="-ffile-prefix-map=${scratch_dir}=/usr/src/echo-whisper-runtime -fmacro-prefix-map=${scratch_dir}=/usr/src/echo-whisper-runtime -fdebug-prefix-map=${scratch_dir}=/usr/src/echo-whisper-runtime"
cleanup() {
    git -C "${source_dir}" worktree remove --force "${worktree_dir}" >/dev/null 2>&1 || true
    rm -rf -- "${scratch_dir}"
}
trap cleanup EXIT

git -C "${source_dir}" worktree add --detach "${worktree_dir}" "${expected_commit}" >/dev/null
git -C "${worktree_dir}" apply --check "${receipt_patch}"
git -C "${worktree_dir}" apply "${receipt_patch}"
git -C "${worktree_dir}" apply --check "${probe_patch}"
git -C "${worktree_dir}" apply "${probe_patch}"

SOURCE_DATE_EPOCH="${source_date_epoch}" cmake -S "${worktree_dir}" -B "${build_dir}" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_FLAGS="${compiler_path_flags}" \
    -DCMAKE_CXX_FLAGS="${compiler_path_flags}" \
    -DCMAKE_SKIP_RPATH=ON \
    -DBUILD_SHARED_LIBS=ON \
    -DGGML_NATIVE=OFF \
    -DGGML_BACKEND_DL=ON \
    -DGGML_CPU_ALL_VARIANTS=ON \
    -DGGML_VULKAN=ON \
    -DWHISPER_BUILD_TESTS=OFF \
    -DWHISPER_BUILD_EXAMPLES=ON \
    -DWHISPER_BUILD_SERVER=OFF
SOURCE_DATE_EPOCH="${source_date_epoch}" cmake --build "${build_dir}" --config Release --target whisper-cli echo-whisper-runtime-probe --parallel

mv -- "${build_dir}/bin" "${stage_dir}"
check_staged_runtime "${stage_dir}"
"${runtime_verifier}" --create \
    "${stage_dir}" \
    "${build_dir}/CMakeCache.txt" \
    "${revision}" \
    "${expected_commit}" \
    "${source_date_epoch}" \
    "${repo_root}" \
    "${probe_patch}" \
    "${receipt_patch}"
"${runtime_verifier}" --verify "${stage_dir}"

mv -- "${stage_dir}" "${output_dir}"
trap - EXIT
cleanup
printf 'build-whisper-vulkan-receipt: built %s from %s (%s)\n' "${output_dir}" "${expected_commit}" "${revision}"
