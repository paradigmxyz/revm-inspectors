use super::types::{CallTrace, CallTraceNode, TraceMemberOrder};
use alloc::vec::Vec;
use alloy_primitives::Address;

/// An arena of recorded traces.
///
/// This type will be populated via the [TracingInspector](super::TracingInspector).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CallTraceArena {
    /// The arena of recorded trace nodes
    pub(crate) arena: Vec<CallTraceNode>,
}

impl Default for CallTraceArena {
    fn default() -> Self {
        let mut this = Self { arena: Vec::with_capacity(8) };
        this.clear();
        this
    }
}

impl CallTraceArena {
    /// Returns the nodes in the arena.
    #[inline]
    pub fn nodes(&self) -> &[CallTraceNode] {
        &self.arena
    }

    /// Returns a mutable reference to the nodes in the arena.
    #[inline]
    pub fn nodes_mut(&mut self) -> &mut Vec<CallTraceNode> {
        &mut self.arena
    }

    /// Consumes the arena and returns the nodes.
    #[inline]
    pub fn into_nodes(self) -> Vec<CallTraceNode> {
        self.arena
    }

    /// Clears the arena
    ///
    /// Note that this method has no effect on the allocated capacity of the arena.
    pub fn clear(&mut self) {
        self.arena.clear();
        self.arena.push(Default::default());
    }

    /// Returns __all__ addresses in the recorded traces, that is addresses of the trace and the
    /// caller address.
    pub fn trace_addresses(&self) -> impl Iterator<Item = Address> + '_ {
        self.nodes().iter().flat_map(|node| [node.trace.address, node.trace.caller].into_iter())
    }

    /// Pushes a new trace into the arena as a child of `parent`, returning the trace ID
    ///
    /// This appends a new trace to the arena, and also inserts a new entry in the node's parent
    /// node children set if `attach_to_parent` is `true`. E.g. if calls to precompiles should
    /// not be included in the call graph this should be called with [PushTraceKind::PushOnly].
    ///
    /// If the new trace is the root call (depth 0), it replaces the root node instead.
    pub(crate) fn push_trace(
        &mut self,
        parent: usize,
        kind: PushTraceKind,
        new_trace: CallTrace,
    ) -> usize {
        // The entry node, just update it.
        if new_trace.depth == 0 {
            self.arena[0].trace = new_trace;
            return 0;
        }

        debug_assert_eq!(
            self.arena[parent].trace.depth + 1,
            new_trace.depth,
            "parent must be exactly one level above the new trace"
        );

        let idx = self.arena.len();
        self.arena.push(CallTraceNode {
            parent: Some(parent),
            trace: new_trace,
            idx,
            ..Default::default()
        });

        // Also track the child in the parent node.
        if kind.is_attach_to_parent() {
            let parent = &mut self.arena[parent];
            let trace_location = parent.children.len();
            parent.ordering.push(TraceMemberOrder::Call(trace_location));
            parent.children.push(idx);
        }

        idx
    }
}

/// How to push a trace into the arena
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PushTraceKind {
    /// This will _only_ push the trace into the arena.
    PushOnly,
    /// This will push the trace into the arena, and also insert a new entry in the node's parent
    /// node children set.
    PushAndAttachToParent,
}

impl PushTraceKind {
    #[inline]
    const fn is_attach_to_parent(&self) -> bool {
        matches!(self, Self::PushAndAttachToParent)
    }
}
