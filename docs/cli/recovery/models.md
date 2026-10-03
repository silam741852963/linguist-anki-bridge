# Write and recovery algorithm

## ALG-MODEL — model installation/change

1. Compute fixed model field/template/CSS manifest and explicit old-task→new-task mapping. Inspect existing models by name and actual hash.
2. If an identical managed model exists, reuse. If name exists with different content, block or create a separately reviewed versioned name; never overwrite unknown shared content.
3. Journal model creation before calling Anki. Reconcile lost response by name+exact manifest+operation evidence; name alone is insufficient.
4. Verify actual fields/order/templates/CSS. A partial model needs recovery before notes use it. Never delete a shared model as automatic compensation. Schema migration and card generation require tested native adapter behavior.

Implemented orchestration (WP-10): `linguist_application::model_install` follows these steps over an injected native port, with a verified collection checkpoint required before creation and a pre-state record plus journal intent written before the call. See [OP-17](../operations/collection.md#op-17-models-install-purpose) for behavior and the [WP-10 known limits](../implementation/wp-10.md#known-limits-revisit-after-all-packages). No live native `install_model` adapter exists.
