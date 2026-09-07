#!/bin/sh
set -eu

setup_url="${ECHO_REPOSITORY_SETUP_URL:-https://ddv1982.github.io/echo/apt/echo-repository-setup_1.0_all.deb}"
setup_sha256_url="${ECHO_REPOSITORY_SETUP_SHA256_URL:-${setup_url}.sha256}"
setup_sha256_sig_url="${ECHO_REPOSITORY_SETUP_SHA256_SIG_URL:-${setup_sha256_url}.asc}"
signing_key_url="${ECHO_REPOSITORY_SETUP_SIGNING_KEY_URL:-https://ddv1982.github.io/echo/apt/echo-archive-keyring.pgp}"
expected_sha256="${ECHO_REPOSITORY_SETUP_SHA256:-}"
expected_signing_fingerprint="${ECHO_REPOSITORY_SETUP_SIGNING_KEY_FINGERPRINT:-__ECHO_APT_SIGNING_KEY_FINGERPRINT__}"
tmp_dir=""
apt_install_dir=""
setup_deb=""
apt_setup_deb=""
setup_sha256_file=""
setup_sha256_sig_file=""
signing_key_file=""
gnupg_home=""

cleanup() {
  if [ -n "$apt_install_dir" ]; then
    rm -rf "$apt_install_dir"
  fi
  if [ -n "$tmp_dir" ]; then
    rm -rf "$tmp_dir"
  fi
}
trap cleanup EXIT
trap 'trap - EXIT; cleanup; exit 130' HUP INT TERM

case "$expected_signing_fingerprint" in
  __ECHO_*_SIGNING_KEY_FINGERPRINT__)
    expected_signing_fingerprint=""
    ;;
esac
expected_signing_fingerprint="$(printf '%s' "$expected_signing_fingerprint" | tr -d '[:space:]' | tr '[:lower:]' '[:upper:]')"

if command -v curl >/dev/null 2>&1; then
  downloader="curl"
elif command -v wget >/dev/null 2>&1; then
  downloader="wget"
else
  echo "Echo repository setup requires curl or wget." >&2
  exit 1
fi

download_to() {
  url="$1"
  output="$2"
  case "$downloader" in
    curl)
      curl -fsSLo "$output" "$url"
      ;;
    wget)
      wget -qO "$output" "$url"
      ;;
  esac
}

calculate_sha256() {
  file_path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file_path" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file_path" | awk '{print $1}'
  else
    echo "Missing sha256 verifier. Install sha256sum or shasum." >&2
    exit 1
  fi
}

require_expected_keyring_fingerprint() {
  keyring_fingerprints_file="$tmp_dir/keyring-fingerprints.txt"
  if ! GNUPGHOME="$gnupg_home" gpg --batch --import-options show-only --with-colons --import "$signing_key_file" >"$keyring_fingerprints_file" 2>/dev/null; then
    echo "Could not inspect downloaded Echo setup signing keyring." >&2
    exit 1
  fi

  keyring_validation="$(awk -F: -v expected="$expected_signing_fingerprint" '
    function finish_primary_key() {
      if (!seen_primary_key) {
        return
      }
      primary_key_count++
      if (primary_key_has_expected_signer) {
        accepted_primary_key_count++
      } else {
        unexpected_primary_key_fingerprint = primary_key_fingerprint
      }
      primary_key_has_expected_signer = 0
      primary_key_fingerprint = ""
    }
    $1 == "pub" {
      finish_primary_key()
      seen_primary_key = 1
      next
    }
    $1 == "fpr" && seen_primary_key {
      fingerprint = toupper($10)
      gsub(/[[:space:]]/, "", fingerprint)
      if (primary_key_fingerprint == "") {
        primary_key_fingerprint = fingerprint
      }
      if (fingerprint == expected) {
        primary_key_has_expected_signer = 1
      }
    }
    END {
      finish_primary_key()
      if (primary_key_count == 0) {
        print "NO_PRIMARY_KEY"
      } else if (accepted_primary_key_count == 0) {
        print "MISSING_PINNED_SIGNER"
      } else if (unexpected_primary_key_fingerprint != "") {
        print "UNEXPECTED_PRIMARY_KEY " unexpected_primary_key_fingerprint
      } else if (accepted_primary_key_count > 1) {
        print "MULTIPLE_ACCEPTED_PRIMARY_KEYS"
      } else {
        print "OK"
      }
    }
  ' "$keyring_fingerprints_file")"

  case "$keyring_validation" in
    OK)
      ;;
    NO_PRIMARY_KEY)
      echo "Downloaded Echo setup signing keyring did not contain a primary key." >&2
      exit 1
      ;;
    MISSING_PINNED_SIGNER)
      echo "Downloaded Echo setup signing keyring did not contain the pinned signer fingerprint." >&2
      echo "Expected signer: $expected_signing_fingerprint" >&2
      exit 1
      ;;
    UNEXPECTED_PRIMARY_KEY*)
      unexpected_primary_key="${keyring_validation#UNEXPECTED_PRIMARY_KEY }"
      echo "Downloaded Echo setup signing keyring contained an unexpected primary key." >&2
      echo "Expected signer:    $expected_signing_fingerprint" >&2
      echo "Unexpected primary: $unexpected_primary_key" >&2
      exit 1
      ;;
    MULTIPLE_ACCEPTED_PRIMARY_KEYS)
      echo "Downloaded Echo setup signing keyring contained multiple primary keys for the pinned signer." >&2
      echo "Expected signer: $expected_signing_fingerprint" >&2
      exit 1
      ;;
    *)
      echo "Could not validate downloaded Echo setup signing keyring." >&2
      exit 1
      ;;
  esac
}

