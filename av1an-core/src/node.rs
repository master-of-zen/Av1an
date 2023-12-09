//! This module contains the nc node message, trait and helper methods.
//! To use the node you have to implement the Node trait that has two methods:
//! set_initial_data() and process_data_from_server()

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::thread::{self, spawn, JoinHandle};
use std::time::Duration;

use log::{debug, error, info};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::communicator::Communicator;
use crate::config::Configuration;
use crate::error::Error;
use crate::node_info::NodeID;
use crate::server::{JobStatus, ServerMessage};

/// This message is sent from the node to the server in order to register, receive new data and send processed data.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum NodeMessage<ProcessedDataT, CustomMessageT> {
  /// Register this node with the server. The server will assign a new node id to this node and answers with a ServerMessage::InitialData message.
  /// This is the first thing every node has to do!
  Register,
  /// This node needs new data to process. The server answers with a JobStatus message.
  NeedsData(NodeID),
  /// This node has finished processing the data and sends it to the server. No answer from the server.
  HasData(NodeID, ProcessedDataT),
  /// This node sends a heartbeat message every n seconds. The time span between two heartbeats is set in the configuration Configuration.
  HeartBeat(NodeID),
  /// This is a message that the server sends to itself to break out from blocking on node connection via accept() and
  /// start checking the heartbeat time stamps of all nodes.
  CheckHeartbeat,
  /// Get some statistics from the server:
  /// - number of active nodes (node ids)
  /// - other items
  GetStatistics,
  /// Tell the server to shut down
  ShutDown,
  /// Move all the nodes to a new server given by address and port
  NewServer(String, u16),
  /// Register migrated node to new server
  NodeMigrated(NodeID),
  /// Send a custom message to one or all nodes
  CustomMessage(CustomMessageT, Option<NodeID>),
  // More items may be added in the future
}

/// This trait has to be implemented for the code that runs on all the nodes.
pub trait Node {
  type InitialDataT: Serialize + DeserializeOwned;
  type NewDataT: Serialize + DeserializeOwned;
  type ProcessedDataT: Serialize + DeserializeOwned;
  type CustomMessageT: Serialize + DeserializeOwned;

  /// Once this node has sent a NodeMessage::Register message the server responds with a ServerMessage::InitialData message.
  /// Then this method is called with the data received from the server.
  fn set_initial_data(
    &mut self,
    node_id: NodeID,
    initial_data: Option<Self::InitialDataT>,
  ) -> Result<(), Error> {
    debug!("Got new node id: {}", node_id);

    match initial_data {
      Some(_) => debug!("Got some initial data from the server."),
      None => debug!("Got no initial data from the server."),
    }

    Ok(())
  }

  /// Whenever the node requests new data from the server, the server will respond with new data that needs to be processed by the node.
  /// This method is then called with the data that was received from the server.
  /// Here you put your code that does the main number crunching on every node.
  /// Note that you have to use the decode_data() or decode_data2() helper methods from the utils module in order to
  /// deserialize the data.
  fn process_data_from_server(
    &mut self,
    data: &Self::NewDataT,
  ) -> Result<Self::ProcessedDataT, Error>;

  /// The server has send a special user defined custom message to the node.
  /// Usually this is not needed, only for debug purposes or if s.th. special has happened (user interaction for example)
  fn process_custom_message(&mut self, _custom_message: &Self::CustomMessageT) {
    debug!("Got a custom message from server");
  }
}

/// Main data structure for managing and starting the computation on the nodes.
pub struct NodeStarter {
  /// Configuration for the server and the node.
  config: Configuration,
}

impl NodeStarter {
  /// Create a new NodeStarter using the given configuration
  pub fn new(config: Configuration) -> Self {
    debug!("NodeStarter::new()");

    NodeStarter { config }
  }

  /// The main entry point for the code that runs on all nodes.
  /// You give it your own user defined data structure that implements the Node trait.
  /// Everything else is done automatically for you.
  /// The Node trait method set_initial_data() is called here once in order to set the node id and some optional data that is
  /// send to all nodes at the beginning.
  pub fn start<T: Node>(&mut self, node: T) -> Result<(), Error> {
    debug!("NodeStarter::start()");

    let ip_addr: IpAddr = self.config.address.parse()?;
    let server_addr = SocketAddr::new(ip_addr, self.config.port);
    let server_addr = Arc::new(Mutex::new(server_addr));

    let mut node_process = NodeProcess::new(server_addr.clone(), node, &self.config);
    node_process.get_initial_data()?;

    let node_heartbeat = NodeHeartbeat::new(server_addr, node_process.node_id, &self.config);

    let thread_handle = self.start_heartbeat_thread(node_heartbeat);
    self.start_main_loop(node_process);
    thread_handle.join().unwrap();

    info!("Job done, exit now");
    Ok(())
  }

