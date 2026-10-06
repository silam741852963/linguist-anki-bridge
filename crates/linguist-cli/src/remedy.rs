//! Next-step guidance for errors caused by a missing optional dependency, an
//! unavailable capability or local limits. The error code stays the contract;
//! `next` is advice for people and never changes the exit code.

const WRITES: &str = "Collection writes need the verified native adapter: install the Linguist companion add-on (dist/linguist-bridge.ankiaddon) next to AnkiConnect, set an AnkiConnect API key and name its environment variable in anki.api_key_env, use a loopback anki.endpoint, open the profile, then check `linguist-anki-bridge doctor --bridge`. Previews, review, approval and `plans export` work without it.";

/// Guidance for the error's leading code, or `None` when the message itself
/// already names the fix (validation, conflicts, missing IDs).
pub fn next_step(message: &str) -> Option<&'static str> {
    let code = message.split(':').next().unwrap_or(message).trim();
    Some(match code {
        "ANKI_DEPENDENCY_UNAVAILABLE"
        | "ANKI_TRANSPORT_UNAVAILABLE"
        | "ANKI_READ_TIMEOUT"
        | "ANKI_HTTP_FAILURE" => {
            "Start Anki Desktop with the AnkiConnect add-on, check anki.endpoint (`linguist-anki-bridge config show anki.endpoint`), then run `linguist-anki-bridge doctor`. Offline preparation (`vocab add`, `grammar add`, `plans ...`) does not need Anki."
        }
        "ANKI_PROTOCOL_UNSUPPORTED" | "ANKI_REFLECTION_INVALID" => {
            "Install or update the AnkiConnect add-on (API version 6), restart Anki, then run `linguist-anki-bridge doctor`."
        }
        "ANKI_CREDENTIAL_UNAVAILABLE" => {
            "Export the AnkiConnect API key in the environment variable named by anki.api_key_env, or unset anki.api_key_env when AnkiConnect has no key."
        }
        "ANKI_PROFILE_CONFLICT" => {
            "Open the profile named by anki.expected_profile in Anki, or change it with `linguist-anki-bridge config set anki.expected_profile NAME`."
        }
        "REMOTE_ENDPOINT_NOT_ALLOWED"
        | "ENDPOINT_ADDRESS_POLICY_REJECTED"
        | "ANKI_DNS_UNAVAILABLE" => {
            "Use a loopback anki.endpoint such as http://127.0.0.1:8765, or list the host in network.allowed_remote_service_hosts (never while --offline)."
        }
        "APPLY_BINDING_WEAK" => {
            "Bind the plan to the live collection with `linguist-anki-bridge plans bind PLAN --digest DIGEST` (needs the verified native companion), then validate and approve the new revision. `plans export` works without a binding."
        }
        "CAPABILITY_UNAVAILABLE" => {
            let detail = message.to_ascii_lowercase();
            if detail.contains("native")
                || detail.contains("--apply")
                || detail.contains("managed writes")
                || detail.contains("companion")
            {
                WRITES
            } else if detail.contains("provider_read_policy") || detail.contains("proxy") {
                "A network read was refused by --offline, network.offline or the host policy. Run without --offline, list the host in network.allowed_remote_service_hosts, or prepare authored content: `--set dictionary.provider=authored`."
            } else if detail.contains("dictionary") {
                "Prepare from authored content with `--set dictionary.provider=authored` (give the meaning yourself), or use a dictionary and explanation language this build supports (`linguist-anki-bridge config describe dictionary.provider`)."
            } else if detail.contains("ollama") || detail.contains("model") {
                "Use a loopback Ollama endpoint with an installed model (`linguist-anki-bridge doctor --ollama`), or prepare without generation: `--set llm.enabled=false`."
            } else {
                "This build does not provide the capability named in the error. `linguist-anki-bridge config describe KEY` lists the values a setting supports; `linguist-anki-bridge doctor --local` lists what is available."
            }
        }
        "OLLAMA_TRANSPORT_UNAVAILABLE"
        | "OLLAMA_GATE_UNAVAILABLE"
        | "OLLAMA_HTTP_FAILED"
        | "OLLAMA_READ_FAILED"
        | "OLLAMA_DEADLINE"
        | "RESOURCE_OLLAMA_UNAVAILABLE" => {
            "Start Ollama (`ollama serve`) at llm.endpoint and check it with `linguist-anki-bridge doctor --ollama`, or prepare without generation: `--set llm.enabled=false`."
        }
        "OLLAMA_MODEL_MISSING" | "OLLAMA_MODEL_UNAVAILABLE" => {
            "Pull the model named by llm.model (`linguist-anki-bridge resources install --help` shows the pinned install), or pick an installed one with `linguist-anki-bridge config set llm.model MODEL`."
        }
        "OLLAMA_CREDENTIAL_UNAVAILABLE" => {
            "Export the credential in the environment variable named by llm.api_key_env, or unset llm.api_key_env for a local Ollama."
        }
        "OCR_ENGINE_UNAVAILABLE" | "OCR_LANGUAGE_PACK_MISSING" => {
            "Install Tesseract and the needed language packs (`tesseract --list-langs`), or a pinned pack with `linguist-anki-bridge resources install tesseract ...`; set ocr.executable and ocr.resource_path when they are not on the default path."
        }
        "BROWSER_HELPER_UNAVAILABLE" => {
            "Browser fallback is not available in this build; set browser.enabled=false."
        }
        "AUDIO_SYNTHESIS_FAILED" | "AUDIO_OUTPUT_MISSING" => {
            "Check audio.executable and audio.voice_resource (install a voice with `linguist-anki-bridge resources install piper ...`), or keep source audio with `--set audio.provider=preserve`."
        }
        "IMAGE_SEARCH_FAILED" => {
            "Retry later, or prepare without image search: `--set images.search_when_missing=false`."
        }
        "DICTIONARY_PROVIDER_FAILED"
            if message.contains("Offline") || message.contains("CacheMiss") =>
        {
            "--offline allows only cached dictionary pages. Run without --offline, or prepare from authored content: `--set dictionary.provider=authored`."
        }
        "DICTIONARY_PROVIDER_FAILED" => {
            "Retry later, or prepare from authored content only: `--set dictionary.provider=authored`."
        }
        "RESOURCE_OFFLINE" => {
            "Install from a local file (`--source PATH`), or run without --offline and network.offline."
        }
        "RESOURCE_HOST_NOT_ALLOWED" => {
            "Download the file yourself and install it from a local path, or list the host in network.allowed_remote_service_hosts."
        }
        "EDITOR_UNAVAILABLE" => {
            "Set editing.editor_argv, or VISUAL/EDITOR, or pass a typed patch with `--patch FILE`."
        }
        "STORE_NOT_FOUND" => {
            "There is no local state yet. Prepare content first (`linguist-anki-bridge vocab add --help`), or check storage.state_dir."
        }
        "STORAGE_FREE_SPACE" | "RESOURCE_FREE_SPACE" => {
            "Free disk space on the file system holding storage.state_dir, or move state with storage.state_dir; `linguist-anki-bridge cache prune` previews what local cache can be removed."
        }
        "INPUT_TOO_LARGE" | "INPUT_FILE_TOO_LARGE" | "INPUT_RECORD_TOO_LARGE" => {
            "Split the input into smaller files, or raise input.max_file_mb or input.max_record_chars deliberately."
        }
        "INPUT_BATCH_TOO_LARGE" => {
            "Split the batch (for example with --limit or a narrower --query), or raise selection.max_notes deliberately."
        }
        "NOTE_SELECTOR_TOO_LARGE" => {
            "Select at most 10000 note IDs, or use --query/--deck with --limit; the query must fit input.max_record_chars."
        }
        "INPUT_STDIN_IS_TERMINAL" => {
            "Pipe or redirect the input (`... --document - < file.json`), or pass a file path."
        }
        "USAGE" => "Run `linguist-anki-bridge --help` or `linguist-anki-bridge COMMAND --help`.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::next_step;

    #[test]
    fn hints_follow_the_leading_code_only() {
        assert!(next_step("ANKI_DEPENDENCY_UNAVAILABLE").is_some());
        assert_eq!(
            next_step("CAPABILITY_UNAVAILABLE: native export"),
            Some(super::WRITES)
        );
        assert_ne!(
            next_step(
                "CAPABILITY_UNAVAILABLE: dictionary definition translation is not implemented"
            ),
            Some(super::WRITES)
        );
        assert!(next_step("OCR_ENGINE_UNAVAILABLE: tesseract").is_some());
        assert!(next_step("PLAN_NOT_FOUND").is_none());
        assert!(next_step("detail mentions ANKI_DEPENDENCY_UNAVAILABLE").is_none());
    }
}
