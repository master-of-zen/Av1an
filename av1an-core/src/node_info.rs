//! This module contains the node id and node info data structure.
//! NodeID is just a new type pattern for a integer number.
//! NodeInfo holds the node id and a time stamp for the heartbeat.

use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use std::time::Instant;

use rand::random;
use serde::{Deserialize, Serialize};

/// New type pattern for the node id.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeID(u64);

impl NodeID {
  /// Create a new temporary node id that will be set later
  pub(crate) fn unset() -> Self {
    NodeID(0)
  }

  /// Create a new random node id.
  pub(crate) fn random() -> Self {
    NodeID(random())
  }
}

impl Display for NodeID {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.0)
  }
}

/// This data structure contains the node id and the time stamps for the heartbeat.
#[derive(Debug, PartialEq)]
pub(crate) struct NodeInfo<U> {
  /// The id of the node.
  node_id: NodeID,
  /// Time stamp for the node id since the last valid heartbeat.
  instant: Instant,
  /// Message queue (FIFO) for messages that will be send to the node.
  message_queue: VecDeque<U>,
}

impl<U> NodeInfo<U> {
  /// Create a new node info with the given node id and the current time stamp.
  fn new(node_id: NodeID) -> Self {
    NodeInfo {
      node_id,
      instant: Instant::now(),
      message_queue: VecDeque::new(),
    }
  }

  /// When a node sends a valid heartbeat update the time stamp for that node.
  fn update_heartbeat(&mut self) {
    self.instant = Instant::now();
  }

  /// Check if the heartbeat for the node is invalid using the given time range.
  fn heartbeat_invalid(&self, limit: u64) -> bool {
    let diff = Instant::now() - self.instant;
    diff.as_secs() > limit
  }

  /// Add a new message to the message queue
  fn add_message(&mut self, message: U) {
    self.message_queue.push_back(message);
    // If maximum number of messages in message queue is reached
    // discard first message
    if self.message_queue.len() > 10 {
      self.message_queue.pop_front();
    }
  }

  /// Get first message if any is available
  fn get_message(&mut self) -> Option<U> {
    self.message_queue.pop_front()
  }
}

pub(crate) struct NodeList<U> {
  // TODO: Maybe use a hashmap instead of a vec ?
  /// List of all nodes that have been registered.
  nodes: Vec<NodeInfo<U>>,
}

impl<U: Clone> NodeList<U> {
  /// Creates a new empty node list
  pub(crate) fn new() -> Self {
    NodeList { nodes: Vec::new() }
  }

  /// All the registered nodes are checked here. If the heartbeat time stamp
  /// is too old (> 2 * heartbeat in [`Configuration`](crate::config::Configuration)) then
  /// the Server trait method [`heartbeat_timeout()`](crate::server::Server::heartbeat_timeout)
  /// is called where the node should be marked as offline.
  pub(crate) fn check_heartbeat(
    &self,
    heartbeat_duration: u64,
  ) -> impl Iterator<Item = NodeID> + '_ {
    self
      .nodes
      .iter()
      .filter(move |node| node.heartbeat_invalid(heartbeat_duration))
      .map(|node| node.node_id)
  }

  /// This method generates a new and unique node id for a new node that has just registered with the server.
  /// It loops through the list of all nodes and checks wether the new id is already taken. If yes a new random id
  /// will be created and re-checked with the node list.
  pub(crate) fn register_new_node(&mut self) -> NodeID {
    let mut new_id: NodeID = NodeID::random();

    'l1: loop {
      for node_info in self.nodes.iter() {
        if node_info.node_id == new_id {
          new_id = NodeID::random();
          continue 'l1;
        }
      }

      break;
    }

    self.nodes.push(NodeInfo::new(new_id));

    new_id
  }

  /// Update the heartbeat timestamp for the given node.
  /// This happens when the heartbeat thread in the [`node`](crate::node) module
  /// has send the [`NodeMessage::HeartBeat`](crate::node::NodeMessage) message to the server.
  pub(crate) fn update_heartbeat(&mut self, node_id: NodeID) {
    for node in self.nodes.iter_mut() {
      if node.node_id == node_id {
        node.update_heartbeat();
        break;
      }
    }
  }

  /// Return the number of nodes that have registered since the start of the server.
  /// Note that this also includes inactive nodes.
  pub(crate) fn len(&self) -> usize {
    self.nodes.len()
  }

  /// Return a list of node ids and elapsed heartbeats as
  /// Vec<(NodeID, f64)>
  pub(crate) fn get_time_stamps(&self) -> Vec<(NodeID, f64)> {
    self
      .nodes
      .iter()
      .map(|node_info| (node_info.node_id, node_info.instant.elapsed().as_secs_f64()))
      .collect()
  }

  /// Add a new message for the given node.
  pub(crate) fn add_message(&mut self, message: U, node_id: NodeID) {
    for node in self.nodes.iter_mut() {
      if node.node_id == node_id {
        node.add_message(message);
        break;
      }
    }
  }

  /// Add a new message for all nodes.
  pub(crate) fn add_message_all(&mut self, message: U) {
    for node in self.nodes.iter_mut() {
      node.add_message(message.clone());
    }
  }

  /// Get first message for the given node id, if any.
  pub(crate) fn get_message(&mut self, node_id: NodeID) -> Option<U> {
    for node in self.nodes.iter_mut() {
      if node.node_id == node_id {
        return node.get_message();
      }
    }

    return None;
  }

  /// Remove a node from the current (old) server because it will
  /// migrate to a new server.
  pub(crate) fn remove_node(&mut self, node_id: NodeID) {
    let i = self
      .nodes
      .iter()
      .position(|node| node.node_id == node_id)
      .unwrap();
    self.nodes.swap_remove(i);
  }

  /// Migrate node to new server -> register a new node id.
  pub(crate) fn migrate_node(&mut self, node_id: NodeID) {
    self.nodes.push(NodeInfo::new(node_id))
  }
}

