//! [`NodePool`]: a fixed-size pool of [`Node`] entities, with atomic
//! allocation semantics for concurrent sessions.
//!
//! The pool is application runtime state rather than pure domain logic:
//! it protects domain [`Node`] values with an async-aware lock so
//! transports can safely allocate nodes concurrently.

use tokio::sync::mpsc;
use tokio::sync::Mutex;

use crate::app::terminal::SessionSignal;
use crate::domain::node::{Node, NodeStatus, TransitionError};

/// A fixed-size pool of [`Node`]s sitting behind an async-aware lock.
///
/// Besides allocation, the pool carries each live node's
/// [`SessionSignal`] sender — the minimal cross-session seam (July
/// 2026 review, item 26) another task uses to reach a session parked
/// at a prompt. The fuller who's-online presence registry (item 25)
/// grows here with Tier E's `WHO` slice.
#[derive(Debug)]
pub struct NodePool {
    nodes: Mutex<Vec<Node>>,
    signals: Mutex<Vec<Option<mpsc::UnboundedSender<SessionSignal>>>>,
}

impl NodePool {
    /// Constructs a pool of `max_nodes` idle nodes, numbered `1..=max_nodes`.
    pub fn new(max_nodes: u32) -> Self {
        let nodes = (1..=max_nodes).map(Node::new).collect();
        let signals = (1..=max_nodes).map(|_| None).collect();
        Self {
            nodes: Mutex::new(nodes),
            signals: Mutex::new(signals),
        }
    }

    /// Installs `sender` as node `number`'s session-signal lane. The
    /// transport calls this once per accepted connection, right after
    /// [`allocate`](Self::allocate); [`release`](Self::release) clears
    /// it. Out-of-range numbers are ignored.
    pub async fn attach_signal_sender(
        &self,
        number: u32,
        sender: mpsc::UnboundedSender<SessionSignal>,
    ) {
        let mut signals = self.signals.lock().await;
        if let Some(slot) = number
            .checked_sub(1)
            .and_then(|index| signals.get_mut(index as usize))
        {
            *slot = Some(sender);
        }
    }

    /// The session-signal sender for node `number`, if a session is
    /// live on it. Cloning the sender is how another task addresses
    /// that session (e.g. delivering an OLM line into its prompt).
    pub async fn signal_sender(&self, number: u32) -> Option<mpsc::UnboundedSender<SessionSignal>> {
        let signals = self.signals.lock().await;
        number
            .checked_sub(1)
            .and_then(|index| signals.get(index as usize))
            .and_then(Clone::clone)
    }

    /// Atomically claims an idle node by transitioning it to
    /// [`NodeStatus::Connecting`] and returns its number. Returns `None`
    /// if every node is busy.
    pub async fn allocate(&self) -> Option<u32> {
        let mut nodes = self.nodes.lock().await;
        let node = nodes.iter_mut().find(|n| n.status() == NodeStatus::Idle)?;
        node.transition_to(NodeStatus::Connecting).ok()?;
        Some(node.number())
    }

    /// Releases the node identified by `number` back to
    /// [`NodeStatus::Idle`].
    ///
    /// # Errors
    /// Returns [`ReleaseError::UnknownNode`] if no node has that number,
    /// or [`ReleaseError::InvalidTransition`] if the node's current
    /// status does not permit returning to idle.
    pub async fn release(&self, number: u32) -> Result<(), ReleaseError> {
        {
            let mut signals = self.signals.lock().await;
            if let Some(slot) = number
                .checked_sub(1)
                .and_then(|index| signals.get_mut(index as usize))
            {
                *slot = None;
            }
        }
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
            .map(super::super::domain::node::Node::status)
    }
}

/// Errors returned by [`NodePool::release`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReleaseError {
    /// The pool has no node with that number.
    #[error("unknown node #{0}")]
    UnknownNode(u32),
    /// The node's current status does not permit transitioning to
    /// [`NodeStatus::Idle`].
    #[error(transparent)]
    InvalidTransition(#[from] TransitionError),
}

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
        // Two concurrent allocations must claim distinct nodes. Run
        // many trials so a racy implementation has a high chance of
        // producing a duplicate.
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
