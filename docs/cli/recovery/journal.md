# Write and recovery algorithm

## Journal state and step protocol

Operation states: prepared → preflight → checkpointed → mutating → verifying → committed. Alternate exits: failed_before_write, needs_recovery, compensated, restored. Each external step has intent_recorded → request_started → observed_success/observed_failure/unknown → verified. Durable request_started precedes dispatch. Unknown survives restart. AnkiConnect multi is not a transaction.

Persist intent, preconditions and expected postconditions before *each* effect. Local durability failure after any external call stops writing immediately. Never mark an operation failed_before_write merely because its receipt is absent.
