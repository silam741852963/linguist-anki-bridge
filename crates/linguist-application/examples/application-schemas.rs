fn write<T: schemars::JsonSchema>(name: &str) {
    let schema = schemars::schema_for!(T);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../contracts/v2/{name}.schema.json"));
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&schema).unwrap()),
    )
    .unwrap();
}
fn main() {
    write::<linguist_application::mapping::FieldMapping>("source-field-mapping");
    write::<linguist_application::generation::Supplement>("generation-supplement");
    let export = schemars::schema_for!(linguist_application::export::ExportManifest);
    let export_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/v2/export-manifest.schema.json");
    std::fs::write(
        export_path,
        format!("{}\n", serde_json::to_string_pretty(&export).unwrap()),
    )
    .unwrap();
    let schema = schemars::schema_for!(linguist_application::AddInput);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/v2/add-input.schema.json");
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&schema).unwrap()),
    )
    .unwrap();
}
