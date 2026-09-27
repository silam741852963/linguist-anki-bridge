use std::{env, fs, path::Path};
fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/cli/configuration");
    let index_path = root.join("settings-registry.json");
    println!("cargo:rerun-if-changed={}", index_path.display());
    let index: serde_json::Value = serde_json::from_slice(&fs::read(index_path).unwrap()).unwrap();
    let mut entries = Vec::new();
    for group in index["groups"].as_array().unwrap() {
        let path = root.join(group["path"].as_str().unwrap());
        println!("cargo:rerun-if-changed={}", path.display());
        let data: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let group_entries = data["entries"].as_array().unwrap();
        assert_eq!(group_entries.len() as u64, group["count"].as_u64().unwrap());
        entries.extend(group_entries.iter().cloned());
    }
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("registry.json"),
        serde_json::to_vec(&entries).unwrap(),
    )
    .unwrap();
}
