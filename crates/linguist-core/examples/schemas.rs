//! Generate the checked-in schemas from the domain types: cargo run -p linguist-core --example schemas.
use linguist_core::{LearningDocument, records::*, render::RenderedNote};
use schemars::{JsonSchema, schema_for};
fn write<T: JsonSchema>(name: &str) {
    let schema = schema_for!(T);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/v2");
    std::fs::write(
        root.join(format!("{name}.schema.json")),
        format!("{}\n", serde_json::to_string_pretty(&schema).unwrap()),
    )
    .unwrap();
}
fn main() {
    write::<linguist_core::model::ModelComparison>("model-comparison");
    write::<SourceTaskMap>("source-task-map");
    write::<linguist_core::editing::PlanPatch>("plan-patch");
    write::<LearningDocument>("learning-document");
    write::<RenderedNote>("rendered-note");
    write::<PlanRevision>("plan-revision");
    write::<linguist_core::plan_validation::ValidationEvidence>("validation-evidence");
    write::<linguist_core::approval::ApprovalRequest>("approval-request");
    write::<linguist_core::review::ResolutionRequest>("resolution-request");
    write::<linguist_core::review::ResolutionBatch>("resolution-batch");
    write::<Approval>("approval");
    write::<OperationJournal>("operation-journal");
    write::<NativeOperationReceipt>("native-operation-receipt");
    write::<ResumeBindingDecision>("resume-binding-decision");
    write::<GateEvidence>("gate-evidence");
    write::<CapabilityReport>("capability-report");
    write::<Snapshot>("snapshot");
    write::<BackupReceipt>("backup-receipt");
    write::<Job>("job");
    write::<ResolvedSettings>("resolved-settings");
    write::<CollectionBinding>("collection-binding");
}
