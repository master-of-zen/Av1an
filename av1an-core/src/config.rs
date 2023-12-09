//! This module contains the configuration for the server and the nodes.
//! Usually code for the server and the node is shared.

use std::fmt::{self, Display, Formatter};
/// This data structure contains the configuration for the server and the node.
#[derive(Debug, Clone)]
pub struct Configuration {
  /// IP address of the server, default: 127.0.0.1
  pub address: String,
  /// Port used by the server, default: 9000.
  pub port: u16,
  /// Nodes have to send a heartbeat every n seconds or they will be marked as offline.
  /// (The method [`heartbeat_timeout(node_id)`](crate::server::Server::heartbeat_timeout)
  /// with the corresponding node ID is called), default: 5.
  pub heartbeat: u64,
  /// Nodes will wait n seconds before contacting the server again to prevent a denial of service, default: 60.
  pub delay_request_data: u64,
  /// Number of times a node should try to contact the server before giving up, default: 5.
  pub retry_counter: u64,
  /// The number of threads in the thread pool, default: 8.
  pub pool_size: u64,
  /// Enable compression during communication
  pub compress: bool,
  /// Enable encryption during communication
  pub encrypt: bool,
  /// Encryption key, must be exactly 32 chars
  pub key: String,
}

impl Default for Configuration {
  fn default() -> Self {
    Configuration {
      address: "127.0.0.1".to_string(),
      port: 9000,
      heartbeat: 60,
      delay_request_data: 60,
      retry_counter: 5,
      pool_size: 8,
      compress: true,
      encrypt: true,
      // Key must be exactly 32 chars long
      key: "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX".to_string(),
    }
  }
}

impl Display for Configuration {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    write!(
      f,
      "address: '{}', port: '{}', heartbeat: '{}'\n
                  delay request data: '{}', retry counter: '{}', pool size: '{}'\n
                  compress: '{}', encrypt: '{}'",
      self.address,
      self.port,
      self.heartbeat,
      self.delay_request_data,
      self.retry_counter,
      self.pool_size,
      self.compress,
      self.encrypt
    )
  }
}
