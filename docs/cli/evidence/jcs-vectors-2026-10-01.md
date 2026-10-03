# Cross-language JCS vectors

On 2026-10-01, the checked-in five-vector set was generated with ECMAScript `JSON.stringify` and UTF-16 key sorting, then independently checked against Rust `serde_jcs` and the `lab-jcs-v1` domain-separated SHA-256 calculation.

```sh
node scripts/verify-jcs-vectors.mjs
cargo test --locked -p linguist-core --test domain canonical_bytes_and_domain_hash_match_ecmascript_vectors
```

Both passed. The vectors cover object ordering, supplementary-plane versus BMP key ordering, number formatting at exponent thresholds and negative zero, string escaping, and nested array order. They are saved in `contracts/v2/fixtures/jcs-vectors.json` with exact canonical text and `test-vector` domain digests.

This verifies these cross-language examples only. It does not certify all RFC 8785 edge cases, a Python companion implementation, stored-plan compatibility across a future hash-format change, or the entire EV-01 release gate.
