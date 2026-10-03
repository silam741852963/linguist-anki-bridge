//! Exact selection checks for read commands. IDs are decimal strings on the v6 wire.
use std::collections::BTreeSet;

pub fn require_explicit_matches(requested: &[String], found: &[String]) -> Result<(), String> {
    if requested.is_empty() {
        return Ok(());
    }
    let requested_set: BTreeSet<_> = requested.iter().collect();
    let found_set: BTreeSet<_> = found.iter().collect();
    if requested_set.len() != requested.len() {
        return Err("NOTE_SELECTOR_DUPLICATE_ID".into());
    }
    if found_set != requested_set || found_set.len() != found.len() {
        return Err("NOTE_SELECTOR_MISSING_OR_CHANGED_ID".into());
    }
    Ok(())
}
