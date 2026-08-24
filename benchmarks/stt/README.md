# Echo speech benchmark

Build Echo, then compare any locally installed candidates through the same CLI boundary the app uses:

```sh
cargo build --release -p echo-desktop
python3 scripts/benchmark-stt.py \
  --binary target/release/echo-desktop \
  --manifest benchmarks/stt/fixtures.json \
  --candidate whisper:small \
  --candidate whisper:large-v3-turbo-q5_0 \
  --candidate parakeet \
  --repeats 3 \
  --output-dir target/stt-benchmark
```

The committed fixture manifest proves the script and silence behavior. It is too small to decide model quality. For a real comparison, copy it and add project-owned or appropriately licensed WAV files with hand-written references. Keep each language separate in the report and include normal dictation, technical terms, quiet speech, background noise, and silence.

`runs.jsonl` contains every raw observation. `summary.md` reports per-language WER, median inference real-time factor, and silence hallucinations. The built-in WER scorer is for whitespace-delimited languages, including Parakeet's European language set. It rejects Japanese, Chinese, Cantonese, Thai, Lao, Khmer, and Burmese speech fixtures because those need a character-error-rate scorer. A missing engine, model, or failed inference stops the run.

## Pinned multilingual subset

`corpus-fleurs.json` pins twenty CC-BY-4.0 FLEURS test recordings: four each for English, Dutch, German, French, and Spanish. Fetching is explicit and verifies the full source revision, byte size, SHA-256, and 16 kHz mono PCM16 shape:

```sh
python3 scripts/fetch-stt-corpus.py \
  --manifest benchmarks/stt/corpus-fleurs.json \
  --output-dir target/stt-fleurs-corpus
```

The generated `fixtures.json` can drive the normal benchmark. A CPU-only candidate uses the same runtime binary with upstream `--no-gpu`, so it is a valid negative control:

```sh
python3 scripts/benchmark-stt.py \
  --binary target/release/echo-desktop \
  --manifest target/stt-fleurs-corpus/fixtures.json \
  --candidate 'whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback' \
  --candidate 'whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback,cpu-only' \
  --warmups 1 --repeats 10 --output-dir target/stt-host-matrix
```

Evaluate the paired rows with `scripts/analyze-stt-host-matrix.py`. This subset covers clean read speech only. It cannot pass the production corpus gate until project-owned dictation, technical identifiers, fast and quiet speech, noise, false starts, silence, and nonspeech are added.

Keep fresh-cache first-use and populated-cache measurements in separate output directories. Record reset/reboot runs separately; a warmed run must never be presented as first-use evidence.