verify_sha256_sidecar_signature() {
  signature_status_file="$tmp_dir/setup-sha256-signature.status"
  if ! GNUPGHOME="$gnupg_home" gpg --batch --status-fd 3 --verify "$setup_sha256_sig_file" "$setup_sha256_file" 3>"$signature_status_file" >/dev/null 2>&1; then
    echo "Echo setup SHA256 signature verification failed." >&2
    exit 1
  fi

  validsig_matches="$(awk -v expected="$expected_signing_fingerprint" '
    $1 == "[GNUPG:]" && $2 == "VALIDSIG" {
      signer = toupper($3)
      primary = toupper($NF)
      gsub(/[[:space:]]/, "", signer)
      gsub(/[[:space:]]/, "", primary)
      if (signer == expected || primary == expected) {
        matched = 1
      } else {
        if (unexpected != "") {
          unexpected = unexpected " "
        }
        unexpected = unexpected signer
        if (primary != "" && primary != signer) {
          unexpected = unexpected "/" primary
        }
      }
      seen = 1
    }
    END {
      if (!seen) {
        print "NO_VALIDSIG"
      } else if (matched) {
        print "OK"
      } else {
        print unexpected
      }
    }
  ' "$signature_status_file")"

  if [ "$validsig_matches" = "NO_VALIDSIG" ]; then
    echo "Echo setup SHA256 signature did not produce a VALIDSIG fingerprint." >&2
    exit 1
  fi

  if [ "$validsig_matches" != "OK" ]; then
    echo "Echo setup SHA256 signature was made by an unexpected signer." >&2
    echo "Expected signing or primary fingerprint: $expected_signing_fingerprint" >&2
    echo "Actual signing/primary fingerprint(s):   $validsig_matches" >&2
    exit 1
  fi
}

authenticate_sha256_sidecar() {
  if ! command -v gpg >/dev/null 2>&1; then
    echo "Missing gpg. Install GnuPG or provide ECHO_REPOSITORY_SETUP_SHA256 from a trusted source." >&2
    exit 1
  fi
  if [ -z "$expected_signing_fingerprint" ]; then
    echo "Missing pinned Echo setup signing key fingerprint." >&2
    echo "Use the release-published installer, or set ECHO_REPOSITORY_SETUP_SIGNING_KEY_FINGERPRINT from a trusted source." >&2
    exit 1
  fi

  printf 'Downloading Echo repository setup SHA256 sidecar and signature...\n'
  download_to "$setup_sha256_url" "$setup_sha256_file"
  download_to "$setup_sha256_sig_url" "$setup_sha256_sig_file"
  download_to "$signing_key_url" "$signing_key_file"

  require_expected_keyring_fingerprint
  GNUPGHOME="$gnupg_home" gpg --batch --import "$signing_key_file" >/dev/null 2>&1
  verify_sha256_sidecar_signature

  expected_sha256="$(awk 'NF { print $1; exit }' "$setup_sha256_file")"
  if [ -z "$expected_sha256" ]; then
    echo "Authenticated SHA256 sidecar did not contain a checksum." >&2
    exit 1
  fi
  printf 'Authenticated Echo repository setup SHA256 signature with key %s.\n' "$expected_signing_fingerprint"
}

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/echo-repository-setup.XXXXXX")"
setup_deb="$tmp_dir/echo-repository-setup_1.0_all.deb"
setup_sha256_file="$tmp_dir/echo-repository-setup_1.0_all.deb.sha256"
setup_sha256_sig_file="$tmp_dir/echo-repository-setup_1.0_all.deb.sha256.asc"
signing_key_file="$tmp_dir/echo-archive-keyring.pgp"
gnupg_home="$tmp_dir/gnupg"
mkdir -m 700 "$gnupg_home"

printf 'Downloading Echo repository setup package...\n'
download_to "$setup_url" "$setup_deb"
chmod 0644 "$setup_deb"

if [ -z "$expected_sha256" ]; then
  authenticate_sha256_sidecar
fi

if [ -z "$expected_sha256" ]; then
  echo "Could not resolve an authenticated SHA256 for Echo repository setup package." >&2
  echo "Use the signed sidecar path, or set ECHO_REPOSITORY_SETUP_SHA256 from a trusted source." >&2
  exit 1
fi

actual_sha256="$(calculate_sha256 "$setup_deb")"
if [ "$actual_sha256" != "$expected_sha256" ]; then
  echo "Checksum verification failed for Echo repository setup package." >&2
  echo "Expected: $expected_sha256" >&2
  echo "Actual:   $actual_sha256" >&2
  exit 1
fi
printf 'Verified Echo repository setup package SHA256.\n'

apt_install_dir="$(mktemp -d "${TMPDIR:-/tmp}/echo-repository-install.XXXXXX")"
apt_setup_deb="$apt_install_dir/echo-repository-setup_1.0_all.deb"
cp "$setup_deb" "$apt_setup_deb"
chmod 0755 "$apt_install_dir"
chmod 0644 "$apt_setup_deb"

printf 'Installing Echo APT repository setup package...\n'
sudo apt install -y "$apt_setup_deb"

printf '\nEcho APT repository is enabled. Next run:\n'
printf '  sudo apt update\n'
printf '  sudo apt install echo\n'