  /// The heartbeat thread that runs in the background and sends heartbeat messages to the server is started here.
  /// It does this every n seconds which can be configured in the Configuration data structure.
  /// If the server doesn't receive the heartbeat within the valid time span, the server marks the node internally as offline
  /// and gives another node the same data chunk to process.
  fn start_heartbeat_thread(&self, mut node_heartbeat: NodeHeartbeat) -> JoinHandle<()> {
    debug!("NodeStarter::start_heartbeat_thread()");

    spawn(move || {
      loop {
        node_heartbeat.sleep();

        if let Err(e) = node_heartbeat.send_heartbeat_message() {
          error!(
            "Error in send_heartbeat(): {}, retry_counter: {}",
            e,
            node_heartbeat.get_counter()
          );

          if node_heartbeat.dec_and_check_counter() {
            debug!("Retry counter is zero, will exit now");
            break;
          }
        } else {
          // Reset the counter if message was sent successfully
          node_heartbeat.reset_counter();
        }
      }

      debug!("Heartbeat loop finished")
    })
  }

  /// Here is main loop for this node. It keeps requesting and processing data until the server
  /// is finished. There will be no finish message from the server and the node will just run into
  /// a timeout and exit.
  /// If there is an error this node will wait n seconds before it tries to reconnect to the server.
  /// The delay time can be configured in the Configuration data structure.
  /// With every error the retry counter is decremented. If it reaches zero the node will give up and exit.
  /// The counter can be configured in the Configuration.
  fn start_main_loop<T: Node>(&self, mut node_process: NodeProcess<T>) {
    debug!("NodeStarter::start_main_loop()");

    loop {
      debug!("Ask server for new data");

      if let Err(e) = node_process.get_and_process_data() {
        error!(
          "Error in get_and_process_data(): {}, retry counter: {:?}",
          e,
          node_process.get_counter()
        );

        if node_process.dec_and_check_counter() {
          debug!("Retry counter is zero, will exit now");
          break;
        }

        debug!(
          "Will wait before retry (delay_request_data: {} sec)",
          node_process.get_delay()
        );
        node_process.sleep();
      } else {
        // Reset the counter if message was sent successfully
        node_process.reset_counter()
      }
    }

    debug!("Main loop finished")
  }
}

/// Manages and sends heartbeat messages to the server.
struct NodeHeartbeat {
  /// IP address and port of the server.
  server_addr: Arc<Mutex<SocketAddr>>,
  /// The node id for this node,
  node_id: NodeID,
  /// How often should the heartbeat thread try to contact the server before giving up.
  retry_counter: RetryCounter,
  /// Send every heartbeat_duration seconds the xxx message to the server.
  heartbeat_duration: Duration,
  /// Handles all the communication
  communicator: Communicator,
}

impl NodeHeartbeat {
  /// Creates a new NodeHeartbeat with the given arguments.
  fn new(server_addr: Arc<Mutex<SocketAddr>>, node_id: NodeID, config: &Configuration) -> Self {
    debug!("NodeHeartbeat::new()");

    NodeHeartbeat {
      server_addr,
      node_id,
      retry_counter: RetryCounter::new(config.retry_counter),
      heartbeat_duration: Duration::from_secs(config.heartbeat),
      communicator: Communicator::new(config),
    }
  }

  /// The heartbeat thread will sleep for the given duration from the configuration.
  fn sleep(&self) {
    debug!("NodeHeartbeat::sleep()");

    thread::sleep(self.heartbeat_duration);
  }

  /// Send the NodeMessage::HeartBeat message to the server.
  fn send_heartbeat_message(&mut self) -> Result<(), Error> {
    debug!("NodeHeartbeat::send_heartbeat_message()");
    let message: NodeMessage<(), ()> = NodeMessage::HeartBeat(self.node_id);
    let server_addr = *self.server_addr.lock()?;

    self.communicator.send_data(&message, &server_addr)
  }

