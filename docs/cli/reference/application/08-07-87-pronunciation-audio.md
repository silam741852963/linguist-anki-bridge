# Application specification reference

### 8.7 Pronunciation audio

Preserve user recordings and all valid pronunciation tracks unless replacement is explicitly requested. Prefer validated dictionary recordings for an accepted reading/locale; then configured local TTS or online TTS fallback. Rust has eSpeak and remote Google TTS; Python uses gTTS. **REVIEW R22:** provider order and acceptable voice/locale defaults.

For Japanese, synthesize the accepted reading where available, rather than assuming written Kanji will be pronounced correctly. Keep pronunciation text, locale, provider, voice and sound reference aligned. UK/US accents and alternate Japanese readings remain visible. Grammar audio is optional and separately specified.

Bound downloads, validate audio content/MIME, restrict provider URLs and redirects, and use correct extensions. Current native dictionary audio is capped at 6 MiB and remote TTS limits text length; surface these capability limits. Provider failure must not clear old audio. Remote TTS availability is not guaranteed by its existence in source.
