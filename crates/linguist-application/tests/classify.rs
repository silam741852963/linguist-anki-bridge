//! WP-27 `plans classify`: a fake agent classifies an applied note, the plan
//! gets a revision that differs only in the profile, and once that revision
//! is approved `notes retag` writes the profile's tags under its approval.
mod common;
use common::*;
use linguist_application::classify::{ClassifyRequest, classify_plan, compact, profile_only};
use linguist_application::retag::{
    RetagRequest, Revisions, Verdict, latest_by_note, propose, retag,
};
use linguist_config::{ConfigFile, Effective, Registry, ResolveOptions, resolve};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

const LATER: u64 = 1_200_000;

/// A fake `claude` that answers with `reply` as its structured output.
fn agent(s: &Setup, reply: Value) -> (Effective, BTreeMap<String, String>) {
    let bin = s.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let body = json!({"type":"result","subtype":"success","is_error":false,
        "structured_output": reply, "modelUsage":{"fake-model":{}}});
    std::fs::write(
        bin.join("claude"),
        format!(
            "#!/bin/sh\n[ \"$1\" = --version ] && {{ echo '9.9.9 (Fake Code)'; exit 0; }}\ncat > '{}'\nprintf '%s' '{}'\n",
            bin.join("prompt.txt").display(),
            body
        ),
    )
    .unwrap();
    std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let environment = BTreeMap::from([
        ("HOME".into(), s.root.to_str().unwrap().into()),
        ("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())),
    ]);
    let mut settings = resolve(
        &Registry::builtin(),
        &ConfigFile::default(),
        &ResolveOptions::default(),
    )
    .unwrap();
    for (key, value) in [
        ("storage.state_dir", json!(s.root)),
        ("llm.enabled", json!(true)),
        ("llm.provider", json!("claude_code")),
        ("llm.fallback", json!([])),
    ] {
        settings.values.insert(key.into(), value);
    }
    (settings, environment)
}

fn leaf() -> String {
    linguist_core::profile::taxonomy().leaves[0].clone()
}

fn reply(s: &Setup, domain: &str) -> Value {
    json!({"profiles": [{"id": s.item.to_string(), "domain": domain, "domains_secondary": [],
        "functions": ["not::for-vocabulary"], "register": [], "connotation": "neutral",
        "transitivity_partner": ""}]})
}

fn applied(s: &mut Setup, anki: &mut Anki) {
    let request = s.request();
    let outcome = apply_item(&mut s.store, &s.token, anki, &request).unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
}

fn approve(s: &mut Setup, revision: u32, digest: &str) -> Uuid {
    let plan = s.store.revision(s.plan.id, revision).unwrap();
    let warnings: Vec<String> = linguist_core::plan_validation::inspect(&plan)
        .unwrap()
        .items
        .iter()
        .flat_map(|item| item.issues.iter())
        .filter(|i| i.severity == linguist_core::Severity::Warning)
        .map(|i| i.code.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    s.store
        .approve_revision(&linguist_core::approval::ApprovalRequest {
            plan_id: s.plan.id,
            revision,
            digest: digest.into(),
            item_ids: None,
            actor: "reviewer".into(),
            accepted_warnings: warnings,
        })
        .unwrap()
        .unwrap()
        .id
}

#[test]
fn a_classified_profile_reaches_the_note_through_its_approval() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    applied(&mut s, &mut anki);
    let (settings, environment) = agent(&s, reply(&s, &leaf()));
    let base = s.store.revision(s.plan.id, 1).unwrap();
    let result = classify_plan(
        &mut s.store,
        &base,
        s.digest,
        &ClassifyRequest {
            settings: &settings,
            environment: &environment,
            batch: 25,
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(result["classified"], 1, "{result}");
    assert_eq!(result["revision"], 2);
    // The agent saw the compact note, not the whole document.
    let prompt = std::fs::read_to_string(s.root.join("bin/prompt.txt")).unwrap();
    assert!(prompt.contains(&s.item.to_string()));
    let child = s.store.revision(s.plan.id, 2).unwrap();
    let document = &child.documents[0];
    assert!(profile_only(&base.documents[0], document));
    let linguist_core::LearningContent::Vocabulary(v) = &document.content else {
        panic!("vocabulary")
    };
    let profile = v.profile.as_ref().unwrap();
    assert_eq!(profile.domain, leaf());
    // Grammar functions on a vocabulary note are dropped; the rest is kept.
    assert!(profile.functions.is_empty());
    assert_eq!(profile.connotation, "neutral");
    assert_eq!(compact(document)["kind"], "vocabulary");

    // Unapproved, the classified revision is not what a retag renders.
    let latest = latest_by_note(&s.store).unwrap();
    let before = propose(
        &s.store,
        &mut anki,
        &mut Revisions::default(),
        10,
        latest.get(&10),
    )
    .unwrap();
    assert!(matches!(before.verdict, Verdict::Current), "{before:?}");

    let approval = approve(&mut s, 2, result["digest"].as_str().unwrap());
    let change = propose(
        &s.store,
        &mut anki,
        &mut Revisions::default(),
        10,
        latest.get(&10),
    )
    .unwrap();
    let domain_tag = format!("lab::domain::{}", leaf());
    match &change.verdict {
        Verdict::Change { tags_added, .. } => {
            assert!(tags_added.contains(&domain_tag), "{tags_added:?}")
        }
        other => panic!("{other:?}"),
    }
    let note = anki.notes[&10].clone();
    let checkpoint = checkpoint_for(&mut s, &[&note], &[], LATER);
    let outcome = retag(
        &mut s.store,
        &s.token,
        &mut anki,
        &change,
        &RetagRequest {
            checkpoint_id: checkpoint,
            group_id: None,
            protected_manifest_digest: "protected-v1",
            reuse_max_age_seconds: 600,
            max_package_bytes: 4 * 1024 * 1024,
            now_ms: LATER + 1000,
        },
    )
    .unwrap();
    assert_eq!(
        outcome.state,
        OperationState::Committed,
        "{:?}",
        outcome.issues
    );
    assert!(anki.notes[&10].tags.contains(&domain_tag));
    let now = latest_by_note(&s.store).unwrap()[&10].clone().unwrap();
    assert_eq!((now.revision, now.approval_id), (2, approval));
}

#[test]
fn a_reply_outside_the_taxonomy_publishes_nothing() {
    let mut s = setup(Kind::Update);
    let (settings, environment) = agent(&s, reply(&s, "made::up::domain"));
    let base = s.store.revision(s.plan.id, 1).unwrap();
    let request = ClassifyRequest {
        settings: &settings,
        environment: &environment,
        batch: 25,
        limit: None,
    };
    let result = classify_plan(&mut s.store, &base, s.digest, &request).unwrap();
    assert_eq!(result["classified"], 0, "{result}");
    assert_eq!(s.store.latest_revision(s.plan.id).unwrap(), 1);
    // A stale base is refused before any call.
    let error = classify_plan(&mut s.store, &base, "other", &request).unwrap_err();
    assert!(error.starts_with("CLASSIFY_BASE_CONFLICT"), "{error}");
}

#[test]
fn review_decisions_follow_a_classified_document_to_its_new_digest() {
    let mut s = setup(Kind::Update);
    let mut anki = Anki::new(&s, Kind::Update);
    applied(&mut s, &mut anki);
    let (settings, environment) = agent(&s, reply(&s, &leaf()));
    // A base revision holding one decision on the document.
    let mut base = s.store.revision(s.plan.id, 1).unwrap();
    base.revision = 2;
    base.parent_digest = Some(s.digest.into());
    base.binding = None;
    let before = base.documents[0].semantic_digest().unwrap();
    let decision = linguist_core::records::ReviewDecision {
        id: Uuid::new_v4(),
        issue_id: format!("DICTIONARY_SENSE_REVIEW:{}", base.documents[0].id),
        input_digest: before.clone(),
        actor: "campaign-policy".into(),
        created_at: "unix-seconds:1".into(),
        choice: linguist_core::records::ReviewChoice::Sense("k".into()),
    };
    base.review_decisions.push(decision.clone());
    // The document's own review record, which readiness checks.
    base.documents[0].reviews.push(decision);
    let before = base.documents[0].semantic_digest().unwrap();
    base.review_decisions[0].input_digest = before.clone();
    base.documents[0].reviews[0].input_digest = before.clone();
    let digest = s.store.publish_revision(&base).unwrap();
    let result = classify_plan(
        &mut s.store,
        &base,
        &digest,
        &ClassifyRequest {
            settings: &settings,
            environment: &environment,
            batch: 25,
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(result["revision"], 3, "{result}");
    let child = s.store.revision(s.plan.id, 3).unwrap();
    let after = child.documents[0].semantic_digest().unwrap();
    assert_ne!(before, after);
    assert_eq!(child.review_decisions.len(), 1);
    assert_eq!(child.review_decisions[0].input_digest, after);
    assert_eq!(child.review_decisions[0].actor, "campaign-policy");
    assert_eq!(child.documents[0].reviews[0].input_digest, after);
    // Still a profile-only change for `notes retag`.
    assert!(profile_only(&base.documents[0], &child.documents[0]));
    // The staged render was made after the reviews moved.
    let evidence = linguist_core::plan_validation::inspect(&child).unwrap();
    let codes: Vec<&str> = evidence
        .items
        .iter()
        .flat_map(|item| item.issues.iter().map(|i| i.code.as_str()))
        .collect();
    assert!(!codes.contains(&"STAGED_RENDER_MISMATCH"), "{codes:?}");
}
