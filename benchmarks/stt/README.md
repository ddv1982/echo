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
