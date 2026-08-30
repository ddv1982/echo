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
  fe1b078fc95beac0897d8d8e5a0e7bdc7720c7164760e47a01ad5aa93e43a79c
```

The builder reads the named Git tree, not the working tree. Two builds from
the same Git object produce the same archive bytes.
