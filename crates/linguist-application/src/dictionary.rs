//! Verified dictionary facts remain separate from authored and captured field intent.
use linguist_config::Effective;
use linguist_core::{records::*, *};
use std::collections::BTreeMap;

fn validate_settings(settings: &Effective) -> Result<(), String> {
    let registry = linguist_config::Registry::builtin();
    for key in [
        "dictionary.provider",
        "network.max_response_mb",
        "input.max_file_mb",
    ] {
        registry.validate_value(
            key,
            settings
                .values
                .get(key)
                .ok_or("DICTIONARY_SETTING_MISSING")?,
        )?;
    }
    Ok(())
}

/// Enrich a retained plan as a child revision, retaining the recoverable original.
/// Uses the plan's frozen settings; no current configuration can change its policy.
pub fn enrich_revision(
    store: &mut linguist_store::Store,
    base: &PlanRevision,
    dictionary: Option<&dyn crate::DictionaryPort>,
) -> Result<PlanRevision, String> {
    if store.latest_revision(base.id)? != base.revision
        || store.revision(base.id, base.revision)? != *base
    {
        return Err("DICTIONARY_BASE_CONFLICT".into());
    }
    let settings = Effective {
        version: base.settings.version,
        values: base.settings.values.clone(),
        provenance: base.settings.provenance.clone(),
        fingerprint: base.settings.fingerprint.clone(),
        semantic_fingerprint: base.settings.semantic_fingerprint.clone(),
        execution_fingerprint: base.settings.execution_fingerprint.clone(),
    };
    validate_settings(&settings)?;
    if settings.values["dictionary.provider"] == "authored" {
        return Err(
            "CAPABILITY_UNAVAILABLE: dictionary enrichment requires a dictionary provider".into(),
        );
    }
    if base.documents.iter().any(|document| {
        document.sources.iter().any(|source| {
            matches!(
                source.kind.as_str(),
                "jisho_api_v1" | "wiktionary_definition_v0.8"
            )
        })
    }) {
        return Err("DICTIONARY_ALREADY_ENRICHED".into());
    }
    let cap = settings.values["input.max_file_mb"]
        .as_u64()
        .ok_or("DICTIONARY_SETTING_MISSING")?
        * 1024
        * 1024;
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0u64;
    for digest in base
        .documents
        .iter()
        .flat_map(|document| &document.archives)
        .flat_map(|archive| &archive.asset_digests)
    {
        if seen.insert(digest.clone()) {
            total = total
                .checked_add(store.asset(digest, cap)?.len() as u64)
                .ok_or("DICTIONARY_ARCHIVE_LIMIT")?;
        }
    }
    if total > cap {
        return Err("DICTIONARY_ARCHIVE_LIMIT".into());
    }
    let mut child = base.clone();
    let mut assets = Vec::new();
    for document in &mut child.documents {
        let (enriched, responses) = enrich_document(document, &settings, dictionary)?;
        for bytes in responses {
            if seen.insert(canonical::asset_digest(&bytes)) {
                total = total
                    .checked_add(bytes.len() as u64)
                    .ok_or("DICTIONARY_ARCHIVE_LIMIT")?;
                if total > cap {
                    return Err("DICTIONARY_ARCHIVE_LIMIT".into());
                }
                assets.push(bytes);
            }
        }
        *document = enriched;
    }
    if total > cap {
        return Err("DICTIONARY_ARCHIVE_LIMIT".into());
    }
    child.revision = base
        .revision
        .checked_add(1)
        .ok_or("DICTIONARY_REVISION_LIMIT")?;
    child.parent_digest = Some(base.approval_digest().map_err(|e| e.to_string())?);
    child.binding = None;
    child.rendered.clear();
    let sources: Vec<_> = child
        .documents
        .iter()
        .flat_map(|document| &document.sources)
        .collect();
    child.source_digest =
        canonical::digest("source-capture", &sources).map_err(|e| e.to_string())?;
    child.approval_digest().map_err(|e| e.to_string())?;
    for bytes in assets {
        store.publish_asset(&bytes, cap)?;
    }
    store.publish_revision(&child)?;
    Ok(child)
}