#[cfg(test)]
mod tests {
  use std::thread;
  use std::time::Duration;

  use super::*;

  #[test]
  fn test_heartbeat_invalid() {
    let node_info: NodeInfo<()> = NodeInfo::new(NodeID::unset());

    thread::sleep(Duration::from_secs(3));

    assert!(!node_info.heartbeat_invalid(5));

    thread::sleep(Duration::from_secs(3));

    assert!(node_info.heartbeat_invalid(5));
  }

  #[test]
  fn test_update_heartbeat() {
    let mut node_info: NodeInfo<()> = NodeInfo::new(NodeID::unset());

    thread::sleep(Duration::from_secs(5));

    assert!(node_info.heartbeat_invalid(3));

    node_info.update_heartbeat();

    assert!(!node_info.heartbeat_invalid(3));
  }

  #[test]
  fn test_register_new_node() {
    let mut node_list: NodeList<()> = NodeList::new();

    let node = node_list.register_new_node();

    assert_eq!(node_list.nodes.len(), 1);

    assert_eq!(node_list.nodes[0].node_id, node);

    let node = node_list.register_new_node();

    assert_eq!(node_list.nodes.len(), 2);

    assert_ne!(node_list.nodes[0].node_id, node);
    assert_eq!(node_list.nodes[1].node_id, node);
  }

  #[test]
  fn test_node_list_check_heartbeat() {
    let mut node_list: NodeList<()> = NodeList::new();

    let _ = node_list.register_new_node();
    let _ = node_list.register_new_node();
    let _ = node_list.register_new_node();
    let _ = node_list.register_new_node();

    let result = node_list.check_heartbeat(5);
    let result = result.collect::<Vec<NodeID>>();

    assert_eq!(result.len(), 0);

    thread::sleep(Duration::from_secs(5));

    let result = node_list.check_heartbeat(3);
    let result = result.collect::<Vec<NodeID>>();

    assert_eq!(result.len(), 4);
  }

  #[test]
  fn test_node_list_update_heartbeat() {
    let mut node_list: NodeList<()> = NodeList::new();

    let _ = node_list.register_new_node();
    let _ = node_list.register_new_node();
    let node_id = node_list.register_new_node();
    let _ = node_list.register_new_node();

    thread::sleep(Duration::from_secs(5));

    node_list.update_heartbeat(node_id);

    let result = node_list.check_heartbeat(3);
    let result = result.collect::<Vec<NodeID>>();

    assert_eq!(result.len(), 3);

    for other_id in result {
      assert_ne!(other_id, node_id);
    }
  }
}
