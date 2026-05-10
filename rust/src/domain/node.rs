//! [`Node`] entity (spec: `core.allium:Node`).
//!
//! Phase 1 only models the subset of statuses and transitions the
//! sign-in / log-off loop walks through. The full state machine (with
//! `reserved`, `suspended`, `shutting_down`, etc.) lands as later
//! slices need it.

/// Lifecycle status of a [`Node`].
///
/// Phase 1 subset of `core.allium:Node.status`. The omitted variants
/// (`reserved`, `logging_on`, `suspended`, `shutting_down`) are added
/// in their owning slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeStatus {
    /// Available for a new session.
    Idle,
    /// A transport has accepted a connection; the session is being set
    /// up but the user has not yet logged on.
    Connecting,
    /// The user is logged on and using the BBS.
    LoggedOn,
    /// The session is winding down.
    LoggingOff,
}

/// A concurrent-session slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    number: u32,
    status: NodeStatus,
}

impl Node {
    /// Constructs a new node with the given `number` (1-based) in the
    /// [`NodeStatus::Idle`] status.
    #[must_use]
    pub fn new(number: u32) -> Self {
        Self {
            number,
            status: NodeStatus::Idle,
        }
    }

    /// Returns this node's display number.
    #[must_use]
    pub fn number(&self) -> u32 {
        self.number
    }

    /// Returns this node's current status.
    #[must_use]
    pub fn status(&self) -> NodeStatus {
        self.status
    }

    /// Attempts to transition `self` to `target`.
    ///
    /// # Errors
    /// Returns [`TransitionError`] if the spec does not allow a
    /// transition from the current status to `target`.
    pub(crate) fn transition_to(&mut self, target: NodeStatus) -> Result<(), TransitionError> {
        if !is_transition_allowed(self.status, target) {
            return Err(TransitionError {
                from: self.status,
                to: target,
            });
        }
        self.status = target;
        Ok(())
    }
}

/// Returned when the requested transition is not in the spec's
/// transition table for the Phase 1 subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid node transition: {from:?} -> {to:?}")]
pub struct TransitionError {
    /// Status the node was in when the transition was attempted.
    pub from: NodeStatus,
    /// Status the caller asked to move into.
    pub to: NodeStatus,
}

/// Returns whether the spec's Phase 1 transition table permits
/// `from -> to`. Later slices extend the table as more statuses land.
fn is_transition_allowed(from: NodeStatus, to: NodeStatus) -> bool {
    matches!(
        (from, to),
        (NodeStatus::Idle, NodeStatus::Connecting)
            | (
                NodeStatus::Connecting | NodeStatus::LoggingOff,
                NodeStatus::Idle
            )
            | (NodeStatus::Connecting, NodeStatus::LoggedOn)
            | (NodeStatus::LoggedOn, NodeStatus::LoggingOff)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_node_is_idle() {
        let node = Node::new(1);
        assert_eq!(node.number(), 1);
        assert_eq!(node.status(), NodeStatus::Idle);
    }

    #[test]
    fn idle_to_connecting_is_allowed() {
        let mut node = Node::new(1);
        node.transition_to(NodeStatus::Connecting).expect("allowed");
        assert_eq!(node.status(), NodeStatus::Connecting);
    }

    #[test]
    fn full_phase1_path_is_allowed() {
        let mut node = Node::new(1);
        node.transition_to(NodeStatus::Connecting).unwrap();
        node.transition_to(NodeStatus::LoggedOn).unwrap();
        node.transition_to(NodeStatus::LoggingOff).unwrap();
        node.transition_to(NodeStatus::Idle).unwrap();
    }

    #[test]
    fn carrier_dropped_during_connecting_returns_to_idle() {
        let mut node = Node::new(1);
        node.transition_to(NodeStatus::Connecting).unwrap();
        node.transition_to(NodeStatus::Idle).expect("allowed");
        assert_eq!(node.status(), NodeStatus::Idle);
    }

    #[test]
    fn idle_to_logged_on_is_rejected() {
        let mut node = Node::new(1);
        let err = node
            .transition_to(NodeStatus::LoggedOn)
            .expect_err("not allowed");
        assert_eq!(err.from, NodeStatus::Idle);
        assert_eq!(err.to, NodeStatus::LoggedOn);
        assert_eq!(node.status(), NodeStatus::Idle);
    }

    #[test]
    fn idle_to_logging_off_is_rejected() {
        let mut node = Node::new(1);
        assert!(node.transition_to(NodeStatus::LoggingOff).is_err());
    }

    #[test]
    fn logged_on_to_idle_is_rejected() {
        let mut node = Node::new(1);
        node.transition_to(NodeStatus::Connecting).unwrap();
        node.transition_to(NodeStatus::LoggedOn).unwrap();
        assert!(node.transition_to(NodeStatus::Idle).is_err());
    }
}
