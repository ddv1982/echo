# Echo

Echo is private, local dictation for Linux. Press **Super+Alt+Space**, speak,
then press it again. Echo transcribes the recording on your machine and inserts
the transcribed text at the active cursor.

## Install

Echo publishes Linux x86-64 packages on
[GitHub Releases](https://github.com/ddv1982/echo/releases). Use the `.deb` on
Debian or Ubuntu, the `.rpm` on Fedora, or the AppImage on other distributions.

```sh
sudo apt install ./FILE.deb
# or
sudo dnf install ./FILE.rpm
# or
chmod +x FILE.AppImage && ./FILE.AppImage
```

The raw `echo-desktop` binary is also available for systems that already have
its desktop libraries. Packages and the AppImage are the recommended installs.

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

Settings and local data follow the XDG directories. Models and managed
components normally live in `~/.cache/echo`; history and dictionary data
normally live in `~/.local/share/echo`.

## CLI

The desktop binary can transcribe a WAV file without opening the app:

```sh
echo-desktop transcribe speech.wav --engine whisper --language de
```

See the [CLI reference](docs/cli.md) for recording, JSON output, language
catalogs, and one-run engine overrides.

## Build from source

You need Rust 1.88 or newer, Node.js 22 or newer, and the native desktop
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
- [Release history cleanup](docs/history/releases.md)

## License

Echo is available under the [MIT license](LICENSE-MIT).
