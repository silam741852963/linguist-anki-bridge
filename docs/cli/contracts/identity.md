# Domain contracts

## Identity and canonical hashes

`CollectionBinding`: endpoint/profile/path fingerprint, bridge installation and sidecar lineage UUIDs, capability/version manifest, collection-session epoch and verification time. Confidence is `strong|manifest_verified|weak`: strong requires verified native session plus matching source/target manifests; manifest_verified and weak permit read/prepare/export only. All managed writes require the selected bridge and current strong execution binding. Session epoch is recorded separately from stable approval identity; explicit ResumeBindingDecision handles restart without regenerating content. No UUID/profile fingerprint is claimed immune to collection replacement. See [final native contract](../decisions/native-bridge.md).

`ContentDigest`: SHA-256 over RFC 8785 JCS bytes of the versioned semantic projection: raw UTF-8 strings, retained array order, explicit intent variants and no volatile execution timestamps. Store format identifier `lab-jcs-v1`; old formats cannot be silently compared. Anki IDs serialize as decimal strings and parameter numbers must be finite. See [wire/hash contract](../decisions/storage-and-wire.md). Hash file bytes separately. Normalize only semantic identity text; never normalize saved source fields/media before conflict comparison. Base64 decode then hash media bytes.

Creation marker contract: add exactly one reserved `lab::operation::<uuid>` tag, retaining user tags. The UUID is assigned and journaled before dispatch; it is not configurable, secret, or proof of identity by itself. Notes created for split siblings have distinct child UUIDs. Existing unrelated notes must not be adopted merely because a user copied the marker.

`NoteIdentity`: collection binding + note ID for existing notes; operation UUID for creation. Vocabulary semantic identity: target language + normalized expression + optional reviewed sense key. Grammar semantic identity: target language + canonical pattern + reviewed use key. Semantic candidates do not override physical note identity.
