# Write and recovery algorithm

## Journal state and step protocol

Operation states: prepared → preflight → checkpointed → mutating → verifying → committed. Alternate exits: failed_before_write, needs_recovery, compensated, restored. Each external step has intent_recorded → request_started → observed_success/observed_failure/unknown → verified. Durable request_started precedes dispatch. Unknown survives restart. AnkiConnect multi is not a transaction.

Persist intent, preconditions and expected postconditions before *each* effect. Local durability failure after any external call stops writing immediately. Never mark an operation failed_before_write merely because its receipt is absent.

Implemented store rules: `committed` requires every step `verified`. `failed_before_write` allows only steps still at `intent_recorded` or at an explicit `observed_failure`, which callers record only with evidence of no collection effect. Reconciliation may move an `unknown` step to `observed_failure` (and the operation from `needs_recovery` to `failed_before_write`) only on such evidence, or to `verified`. Since WP-12, `restored` finalizes a `committed` or `needs_recovery` operation only when a committed restore journal with a verified restore receipt names it; the step evidence must be unchanged and no step may be `request_started` or `unknown`. `compensated` remains unavailable.
