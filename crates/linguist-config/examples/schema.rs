use linguist_config::{Registry, schema};

fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/v2/config-file-normalized.schema.json");
    let value = schema::normalized_file(&Registry::builtin());
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
}
