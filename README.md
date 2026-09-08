# Echo

[![Latest release](https://img.shields.io/github/v/release/ddv1982/echo?display_name=tag&sort=semver)](https://github.com/ddv1982/echo/releases/latest)
[![CI](https://github.com/ddv1982/echo/actions/workflows/check.yml/badge.svg)](https://github.com/ddv1982/echo/actions/workflows/check.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE-MIT)
[![Platforms](https://img.shields.io/badge/platforms-Linux-555.svg)](https://github.com/ddv1982/echo/releases/latest)

Echo is private, local dictation for Linux. Press **Super+Alt+Space**, speak,
then press it again. Echo transcribes the recording on your machine and inserts
the transcribed text at the active cursor.

## Install

Debian and Ubuntu users can enable the signed package repository
(recommended; updates through apt and Software Updater):

```sh
bash <(curl -fsSL https://ddv1982.github.io/echo/install-apt-repo.sh)
sudo apt update
sudo apt install echo
```

The bootstrap authenticates the setup package with the archive keyring.

You can also download the latest Linux x86-64 installer from
[GitHub Releases](https://github.com/ddv1982/echo/releases/latest):

- `.deb` for Debian and Ubuntu (`sudo apt install ./echo_VERSION_amd64.deb`)
- `.rpm` for Fedora, RHEL, and compatible distributions
- `.AppImage` for a portable application

GitHub Release files include `SHA256SUMS` and GitHub attestations.

The raw `echo-desktop` binary is a last-resort option for systems that already
have its desktop libraries. The APT repository, packages, and AppImage are the
recommended installs.

## Make your first dictation

1. Open Echo from the application menu.
2. Choose and test a microphone on Home.
3. Install a local speech engine when Echo prompts you.
4. Finish the shortcut check. On older GNOME versions, Echo offers an explicit
   **Set up GNOME shortcut** action.
5. Put the cursor in another application. Press **Super+Alt+Space**, speak, and
   press the shortcut again.

Echo stays in the tray when its window closes. Home shows recording and
transcription progress; History stores completed transcripts; Dictionary lets
you define spoken-to-written replacements.

## Privacy and local data

Audio and transcripts stay on this machine. Speech recognition, Dictionary
replacements, and text insertion run locally. Echo uses the network only to
download speech models and managed runtime components that you choose to install.

Settings and local data follow the XDG directories. Models and all managed
runtimes and components normally live in `~/.cache/echo`; history and
dictionary data normally live in `~/.local/share/echo`.

Each root uses an absolute `ECHO_CONFIG_DIR`, `ECHO_DATA_DIR`, or
`ECHO_MODEL_DIR` override first, then the corresponding absolute XDG directory,
then an absolute `HOME`. An explicit Echo override that is empty or relative is
an error. Empty or relative XDG values are ignored and may fall back to an
absolute `HOME`. Echo stops with an actionable error when no secure absolute
root can be resolved. It never falls back to a fixed directory under `/tmp`.

## CLI

The desktop binary can transcribe a WAV file without opening the app:

```sh
echo-desktop transcribe speech.wav --engine whisper --language de
```

See the [CLI reference](docs/cli.md) for recording, JSON output, language
catalogs, and one-run engine overrides.

## Build from source

You need Rust 1.89 or newer, Node.js 22 or newer, and the native desktop
dependencies for your distribution. On a prepared system:

```sh
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --release
```

Run `./target/release/echo-desktop`. See
[troubleshooting](docs/troubleshooting.md#build-dependencies) for Debian and
Ubuntu build packages.

## Help and project docs

- [Troubleshooting](docs/troubleshooting.md)
- [GPU runtime](docs/gpu-runtime.md)
- [Architecture](docs/architecture.md)
- [Quality assurance](docs/qa/README.md)
- [Release process](docs/RELEASING.md)
- [Project history](docs/history/README.md)
- [Third-party component notices](THIRD_PARTY.md)

## License

Echo is available under the [MIT license](LICENSE-MIT). Managed runtimes and
model weights retain the terms listed in [THIRD_PARTY.md](THIRD_PARTY.md).
