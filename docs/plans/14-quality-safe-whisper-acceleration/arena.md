# Architecture arena

## Rubric

The independent judge scored evidence integrity, current-host progress, cross-hardware portability, production failure correctness, phase size, and packaging correctness.

| Candidate | Score | Result |
| --- | ---: | --- |
| B | 96 | Base |
| A | 93 | Operational grafts |
| C | 83 | Caller-compatibility grafts |
| D | 72 | Narrative grafts |

The judge ran on `gpt-5.5`. The candidates ran on `gpt-5.5`, `gpt-5.6-terra`, `gpt-5.6-luna`, and `gpt-5.4-mini`.

## Synthesis decision

Candidate B wins because it makes the inference process the authority for the selected device and loaded runtime. It also defines the clearest quarantine and one-retry behavior.

The synthesis grafts these parts:

- From Candidate A, use status-marked run bundles, stale-output refusal, a concrete sweep runner, explicit cache categories, and one launch contract.
- From Candidate C, preserve the current `prepare_with_config` caller seam, keep qualification mode out of user settings, and add telemetry compatibly.
- From Candidate D, keep the phase story direct: evidence, sweep, policy, packaging, then residency.

## Rejections

- Reject `vulkaninfo`, loader logs, device names, or indices as the sole device proof.
- Reject stored WER, timing, backend, device, cache, reset, driver, or ICD fields as authority.
- Reject ambient loader variables and benchmark-only library paths.
- Reject a global Vulkan default, a monolithic GPU package, and driver bundling.
- Reject asymmetric decoding settings between CPU and GPU controls.
- Reject v1.9.3 pre-release promotion from smoke evidence.
- Reject repeated fallback, model switching, and background qualification from user dictation.
- Reject residency before one-shot admission.

## Verification result

The lead read every candidate end to end and agreed with the independent base selection. The judge's only material caution is phase size. This plan splits its large first stage into run bundles, replay, receipt, cache/reset, and sweep phases so each change remains reviewable.