  /// Returns the current value of the retry counter.
  fn get_counter(&self) -> u64 {
    debug!("NodeHeartbeat::get_counter()");

    self.retry_counter.counter
  }

  /// Decrement the retry counter on error and check if it is zero.
  /// If zero return true, else false.
  fn dec_and_check_counter(&mut self) -> bool {
    debug!("NodeHeartbeat::dec_and_check_counter()");

    self.retry_counter.dec_and_check()
  }

  /// Resets the retry counter to the initial value when there was no error.
  fn reset_counter(&mut self) {
    debug!("NodeHeartbeat::reset_counter()");

    self.retry_counter.reset()
  }
}

/// Communication with the server and processing of data.
struct NodeProcess<T> {
  /// IP address and port of the server.
  server_addr: Arc<Mutex<SocketAddr>>,
  /// The suer defined data structure that implements the Node trait.
  node: T,
  /// The node id for this node,
  node_id: NodeID,
  /// How often should the main processing loop try to contact the server before giving up.
  retry_counter: RetryCounter,
  /// In case of IO error wait delay_duration seconds before trying to contact the server again.
  delay_duration: Duration,
  /// Handles all the communication
  communicator: Communicator,
}

impl<T: Node> NodeProcess<T> {
  /// Creates a new NodeProcess with the given arguments.
  fn new(server_addr: Arc<Mutex<SocketAddr>>, node: T, config: &Configuration) -> Self {
    debug!("NodeProcess::new()");

    NodeProcess {
      server_addr,
      node,
      // This will be set in the method get_initial_data()
      node_id: NodeID::unset(),
      retry_counter: RetryCounter::new(config.retry_counter),
      delay_duration: Duration::from_secs(config.delay_request_data),
      communicator: Communicator::new(config),
    }
  }

  /// This is called once at the beginning of NodeStarter::start().
  /// It sends a NodeMessage::Register message to the server and expects a ServerMessage::InitialData message from the server.
  /// On success it sets the new assigned node id for this node and calls the Node trait method set_initial_data().
  /// If the server doesn't respond with a ServerMessage::InitialData message a Error::ServerMsgMismatch error is returned.
  fn get_initial_data(&mut self) -> Result<(), Error> {
    debug!("NodeProcess::get_initial_data()");

    let initial_data: ServerMessage<T::InitialDataT, T::NewDataT, T::CustomMessageT> =
      self.send_register_message()?;

    match initial_data {
      ServerMessage::InitialData(node_id, initial_data) => {
        info!("Got node_id: {} and initial data from server", node_id);
        self.node_id = node_id;
        self.node.set_initial_data(node_id, initial_data)
      }
      _msg => {
        error!("Error in get_initial_data(), ServerMessage mismatch, expected: InitialData");
        Err(Error::ServerMsgMismatch)
      }
    }
  }

  /// Send the NodeMessage::Register message to the server.
  fn send_register_message(
    &mut self,
  ) -> Result<ServerMessage<T::InitialDataT, T::NewDataT, T::CustomMessageT>, Error> {
    debug!("NodeProcess::send_register_message()");
    let message: NodeMessage<T::ProcessedDataT, T::CustomMessageT> = NodeMessage::Register;
    let server_addr = *self.server_addr.lock()?;

    self.communicator.send_receive_data(&message, &server_addr)
  }

  /// This method sends a NodeMessage::NeedsData message to the server and reacts accordingly to the server response:
  /// Only one message is expected as a response from the server: ServerMessage::JobStatus. This status can have two values
  /// 1. JobStatus::Unfinished: This means that the job is note done and there is still some more data to be processed.
  ///      This node will then process the data calling the process_data_from_server() method and sends the data back to the
  ///      server using the NodeMessage::HasData message.
  /// 2. JobStatus::Waiting: This means that not all nodes are done and the server is still waiting for all nodes to finish.
  /// If the server sends a different message this method will return a Error::ServerMsgMismatch error.
  fn get_and_process_data(&mut self) -> Result<(), Error> {
    debug!("NodeProcess::get_and_process_data()");

    let new_data: ServerMessage<T::InitialDataT, T::NewDataT, T::CustomMessageT> =
      self.send_needs_data_message()?;

    match new_data {
      ServerMessage::JobStatus(job_status) => {
        match job_status {
          JobStatus::Unfinished(data) => self.process_data_and_send_has_data_message(&data),
          JobStatus::Waiting => {
            // The node will not exit here since the job is not 100% done.
            // This just means that all the remaining work has already
            // been distributed among all nodes.
            // One of the nodes can still crash and thus free nodes have to ask the server for more work
            // from time to time (delay_request_data).

            debug!(
              "Waiting for other nodes to finish (delay_request_data: {} sec)...",
              self.get_delay()
            );
            self.sleep();
            Ok(())
          }
          _ => {
            // The server does not bother sending the node a JobStatus::Finished message.
            error!("Error: unexpected message from server");
            Err(Error::ServerMsgMismatch)
          }
        }
      }
      ServerMessage::CustomMessage(message) => {
        // Forward custom message to user code
        self.node.process_custom_message(&message);
        Ok(())
      }
      ServerMessage::NewServer(server, port) => {
        self.new_server(server, port)?;
        self.send_node_migrated()
      }
      _ => {
        error!("Error in process_data_and_send_has_data_message(), ServerMessage mismatch");
        Err(Error::ServerMsgMismatch)
      }
    }
  }

