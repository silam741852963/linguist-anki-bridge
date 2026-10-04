fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/cli/configuration/setting-coverage.md");
    std::fs::write(path, linguist_config::coverage::markdown()).unwrap();
}
