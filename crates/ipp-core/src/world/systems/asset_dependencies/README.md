# World asset dependencies

`AssetDependencySystem` tracks one World's authored resource references, including hidden overlay values and animation demand. Ordered mutations update demand from their applied effects even when an operation fails. Restoration rebuilds the index from component state.

The Host's shared [asset service](../../../services/asset_management/README.md) owns acquisition and aggregates consumers across Worlds. Accepted demand becomes available to subsequent service progression; a World neither loads resources independently nor completes the shared release barrier. Upload receipts wait for the relevant attempt to settle, including pending release/retry.

Source entrypoints: [System participation](system.rs), [authored demand](demand.rs), [upload and receipt handling](access.rs), and [retained World state](system_state.rs). The reusable [Host test driver](../../../../tests/support/mod.rs) exercises real service progression and invalidation; maintained lifecycle and geometry suites add native/browser transport coverage.
