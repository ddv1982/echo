# Echo CLI reference

`echo-desktop` opens the desktop application when run without arguments. The
same binary also records dictation, transcribes existing WAV files, and lists
engine languages.

## Record dictation

Record one session until the configured recording limit:

```sh
echo-desktop rec --once
```

Toggle a managed session from a compositor shortcut:

```sh
echo-desktop rec --toggle
```

The first toggle starts recording. The second stops, transcribes, and inserts
the result at the active cursor. Use only one of `--once` and `--toggle`.

## Transcribe a WAV file

```sh
echo-desktop transcribe INPUT.wav [OPTIONS]
```

The command does not open the desktop, microphone, HUD, injector, history, or
recording stores. Text goes to standard output by default. Diagnostics go to
standard error. Options apply to one run and do not rewrite Settings.

Common options:

| Option | Effect |
| --- | --- |
| `--engine auto\|whisper\|parakeet` | Select the engine for this run. |
| `--model NAME` | Select a Whisper model. |
| `--language auto\|CODE` | Detect the language or pin a Whisper language code. |
| `--format text\|json` | Select the output format. |
| `--output PATH` | Write to a file. Use `-` for standard output. |
| `--raw` | Skip Echo's cleanup pass. Cannot be used with JSON. |
| `--whisper-acceleration cpu\|gpu` | Select the Whisper runtime. |
| `--whisper-threads N` | Override Whisper worker threads. |
| `--whisper-beam-size N` | Override the Whisper beam size. |
| `--whisper-best-of N` | Override Whisper candidate count. |
| `--whisper-no-fallback` | Disable Whisper temperature fallback. |

Examples:

```sh
echo-desktop transcribe speech.wav
echo-desktop transcribe speech.wav --engine whisper --model small --language de
echo-desktop transcribe speech.wav --format json --output result.json
echo-desktop transcribe speech.wav --raw
```

`--model` and the Whisper performance options cannot be used with Parakeet.
Parakeet supports automatic language selection only. The output path must not
refer to the input file.

## List languages

```sh
echo-desktop languages --engine whisper
echo-desktop languages --engine parakeet --format json
```

Omit `--engine` to use the engine selected by the current configuration.

## Development HUD

`echo-desktop --hud-demo` opens the recording HUD without starting a session.
It is a development aid and cannot be combined with a command.

Use `echo-desktop --help` and `echo-desktop COMMAND --help` for the options in
the installed version.
