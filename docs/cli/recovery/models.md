# Write and recovery algorithm

## ALG-MODEL — model installation/change

1. Compute fixed model field/template/CSS manifest and explicit old-task→new-task mapping. Inspect existing models by name and actual hash.
2. If an identical managed model exists, reuse. If name exists with different content, block or create a separately reviewed versioned name; never overwrite unknown shared content.
3. Journal model creation before calling Anki. Reconcile lost response by name+exact manifest+operation evidence; name alone is insufficient.
4. Verify actual fields/order/templates/CSS. A partial model needs recovery before notes use it. Never delete a shared model as automatic compensation. Schema migration and card generation require tested native adapter behavior.
