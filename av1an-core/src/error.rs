//! This module contains the common error type for server and node.

use std::{io, net, sync};

use thiserror::Error;

use crate::node_info::NodeID;

/// This data structure contains all error codes for the server and the nodes.
#[derive(Error, Debug)]
pub enum Error {
  /// Parsing the IP address went wrong.
  #[error("IP address parse error: {0}")]
  IPAddrParse(#[from] net::AddrParseError),
  /// Common IO error, usually network related.
  #[error("IO error: {0}")]
  IOError(#[from] io::Error),
  /// Data could not be serialized for sending over the network.
  #[error("Serialize bincode error: {0}")]
  Serialize(bincode::Error),
  /// Data coming from the network could not be deserialized.
  #[error("Deserialize bincode error: {0}")]
  Deserialize(bincode::Error),
  /// The [`bincode`] crate has its own error.
  #[error("Bincode error: {0}")]
  Bincode(#[from] Box<bincode::ErrorKind>),
  /// Decompression error
  #[error("Decompression error")]
  Decompress(#[from] lz4_flex::block::DecompressError),
  /// Encrypt error
  #[error("Encrypt error")]
  Encrypt,
  /// Decrypt error
  #[error("Decrypt error")]
  Decrypt,
  /// The node expected a specific message from the server but got something totally different.
  #[error("Server message mismatch error")]
  ServerMsgMismatch,
  /// The server expected a specific message from the node but got something totally different.
  #[error("Node message mismatch error")]
  NodeMsgMismatch,
  /// A different node id was expected. Expected first node id, found second node id.
  #[error("Node id mismatch error, expected: {0}, found: {1}")]
  NodeIDMismatch(NodeID, NodeID),
  /// [`Mutex`](std::sync::Mutex) could not be locked or a thread did panic while holding the lock.
  #[error("Mutex poisson error")]
  MutexPoison,
  /// An error using the utility data structure [`Array2D`](crate::Array2D).
  #[error("Array2D dimension mismatch error, expected: {0:?}, got: {1:?}")]
  Array2DDimensionMismatch((u64, u64), (u64, u64)),
  /// Custom user defined error. This needs to be replaced in the future with [`Box<dyn Error>`] or something similar.
  #[error("Custom user defined error: {0}")]
  Custom(u32),
}

impl<T> From<sync::PoisonError<sync::MutexGuard<'_, T>>> for Error {
  fn from(_: sync::PoisonError<sync::MutexGuard<'_, T>>) -> Error {
    Error::MutexPoison
  }
}
