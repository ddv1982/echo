#!/usr/bin/env bash
set -euo pipefail

fingerprint="${LINUX_GPG_FINGERPRINT:-}"
artifacts_dir="dist/rust/artifacts"
pages_dir="dist/rust/apt-pages"
stage_public=false

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
ROOT=$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)
cd "${ROOT}"

usage() {
  cat <<'USAGE' >&2
Usage: bash scripts/stage-apt-installer-assets.sh [--stage-public]

Renders and validates the APT installer. With --stage-public, also copies the
installer and repository setup package/checksum/signature to dist/rust/apt-pages.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --stage-public)
      stage_public=true
      shift
      ;;
    --fingerprint)
      fingerprint="$2"
      shift 2
      ;;
    --artifacts-dir)
      artifacts_dir="$2"
      shift 2
      ;;
    --pages-dir)
      pages_dir="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [ -z "${fingerprint}" ]; then
  echo "LINUX_GPG_FINGERPRINT or --fingerprint is required to render the APT installer." >&2
  exit 1
fi

installer="${artifacts_dir}/install-apt-repo.sh"
setup_deb="${artifacts_dir}/echo-repository-setup_1.0_all.deb"
setup_sha256="${setup_deb}.sha256"
setup_sha256_sig="${setup_sha256}.asc"

mkdir -p "${artifacts_dir}"
sed "s/__ECHO_APT_SIGNING_KEY_FINGERPRINT__/${fingerprint}/g" \
  scripts/install-apt-repo.sh > "${installer}"
chmod 0755 "${installer}"

python3 scripts/check_apt_installer.py \
  --rendered-installer "${installer}" \
  --expected-fingerprint "${fingerprint}"
sh -n "${installer}"

if [ "${stage_public}" = true ]; then
  apt_dir="${pages_dir}/apt"
  mkdir -p "${apt_dir}"
  cp "${installer}" "${pages_dir}/install-apt-repo.sh"
  cp "${setup_deb}" "${apt_dir}/echo-repository-setup_1.0_all.deb"
  cp "${setup_sha256}" "${apt_dir}/echo-repository-setup_1.0_all.deb.sha256"
  cp "${setup_sha256_sig}" "${apt_dir}/echo-repository-setup_1.0_all.deb.sha256.asc"
  python3 scripts/check_apt_installer.py \
    --rendered-installer "${pages_dir}/install-apt-repo.sh" \
    --expected-fingerprint "${fingerprint}"
  sh -n "${pages_dir}/install-apt-repo.sh"
fi
