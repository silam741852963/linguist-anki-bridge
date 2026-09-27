# Preparation pipeline

## ALG-PROVIDER — external service boundary

1. Resolve purpose-specific adapter, endpoint, credentials by reference, offline rules, resource availability, deadlines and concurrency limits.
2. Build request from trusted instructions plus separately delimited untrusted source. Never use an image/source URL as a shell argument without safe process argument APIs. Deny local/private network image URLs unless explicitly configured for that provider.
3. Bound body bytes, redirects, total duration and decoded assets. Validate MIME/content, schema and provider language. Preserve rich evidence instead of flattening to strings.
4. Retry only classified transient read failures with bounded jitter/backoff and Retry-After. Authentication/config/schema errors are not automatic retry candidates. Cancellation kills child processes and cleans temporary derivatives.
5. Store cache/results with provider, version, request/resource fingerprint and provenance. Log IDs/timings, not private prompts or secrets by default. Return typed errors/issues for the item.
