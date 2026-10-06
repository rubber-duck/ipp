//! Immutable expression definitions shared by otherwise independent consumers.
//! Consumer bindings, parameters and scratch remain outside the asset.

use crate::expressions::{ExpressionCodecError, ExpressionDeclaration, PreparedExpression};
use crate::services::asset_management::{Asset, AssetLoader, AssetTypeId, AsyncAssetLoader};
use std::any::Any;

/// Generic CPU expression definition, independent of the consuming System.
pub const EXPRESSION_TYPE: AssetTypeId = AssetTypeId(19);

/// Validated logical declaration and its shared, reconstructible instruction plan.
#[derive(Debug)]
pub struct ExpressionAsset {
    declaration: ExpressionDeclaration,
    prepared: PreparedExpression,
    resident_bytes: usize,
}

impl ExpressionAsset {
    /// Decode the existing bounded IPPE format and prepare through the shared evaluator.
    pub fn decode(bytes: &[u8]) -> Result<Self, ExpressionCodecError> {
        let declaration = ExpressionDeclaration::decode(bytes)?;
        let prepared = PreparedExpression::prepare(&declaration)?;
        let resident_bytes = crate::expressions::resident_bytes(&declaration, &prepared);

        Ok(Self {
            declaration,
            prepared,
            resident_bytes,
        })
    }

    /// Immutable authored graph, without source handles or consumer bindings.
    pub fn declaration(&self) -> &ExpressionDeclaration {
        &self.declaration
    }

    /// Borrow the plan for evaluation or clone it to share its immutable allocations.
    /// Consumers own scratch and invalidate their plan access at the asset lifecycle boundary.
    pub fn prepared(&self) -> &PreparedExpression {
        &self.prepared
    }
}

impl Asset for ExpressionAsset {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        self
    }

    /// Retained payload storage: vector capacities, plan slices, owned input names
    /// and distinct text allocations. Text shared by the graph and plan counts once.
    /// Excludes the stable object, Arc control blocks and allocator metadata, encoded
    /// source storage owned by I/O, and consumer-owned plan clones/scratch.
    fn resident_bytes(&self) -> usize {
        self.resident_bytes
    }
}

/// Decode and prepare directly from borrowed windows into private CPU storage.
pub fn expression_asset_loader() -> impl AssetLoader<Data = ExpressionAsset> {
    AsyncAssetLoader::decode(|mut reader| async move {
        let declaration = ExpressionDeclaration::decode_reader(&mut *reader).await?;
        let prepared = PreparedExpression::prepare_async(&declaration)
            .await
            .map_err(|error| format!("Invalid expression declaration: {error:?}"))?;
        let resident_bytes = crate::expressions::resident_bytes(&declaration, &prepared);
        Ok(ExpressionAsset {
            declaration,
            prepared,
            resident_bytes,
        })
    })
}

#[cfg(test)]
#[path = "expression_tests.rs"]
mod tests;
