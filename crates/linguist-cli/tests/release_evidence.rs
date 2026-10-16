//! Release records stay consistent with the code: the operation coverage
//! record lists an existing command-level test for every operation. Gate
//! evidence and traceability records are checked on the `cli-dev` branch.
use std::path::Path;

#[test]
fn operation_coverage_lists_existing_tests_for_all_86_operations() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text =
        std::fs::read_to_string(repo.join("docs/cli/implementation/op-coverage.md")).unwrap();
    let rows: Vec<&str> = text.lines().filter(|l| l.starts_with("| OP-")).collect();
    assert_eq!(rows.len(), 86);
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
    assert!(count >= 86, "{count}");
}