  /// Send the NodeMessage::NeedsData message to the server.
  fn send_needs_data_message(
    &mut self,
  ) -> Result<ServerMessage<T::InitialDataT, T::NewDataT, T::CustomMessageT>, Error> {
    debug!("NodeProcess::send_needs_data_message()");
    let message: NodeMessage<T::ProcessedDataT, T::CustomMessageT> =
      NodeMessage::NeedsData(self.node_id);
    let server_addr = *self.server_addr.lock()?;

    self.communicator.send_receive_data(&message, &server_addr)
  }

  /// Process the new data from the server and sends the result back to the server using
  /// the NodeMessage::HasData message.
  fn process_data_and_send_has_data_message(&mut self, data: &T::NewDataT) -> Result<(), Error> {
    debug!("NodeProcess::process_data_and_send_has_data_message()");
    let result = self.node.process_data_from_server(data)?;
    let message: NodeMessage<T::ProcessedDataT, T::CustomMessageT> =
      NodeMessage::HasData(self.node_id, result);
    let server_addr = *self.server_addr.lock()?;

    self.communicator.send_data(&message, &server_addr)
  }

  /// Change settings for a new server
  fn new_server(&mut self, server: String, port: u16) -> Result<(), Error> {
    debug!("NodeProcess::new_server()");
    let ip_addr: IpAddr = server.parse()?;
    let mut server_addr = self.server_addr.lock()?;
    *server_addr = SocketAddr::new(ip_addr, port);
    Ok(())
  }

  /// Send message to new server that the node has migrated
  fn send_node_migrated(&mut self) -> Result<(), Error> {
    debug!("NodeProcess::send_node_migrated()");

    let message: NodeMessage<T::ProcessedDataT, T::CustomMessageT> =
      NodeMessage::NodeMigrated(self.node_id);
    let server_addr = *self.server_addr.lock()?;

    self.communicator.send_data(&message, &server_addr)
  }

  /// Returns the current value of the retry counter.
  fn get_counter(&self) -> u64 {
    debug!("NodeProcess::get_counter()");

    self.retry_counter.counter
  }

  /// Decrement the retry counter on error and check if it is zero.
  /// If zero return true, else false.
  fn dec_and_check_counter(&mut self) -> bool {
    debug!("NodeProcess::dec_and_check_counter()");

    self.retry_counter.dec_and_check()
  }

  /// Returns the delay duration in seconds.
  fn get_delay(&self) -> u64 {
    debug!("NodeProcess::get_delay()");

    self.delay_duration.as_secs()
  }

  /// The current thread in the main loop sleeps for the given delay from the configuration file.
  fn sleep(&self) {
    debug!("NodeProcess::sleep()");

    thread::sleep(self.delay_duration);
  }

  /// Resets the retry counter to the initial value when there was no error.
  fn reset_counter(&mut self) {
    debug!("NodeProcess::reset_counter()");

    self.retry_counter.reset()
  }
}

/// Counter for node if connection to server is not possible.
/// The counter will be decreased every time there is an IO error and if it is zero the method dec_and_check
/// returns true, otherwise false.
/// When the connection to the server is working again, the counter is reset to its initial value.
#[derive(Debug, Clone)]
struct RetryCounter {
  /// The initial value for the counter. It can be reset to this value when a message has been send / received successfully.
  init: u64,
  /// The current value for the counter. It will be decremented in an IO error case.
  counter: u64,
}

