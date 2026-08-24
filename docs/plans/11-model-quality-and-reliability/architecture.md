# Architecture

## Invariants

1. Engine stdout becomes transcript text only inside that engine adapter.
2. Direct typing never reads or changes the clipboard.
3. Clipboard fallback leaves the exact dictated text available after it reports `Pasted` or `ClipboardOnly`.
4. Settings projects the resolved engine from backend-owned resolution data instead of reimplementing Auto.
5. Recommended setup changes only new or explicitly activated configurations. Existing exact model pins remain exact.
6. Benchmarks use the shipping CLI boundary and fail loudly.

## Parakeet boundary

`crates/echo/src/stt/parakeet.rs` gains a private typed response:

```rust
#[derive(Deserialize)]
struct SherpaOutput {
    text: String,
}
```

Successful output is classified as:

- JSON-shaped: parse a JSON object and return `text`.
- Plain text: accept one legacy transcript.
- Empty: report a protocol error. A valid no-speech result is JSON with an empty `text` field.

Malformed JSON-shaped output is an engine error. Nonzero exit status still prefers stderr. The adapter also reports the model directory in `RunDetail.model_path`.

No parser type escapes the adapter.

## Injection boundary

`LinuxInjector::inject` tries direct typing first. Only fallback calls `paste_text`, which writes the transcript and dispatches Ctrl+V. The clipboard is not restored afterward.

`SysClipboard` chooses its protocol order from the session:

- Wayland: `wl-copy`, then X11 fallback.
- X11: `xclip`, then Wayland fallback.

Fallback no longer reads the previous clipboard, so it cannot restore stale text before the target consumes the transcript.

No new report variant is required. Existing history rows remain compatible.

## Model policy

`recommended_model(HardwareProfile)` remains the only managed recommendation policy:

```text
RAM unknown or < 8 GiB -> Base multilingual Q5_1
reported RAM within 512 MiB of 8 GiB or higher -> Large v3 Turbo Q5_0
```

Small remains catalogued, discoverable, repairable, removable, and explicitly installable. It is removed only from the primary recommendation.

Engine Auto remains source-aware and unchanged. Recommended activation pins `engine = whisper` and the chosen model. Parakeet activation pins `engine = parakeet` and clears the file-backed Whisper model so a later Auto selection is not constrained by a hidden stale pin.

## Settings projection

The existing `LanguageOptions.mode` is already computed through Rust model and engine resolution. The frontend uses it as the projection boundary:

```ts
type SpeechModelPresentation =
  | { kind: 'whisper' }
  | { kind: 'parakeet' }
```

No new IPC endpoint is needed.

- Explicit Parakeet renders the fixed Parakeet row immediately from the requested engine.
- Auto uses `languages.mode` to render the backend-resolved engine.
- Whisper retains exact installed-model selection.

After a settings write, the frontend refreshes language capability so the projection cannot remain stale.

## Benchmark boundary

`scripts/benchmark-stt.py` accepts:

```text
--binary PATH
--manifest PATH
--candidate fake
--candidate parakeet
--candidate whisper:MODEL
--repeats N
--output-dir PATH
```

The manifest names each WAV, language, and hand-written reference. The script isolates config and data directories, preserves runtime/model discovery, invokes one CLI process per row, and writes:

- `runs.jsonl` with exact candidate, utterance, repeat, transcript, word errors, timings, and hallucination state.
- `summary.md` with per-candidate, per-language aggregate WER, median real-time factor, and silence hallucination count.

The script uses only the Python standard library. A small committed manifest covering the existing speech and silence fixtures proves it with Fake. A real multilingual corpus can be supplied without changing the script.
