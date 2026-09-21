//! Bounded GUI semantic snapshots and actions over the host boundary.
//!
//! Snapshots read the authoritative inspect state, the retained evaluated
//! view and the observed input focus through the normal boundary; actions
//! resolve against a fresh bounded snapshot and dispatch through the same
//! validated command and input policies as any other writer. Refusals
//! return as correlated host rejections, never as fabricated state.

use super::{HostServices, WorldSessionContext, WorldSessionReply};
use ipp_core::{
    GuiSemanticActionError, GuiSemanticActionRequest, GuiSemanticSnapshotQuery, GuiSemanticTree,
};

impl<P: HostServices> WorldSessionContext<'_, P> {
    /// Bounded lifetime/revision-fenced semantic snapshot for one panel.
    pub fn gui_semantic_snapshot(
        &self,
        query: &GuiSemanticSnapshotQuery,
    ) -> Result<GuiSemanticTree, String> {
        self.world
            .gui_semantic_snapshot(query.entity, query.max_depth, query.limit)
    }

    /// Resolve one semantic action against a fresh bounded snapshot and
    /// dispatch it through the validated control policy.
    ///
    /// The snapshot bounds match inspect maxima so the addressed node is
    /// always resolvable when present. Lifetime mismatches refuse like
    /// unknown nodes; stale revisions refuse instead of overwriting. Every
    /// action queues as one correlated input-system command.
    pub(crate) fn resolve_semantic_action(
        &mut self,
        request_id: u64,
        request: &GuiSemanticActionRequest,
    ) -> WorldSessionReply {
        let session = self.session.id;
        let tree = match self
            .world
            .gui_semantic_action_target(request.entity, request.node)
        {
            Ok(tree) => tree,
            Err(error) => return WorldSessionReply::Rejected(error),
        };
        if tree.root_incarnation != request.root_incarnation {
            return WorldSessionReply::Rejected(format!(
                "semantic action unknown root for entity {}",
                request.entity.to_bits()
            ));
        }
        let node = match tree.node(request.node) {
            Some(node) => node,
            None => {
                return WorldSessionReply::Rejected(format!(
                    "semantic action unknown node {}",
                    request.node.0
                ));
            }
        };
        if node.lifetime != request.lifetime {
            return WorldSessionReply::Rejected(format!(
                "semantic action unknown node {} (lifetime)",
                request.node.0
            ));
        }
        let command = match ipp_core::action_command(
            &tree,
            request.node,
            request.expected_revision,
            request.action.clone(),
        ) {
            Ok(command) => command,
            Err(error) => return WorldSessionReply::Rejected(semantic_refusal(&error)),
        };
        match self
            .world
            .enqueue_gui_semantic_action_with_reply(session, request_id, command)
        {
            Ok(()) => WorldSessionReply::GuiInput,
            Err(error) => WorldSessionReply::Rejected(error.to_string()),
        }
    }
}

/// Correlated host rejection describing one semantic refusal.
fn semantic_refusal(error: &GuiSemanticActionError) -> String {
    match error {
        GuiSemanticActionError::UnknownNode(id) => {
            format!("semantic action unknown node {}", id.0)
        }
        GuiSemanticActionError::UnsupportedAction {
            node,
        } => {
            format!("semantic action unsupported for node {}", node.0)
        }
        GuiSemanticActionError::StaleRevision {
            node,
            expected,
            found,
        } => format!(
            "semantic action stale revision for node {}: expected {expected} found {found}",
            node.0
        ),
    }
}