impl RetryCounter {
  /// Create a new retry counter with the given limit.
  /// It will count backwards to zero.
  fn new(counter: u64) -> Self {
    debug!("RetryCounter::new()");

    RetryCounter {
      init: counter,
      counter,
    }
  }

  /// Decrements and checks the counter.
  /// If it's zero return true, else return false.
  fn dec_and_check(&mut self) -> bool {
    debug!("RetryCounter::dec_and_check()");

    if self.counter == 0 {
      true
    } else {
      self.counter -= 1;
      false
    }
  }

  /// Resets the counter to it's initial value.
  fn reset(&mut self) {
    debug!("RetryCounter::reset()");

    self.counter = self.init
  }
}

pub fn run_node() {}

#[cfg(test)]
mod tests {
  use std::net::{IpAddr, Ipv4Addr};

  use super::*;

  struct TestNode;

  impl Node for TestNode {
    type InitialDataT = ();
    type NewDataT = ();
    type ProcessedDataT = ();
    type CustomMessageT = ();

    fn process_data_from_server(&mut self, _data: &()) -> Result<(), Error> {
      Ok(())
    }
  }

  fn server_addr_for_test() -> Arc<Mutex<SocketAddr>> {
    let server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    Arc::new(Mutex::new(server_addr))
  }

  #[test]
  fn test_nhb_dec_and_check_counter1() {
    let config = Configuration::default();
    let server_addr = server_addr_for_test();
    let mut nhb = NodeHeartbeat::new(server_addr, NodeID::unset(), &config);

    assert_eq!(nhb.get_counter(), 5);
    assert!(!nhb.dec_and_check_counter());
    assert_eq!(nhb.get_counter(), 4);
    assert!(!nhb.dec_and_check_counter());
    assert_eq!(nhb.get_counter(), 3);
    assert!(!nhb.dec_and_check_counter());
    assert_eq!(nhb.get_counter(), 2);
    assert!(!nhb.dec_and_check_counter());
    assert_eq!(nhb.get_counter(), 1);
    assert!(!nhb.dec_and_check_counter());
    assert_eq!(nhb.get_counter(), 0);
    assert!(nhb.dec_and_check_counter());
    assert_eq!(nhb.get_counter(), 0);
  }

  #[test]
  fn test_nhb_reset_counter() {
    let config = Configuration::default();
    let server_addr = server_addr_for_test();
    let mut nhb = NodeHeartbeat::new(server_addr, NodeID::unset(), &config);

    assert_eq!(nhb.get_counter(), 5);
    assert!(!nhb.dec_and_check_counter());
    assert_eq!(nhb.get_counter(), 4);
    assert!(!nhb.dec_and_check_counter());
    assert_eq!(nhb.get_counter(), 3);
    nhb.reset_counter();
    assert_eq!(nhb.get_counter(), 5);
    assert!(!nhb.dec_and_check_counter());
    assert_eq!(nhb.get_counter(), 4);
  }

  #[test]
  fn test_np_dec_and_check_counter() {
    let node = TestNode {};
    let config = Configuration::default();
    let server_addr = server_addr_for_test();
    let mut np = NodeProcess::new(server_addr, node, &config);

    assert_eq!(np.get_counter(), 5);
    assert!(!np.dec_and_check_counter());
    assert_eq!(np.get_counter(), 4);
    assert!(!np.dec_and_check_counter());
    assert_eq!(np.get_counter(), 3);
    assert!(!np.dec_and_check_counter());
    assert_eq!(np.get_counter(), 2);
    assert!(!np.dec_and_check_counter());
    assert_eq!(np.get_counter(), 1);
    assert!(!np.dec_and_check_counter());
    assert_eq!(np.get_counter(), 0);
    assert!(np.dec_and_check_counter());
    assert_eq!(np.get_counter(), 0);
  }

  #[test]
  fn test_np_reset_counter() {
    let node = TestNode {};
    let config = Configuration::default();
    let server_addr = server_addr_for_test();
    let mut np = NodeProcess::new(server_addr, node, &config);

    assert_eq!(np.get_counter(), 5);
    assert!(!np.dec_and_check_counter());
    assert_eq!(np.get_counter(), 4);
    assert!(!np.dec_and_check_counter());
    assert_eq!(np.get_counter(), 3);
    np.reset_counter();
    assert_eq!(np.get_counter(), 5);
    assert!(!np.dec_and_check_counter());
    assert_eq!(np.get_counter(), 4);
  }
}
