//! [`NodePool`]: a fixed-size pool of [`Node`] entities, with atomic
//! allocation semantics for concurrent sessions.
//!
//! The pool is the unit of concurrency for the BBS: at most
//! `core.allium:config.max_nodes` sessions can be active at once
//! (Slice 7 introduces that config key). The supervisor (Slice 8) calls
//! [`NodePool::allocate`] for each accepted connection and
//! [`NodePool::release`] when the session ends.

use tokio::sync::Mutex;

use crate::domain::node::{Node, NodeStatus, TransitionError};

/// A fixed-size pool of [`Node`]s sitting behind an async-aware lock.
#[derive(Debug)]
pub struct NodePool {
    nodes: Mutex<Vec<Node>>,
}

impl NodePool {
    /// Constructs a pool of `max_nodes` idle nodes, numbered `1..=max_nodes`.
    pub fn new(max_nodes: u32) -> Self {
        let nodes = (1..=max_nodes).map(Node::new).collect();
        Self {
            nodes: Mutex::new(nodes),
        }
    }

    /// Atomically claims an idle node by transitioning it to
    /// [`NodeStatus::Connecting`] and returns its number. Returns `None`
    /// if every node is busy.
    pub async fn allocate(&self) -> Option<u32> {
        let mut nodes = self.nodes.lock().await;
        let node = nodes.iter_mut().find(|n| n.status() == NodeStatus::Idle)?;
        node.transition_to(NodeStatus::Connecting)
            .expect("idle -> connecting is permitted by the spec");
        Some(node.number())
    }

    /// Releases the node identified by `number` back to
    /// [`NodeStatus::Idle`].
    ///
    /// # Errors
    /// Returns [`ReleaseError::UnknownNode`] if no node has that number,
    /// or [`ReleaseError::InvalidTransition`] if the node's current
    /// status does not permit returning to idle (callers are expected
    /// to drive the node through `LoggingOff` first).
    pub async fn release(&self, number: u32) -> Result<(), ReleaseError> {
        let mut nodes = self.nodes.lock().await;
        let node = nodes
            .iter_mut()
            .find(|n| n.number() == number)
            .ok_or(ReleaseError::UnknownNode(number))?;
        node.transition_to(NodeStatus::Idle)
            .map_err(ReleaseError::InvalidTransition)
    }

    /// Returns the current status of the node identified by `number`,
    /// or `None` if no such node exists.
    pub async fn status_of(&self, number: u32) -> Option<NodeStatus> {
        let nodes = self.nodes.lock().await;
        nodes
            .iter()
            .find(|n| n.number() == number)
            .map(|n| n.status())
    }
}

/// Errors returned by [`NodePool::release`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseError {
    /// The pool has no node with that number.
    UnknownNode(u32),
    /// The node's current status does not permit transitioning to
    /// [`NodeStatus::Idle`].
    InvalidTransition(TransitionError),
}

impl std::fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownNode(n) => write!(f, "unknown node #{n}"),
            Self::InvalidTransition(t) => write!(f, "{t}"),
        }
    }
}

impl std::error::Error for ReleaseError {}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    #[tokio::test]
    async fn allocate_returns_first_idle_node() {
        let pool = NodePool::new(2);
        let number = pool.allocate().await.expect("idle node available");
        assert_eq!(number, 1);
        assert_eq!(pool.status_of(1).await, Some(NodeStatus::Connecting));
    }

    #[tokio::test]
    async fn sequential_allocations_yield_distinct_nodes() {
        let pool = NodePool::new(2);
        let a = pool.allocate().await.expect("first");
        let b = pool.allocate().await.expect("second");
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn allocate_returns_none_when_full() {
        let pool = NodePool::new(1);
        pool.allocate().await.expect("first");
        assert!(pool.allocate().await.is_none());
    }

    #[tokio::test]
    async fn released_node_can_be_reallocated() {
        let pool = NodePool::new(1);
        let first = pool.allocate().await.expect("first");
        // Walk through the spec-permitted path back to idle.
        // (allocate left it Connecting; LoggingOff is reachable via
        //  Connecting -> LoggedOn -> LoggingOff -> Idle.)
        // The pool only exposes release(), which goes Connecting -> Idle.
        pool.release(first).await.expect("connecting -> idle");
        assert_eq!(pool.status_of(first).await, Some(NodeStatus::Idle));
        let second = pool.allocate().await.expect("after release");
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn release_unknown_node_errors() {
        let pool = NodePool::new(1);
        let err = pool.release(99).await.expect_err("unknown");
        assert_eq!(err, ReleaseError::UnknownNode(99));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_allocations_yield_distinct_nodes() {
        // Two concurrent allocations must claim distinct nodes — the
        // pool's atomicity guarantee. We run many trials so a racy
        // implementation has a high chance of producing a duplicate.
        for _ in 0..50 {
            let pool = Arc::new(NodePool::new(2));
            let a = pool.clone();
            let b = pool.clone();
            let (ra, rb) = tokio::join!(
                tokio::spawn(async move { a.allocate().await }),
                tokio::spawn(async move { b.allocate().await })
            );
            let na = ra.unwrap().expect("first");
            let nb = rb.unwrap().expect("second");
            assert_ne!(na, nb);
        }
    }
}
