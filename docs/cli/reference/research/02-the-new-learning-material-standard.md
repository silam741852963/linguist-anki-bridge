# Research reference

## 2. The new learning-material standard

Use **LearningDocument v2**, a typed vocabulary-or-grammar domain object. Existing `CardDocument` v1 remains a losslessly imported/exported legacy boundary where representable. Do not pretend the current five-value struct can store a grammar corpus, rich senses, exercise variants and provenance without extension/versioning.

Shared properties: stable document ID; kind; BCP-47 target/explanation languages; source records; immutable raw source archive; learning units; structured evidence; user edits; generated annotations; selected exercise; media manifest; validation issues with severity; output model/schema version; operation/plan IDs. Grammar and vocabulary have separate required properties, while pipeline, plans, review, apply and recovery are shared.

Keep full reference evidence in local structured artifacts and the back-side reference section. Keep the primary recall task short. This is an application of SuperMemo's guidance on focused items, context, cloze, and source attribution; it is not a claim that one template is universally optimal. [SuperMemo's formulation guidance](https://www.supermemo.com/en/blog/twenty-rules-of-formulating-knowledge).


Subsections:

- [2.1 Vocabulary standard](02-01-21-vocabulary-standard.md)
- [2.2 Grammar standard](02-02-22-grammar-standard.md)
- [2.3 What makes a result ready](02-03-23-what-makes-a-result-ready.md)