/// Stage enrichment atomically: provider failure leaves the caller's document unchanged.
/// Returned response bytes must be retained before publishing the enriched revision.
pub fn enrich_document(
    document: &LearningDocument,
    settings: &Effective,
    dictionary: Option<&dyn crate::DictionaryPort>,
) -> Result<(LearningDocument, Vec<Vec<u8>>), String> {
    validate_settings(settings)?;
    let mut provider_assets = Vec::new();
    let mut document = document.clone();
    if settings.values["dictionary.provider"] != "authored" {
        if let LearningContent::Vocabulary(vocab) = &mut document.content {
            let japanese = document.target_language.as_str().split('-').next() == Some("ja");
            let english = document.target_language.as_str().split('-').next() == Some("en");
            if !(japanese
                && matches!(
                    settings.values["dictionary.provider"].as_str(),
                    Some("jisho" | "auto")
                )
                || english
                    && matches!(
                        settings.values["dictionary.provider"].as_str(),
                        Some("wiktionary" | "auto")
                    ))
            {
                return Err(
                    "CAPABILITY_UNAVAILABLE: this dictionary/language adapter is not implemented"
                        .into(),
                );
            }
            if document.explanation_language.as_str().split('-').next() != Some("en") {
                return Err(
                    "CAPABILITY_UNAVAILABLE: dictionary definition translation is not implemented"
                        .into(),
                );
            }
            if vocab.expression.trim().is_empty() {
                return Err("VOCAB_EXPRESSION_REQUIRED".into());
            }
            let page = if let Some(provider) = dictionary {
                provider.lookup(&vocab.expression, &document.target_language)?
            } else {
                let client = linguist_dictionary::transport::DictionaryClient::for_target(
                    settings,
                    &document.target_language,
                )
                .map_err(|e| format!("CAPABILITY_UNAVAILABLE: {e}"))?;
                client
                    .lookup(&vocab.expression, &document.target_language, 1000)
                    .map_err(|e| format!("DICTIONARY_PROVIDER_FAILED: {e}"))?
            };
            if page.query != vocab.expression
                || canonical::asset_digest(&page.raw_bytes) != page.raw_digest
            {
                return Err("DICTIONARY_RESPONSE_CONFLICT".into());
            }
            // Reparse port output: callers cannot fabricate rich facts unrelated to saved bytes.
            let maximum =
                settings.values["network.max_response_mb"].as_u64().unwrap() * 1024 * 1024;
            let verified = if japanese {
                linguist_dictionary::parse_jisho(
                    &page.query,
                    &document.target_language,
                    &page.raw_bytes,
                    maximum,
                    1000,
                )
            } else {
                linguist_dictionary::wiktionary::parse_definition(
                    &page.query,
                    &document.target_language,
                    &page.raw_bytes,
                    maximum,
                    1000,
                )
            }
            .map_err(|e| format!("DICTIONARY_PROVIDER_FAILED: {e}"))?;
            if verified.entries != page.entries
                || verified.request_url != page.request_url
                || verified.exact_matches != page.exact_matches
            {
                return Err("DICTIONARY_RESPONSE_CONFLICT".into());
            }
            vocab.dictionary = page.entries;
            let source_id = uuid::Uuid::new_v4();
            let fields = BTreeMap::from([(
                "provider_response".into(),
                String::from_utf8(page.raw_bytes.clone()).map_err(|_| "INPUT_ENCODING")?,
            )]);
            document.sources.push(SourceRecord {
                id: source_id,
                kind: if japanese {
                    "jisho_api_v1"
                } else {
                    "wiktionary_definition_v0.8"
                }
                .into(),
                location: page.request_url.clone(),
                digest: page.raw_digest.clone(),
                text: fields.get("provider_response").cloned(),
                fields: fields.clone(),
                model_manifest: if japanese {
                    "jisho-api-v1"
                } else {
                    "wiktionary-definition-v0.8"
                }
                .into(),
                template_manifest: None,
                captured_at_unix_seconds: None,
                tags: vec![],
                cards: vec![],
                media_refs: vec![],
            });
            document.archives.push(SourceArchive {
                id: uuid::Uuid::new_v4(),
                source_id,
                digest: page.raw_digest.clone(),
                original_text: fields.get("provider_response").cloned(),
                original_fields: fields,
                asset_digests: vec![page.raw_digest.clone()],
            });
            for (entry_index, entry) in vocab.dictionary.iter().enumerate() {
                for (sense_index, sense) in entry.senses.iter().enumerate() {
                    document.evidence.push(Evidence {
                        id: uuid::Uuid::new_v4(),
                        field: "meaning".into(),
                        provenance: Provenance::Dictionary,
                        source_id: Some(source_id),
                        region_id: None,
                        target: Some(EvidenceTarget::DictionarySense {
                            entry_index,
                            sense_index,
                        }),
                        source_span: None,
                        language: "en".to_owned().try_into()?,
                        claim: sense.definitions.join("; "),
                        source_url: Some(entry.source_url.clone()),
                        ambiguous: vocab.dictionary.len() > 1 || entry.senses.len() > 1,
                    });
                }
            }
            if vocab.dictionary.is_empty() {
                let mut issue = Issue::new(
                    "DICTIONARY_NOT_FOUND",
                    Severity::Warning,
                    Some("dictionary"),
                    "No dictionary entry was found; Existing content remains distinct from dictionary facts.",
                );
                issue.stage = "dictionary".into();
                document.issues.push(issue);
            }
            provider_assets.push(page.raw_bytes);
        } else {
            return Err(
                "CAPABILITY_UNAVAILABLE: grammar preparation requires dictionary.provider=authored"
                    .into(),
            );
        }
    }
    document.issues = validation::validate(&document);
    Ok((document, provider_assets))
}
