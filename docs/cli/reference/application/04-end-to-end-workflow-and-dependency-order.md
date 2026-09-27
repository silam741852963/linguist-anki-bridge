# Application specification reference

## 4. End-to-end workflow and dependency order

```mermaid
flowchart TD
    A[Command and configuration] --> B[Validate input and select source notes]
    B --> C[Exact duplicate resolution]
    C --> D[Capture source fields and media evidence]
    D --> E[OCR and classify source images]
    E --> F[Resolve expression and dictionary evidence]
    F --> G[Ollama nuances and examples]
    F --> H[Kanji information]
    F --> I[Preserve or retrieve internet picture]
    F --> J[Preserve or obtain pronunciation audio]
    G --> K[Canonical document and durable plan]
    H --> K
    I --> K
    J --> K
    K --> L[Inspect, edit, validate, and approve plan]
    L --> M[Apply with conflict checks and snapshots]
    M --> N[Verify collection state and save receipt]
    N --> O[Conflict-aware restore]
```

**REQUIRED invariants:** selection and enrichment do not mutate Anki; dictionaries own definitions and readings; OCR is evidence, not an authoritative replacement for definitions; LLM vocabulary output owns only nuances/examples; user edits remain distinguishable; uncertain pictures are preserved; every mutation has durable recovery evidence; previewed and applied values must agree.

After dictionary resolution, generation, Kanji, new-image search, and audio work may run concurrently with bounded service limits. Existing-image OCR must finish before generation that depends on it. A user's edit to expression, dictionary sense, context, prompt, voice, or image policy invalidates the affected downstream stages rather than every stage indiscriminately.
