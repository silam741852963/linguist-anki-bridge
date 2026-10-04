//! Release-gate records stay consistent: every gate status in
//! docs/cli/decisions/release-gates.json other than `not_run` points at a
//! GateEvidence record that parses, passes `GateEvidence::validate`, carries
//! the same status, and references artifacts that exist. A `pass` needs
//! successful commands and assertions by construction of the contract.
use linguist_core::records::{GateEvidence, GateStatus};
use std::path::Path;

#[test]
fn gate_registry_and_evidence_records_agree() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo.join("docs/cli/decisions/release-gates.json")).unwrap(),
    )
    .unwrap();
    let gates = registry["gates"].as_array().unwrap();
    assert_eq!(gates.len(), 14);
    for gate in gates {
        let id = gate["id"].as_str().unwrap();
        let status = gate["status"].as_str().unwrap();
        if status == "not_run" {
            assert!(gate.get("evidence_paths").is_none(), "{id}");
            continue;
        }
        let paths = gate["evidence_paths"].as_array().expect(id);
        assert_eq!(paths.len(), 1, "{id}");
        let path = repo.join(paths[0].as_str().unwrap());
        let evidence: GateEvidence =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        evidence
            .validate()
            .unwrap_or_else(|e| panic!("{id}: {e:?}"));
        assert_eq!(evidence.gate_id, id);
        let expected = match status {
            "pass" => GateStatus::Pass,
            "fail" => GateStatus::Fail,
            "blocked" => GateStatus::Blocked,
            other => panic!("{id}: {other}"),
        };
        assert_eq!(evidence.status, expected, "{id}");
        assert_eq!(
            evidence.failure_code.as_deref(),
            gate.get("failure_code").and_then(|c| c.as_str()),
            "{id}"
        );
        for artifact in &evidence.artifact_refs {
            assert!(repo.join(artifact).is_file(), "{id}: missing {artifact}");
        }
        // A blocked gate never hides a failing command.
        if evidence.status == GateStatus::Blocked {
            assert!(
                evidence.commands.iter().all(|c| c.exit_code == 0),
                "{id}: blocked with a failing command"
            );
        }
    }
}

#[test]
fn operation_coverage_lists_existing_tests_for_all_61_operations() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text =
        std::fs::read_to_string(repo.join("docs/cli/implementation/op-coverage.md")).unwrap();
    let rows: Vec<&str> = text.lines().filter(|l| l.starts_with("| OP-")).collect();
    assert_eq!(rows.len(), 61);
    for (index, row) in rows.iter().enumerate() {
        assert!(
            row.starts_with(&format!("| OP-{:02} |", index + 1)),
            "{row}"
        );
        let cells: Vec<&str> = row.split(" | ").collect();
        assert!(cells[3].contains("::"), "no command test: {row}");
    }
    let listed = text
        .split("```text")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let mut count = 0;
    for entry in listed.lines().filter(|l| !l.trim().is_empty()) {
        let (file, function) = entry.split_once("::").unwrap();
        let source = std::fs::read_to_string(repo.join(file)).unwrap_or_default();
        assert!(
            source.contains(&format!("fn {function}()")),
            "listed test does not exist: {entry}"
        );
        count += 1;
    }
    assert!(count >= 61, "{count}");
}
