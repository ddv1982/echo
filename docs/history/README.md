# Project history

Git history is the complete record for superseded implementation plans. Large
run outputs and generated QA reports live outside the active source tree.

The [2026-08-30 evidence manifest](evidence-2026-08-30.md) records the first
external archive. It covers plans 01 through 17, raw `.audit` data, frozen QA
runs, and retired qualification commands.

Build and verify that archive from the preserved source commit:

```sh
scripts/build-history-archive.sh build \
  5fb579b001bc1da55762d65255a460dcd9ed54cc \
  /tmp/echo-evidence-2026-08-30.tar.gz
scripts/build-history-archive.sh verify \
  /tmp/echo-evidence-2026-08-30.tar.gz \
  8e15356cfeed861756253499688fa9bee3686795f25d41ed4c903d9af63053de
```

The builder reads the named Git tree, not the working tree. Two builds from
the same Git object produce the same archive bytes.
