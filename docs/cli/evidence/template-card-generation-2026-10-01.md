# Disposable Anki template card generation

On 2026-10-01, the installed Anki Python package reported version `25.09.2`, build `3d813c83`. The following command passed:

```sh
cargo run --locked -q -p linguist-cli -- --output json models builtin | /usr/bin/python3.14 scripts/verify-template-cards.py
```

The script reads the CLI's built-in manifests, creates a collection in a temporary directory, installs both note types there, and checks the exact field order, template order/ordinals, front/back HTML and CSS after installation. It adds disposable notes and observes generated card ordinals:

| Model | Input condition | Generated ordinals |
| --- | --- | --- |
| Vocabulary | Base Expression and Meaning | 0 |
| Vocabulary | Production flag and cue | 0, 1 |
| Vocabulary | Spelling flag and cue | 0, 2 |
| Vocabulary | Both optional tasks complete | 0, 1, 2 |
| Vocabulary | Flag without cue, or cue without flag | 0 |
| Grammar | Base Pattern, Meaning and RecognitionPrompt | 0 |
| Grammar | Application flag, prompt and answer | 0, 1 |
| Grammar | Flag without complete prompt/answer, or prompt/answer without flag | 0 |

Rendered Production and Application questions also contained their supplied cues. The temporary collection was closed and removed. The test did not access a user profile, use the native companion, migrate existing cards, or check retained scheduling/history. It proves template card generation for this installed Anki build and these examples; later Anki builds and native model installation require their own compatibility evidence.
