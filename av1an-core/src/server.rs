//! This module contains the nc server message, trait and helper methods
//! To use the server you have to implement the Server trait that has five methods:
//! initial_data(): This method is called once for every node when the node registers with the server.
//! prepare_data_for_node(): This method is called when the node needs new data to process.
//! process_data_from_node(): This method is called when the node is done with processing the data and has sent the result back to the server.
//! heartbeat_timeout(): This method is called when the node has missed a heartbeat, usually the node is then marked as offline and the chunk
//!     of data for that node is sent to another node.
//! finish_job(): This method is called when the job is done and all the threads are finished. Usually you want to save the results to disk
//!     in here.

use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use log::{debug, error, info};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use threadpool::ThreadPool;

use crate::chunk::Chunk;
use crate::communicator::Communicator;
use crate::config::Configuration;
use crate::error::Error;
use crate::node::NodeMessage;
use crate::node_info::{NodeID, NodeList};

/// This message is send from the server to each node.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum ServerMessage<InitialDataT, NewDataT, CustomMessageT> {
  /// When the node registers for the first time with the NodeMessage::Register message the server assigns a new node id
  /// and sends some optional initial data to the node.
  InitialData(NodeID, Option<InitialDataT>),
  /// When the node requests new data to process with the NodeMessage::NeedsData message, the current job status is sent to
  /// the node: unfinished, waiting or finished.
  JobStatus(JobStatus<NewDataT>),
  /// Send some statistics about the server to the node.
  Statistics(ServerStatistics),
  /// Move all nodes to a new server.
  NewServer(String, u16),
  /// Send a custom message to one or all nodes.
  CustomMessage(CustomMessageT),
}

/// The job status tells the node what to do next: process the new data, wait for other nodes to finish or exit. This is the answer from the server when
/// a node request new data via the NodeMessage::NeedsData message.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum JobStatus<NewDataT> {
  /// The job is not done yet and the node has to process the data the server sends to it.
  Unfinished(NewDataT),
  /// The server is still waiting for other nodes to finish the job. This means that all the work has already been distributed to all the nodes
  /// and the server sends this message to the remaining nodes. It does this because some of the processing nodes can still crash, so that its work
  /// has to be done by a waiting node.
  Waiting,
  /// Now all nodes are finished and the job is done. The server sends this message to all the nodes that request new data.
  Finished,
}

/// This is the trait that you have to implement in order to start the server.
pub trait Server {
  type InitialDataT: Serialize + DeserializeOwned;
  type NewDataT: Serialize + DeserializeOwned;
  type ProcessedDataT: Serialize + DeserializeOwned;
  type CustomMessageT: Serialize + DeserializeOwned + Send + Clone;

  /// This method is called once for every new node that registers with the server using the NodeMessage::Register message.
  /// It may prepare some initial data that is common for all nodes at the beginning of the job.
  fn initial_data(&mut self) -> Result<Option<Self::InitialDataT>, Error> {
    Ok(None)
  }
  /// This method is called when the node requests new data with the NodeMessage::NeedsData message.
  /// It's the servers task to prepare the data for each node individually.
  /// For example a 2D array can be split up into smaller pieces that are processed by each node.
  /// Usually the server will have an internal data structure containing all the registered nodes.
  /// According to the status of the job this method returns a JobStatus value:
  /// Unfinished, Waiting or Finished.
  fn prepare_data_for_node(&mut self, node_id: NodeID) -> Result<JobStatus<Self::NewDataT>, Error>;
  /// When one node is done processing the data from the server it will send the result back to the server and then this method is called.
  /// For example a small piece of a 2D array may be returned by the node and the server puts the resulting data back into the big 2D array.
  fn process_data_from_node(
    &mut self,
    node_id: NodeID,
    data: &Self::ProcessedDataT,
  ) -> Result<(), Error>;
  /// Every node has to send a heartbeat message to the server. If it doesn't arrive in time (2 * the heartbeat value in the Configuration)
  /// then this method is called with the corresponding node id and the node should be marked as offline in this method.
  fn heartbeat_timeout(&mut self, nodes: Vec<NodeID>);
  /// When all the nodes are done with processing and all internal threads are also finished then this method is called.
  /// Usually you want to save all the results to disk and optionally you can write an e-mail to the user that he / she can start
  /// writing a paper for his / her PhD.
  fn finish_job(&mut self);
}

/// Main data structure for managing and starting the server.
pub struct ServerStarter {
  /// Configuration for the server and the node.
  config: Configuration,
}

impl ServerStarter {
  /// Create a new ServerStarter using the given configuration
  pub fn new(config: Configuration) -> Self {
    debug!("ServerStarter::new()");

    ServerStarter { config }
  }

  /// This is the main method that you call when you start the server. It expects your custom data structure that implements the Server trait.
  pub fn start<T: Server + Send + 'static>(&mut self, server: T) -> Result<(), Error> {
    debug!("ServerStarter::new()");

    // let time_start = Instant::now();
    let server_process = Arc::new(ServerProcess::new(&self.config, server));
    let server_heartbeat = ServerHeartbeat::new(&self.config);
    let thread_pool = ThreadPool::new((self.config.pool_size + 1) as usize);

    self.start_heartbeat_thread(&thread_pool, server_heartbeat);
    self.start_main_loop(&thread_pool, server_process.clone());

    // let time_taken = (Instant::now() - time_start).as_secs_f64();
    let time_taken = server_process.calc_total_time();

    info!(
      "Time taken: {} s, {} min, {} h",
      time_taken,
      time_taken / 60.0,
      time_taken / (60.0 * 60.0)
    );

    thread_pool.join();

    Ok(())
  }

  /// The heartbeat check thread is started here in an endless loop.
  /// It calls the method send_check_heartbeat_message() which sends the NodeMessage::CheckHeartbeat message
  /// to the server. The server then checks all the nodes to see if one of them missed a heartbeat.
  /// If there is an IO error the loop exits because the server also has finished its main loop and
  /// doesn't accept any tcp connections anymore.
  /// The job is done and no more heartbeats will arrive.
  fn start_heartbeat_thread(
    &mut self,
    thread_pool: &ThreadPool,
    server_heartbeat: ServerHeartbeat,
  ) {
    debug!("ServerStarter::start_heartbeat_thread()");

    thread_pool.execute(move || {
      loop {
        server_heartbeat.sleep();

        if let Err(e) = server_heartbeat.send_check_heartbeat_message() {
          error!(
            "Error in start_heartbeat_thread(), couldn't send CheckHeartbeat message: {}",
            e
          );
          break;
        }
      }
      debug!("Exit start_heartbeat_thread() main loop");
    });
  }

  /// In here the main loop and the tcp server are started.
  /// For every node connection the method start_node_thread() is called, which handles the node request in a separate thread.
  /// If the job is done one the main loop will exited
  fn start_main_loop<T: Server + Send + 'static>(
    &self,
    thread_pool: &ThreadPool,
    server_process: Arc<ServerProcess<T, T::CustomMessageT>>,
  ) {
    debug!("ServerStarter::start_main_loop()");

    let ip_addr: IpAddr = "0.0.0.0".parse().unwrap(); // TODO: Make this configurable ?
    let socket_addr = SocketAddr::new(ip_addr, server_process.port);
    let listener = TcpListener::bind(socket_addr).unwrap();

    loop {
      match listener.accept() {
        Ok((stream, addr)) => {
          debug!("Connection from node: {}", addr);
          self.start_node_thread(thread_pool, stream, server_process.clone());
        }
        Err(e) => {
          error!("IO error while accepting node connections: {}", e);
        }
      }

      if server_process.is_job_done() {
        // Try to exit main loop as soon as possible.
        // Don't bother with informing the nodes since they have a retry counter for IO errors
        // and will exit when the counter reaches zero.
        // The server check_heartbeat thread also exits if there is an IO error, that means the server
        // doesn't accept connections anymore.
        break;
      }
    }

    info!("Job is done, will call Server::finish_job()");
    server_process.server.lock().unwrap().finish_job();
  }
  /// This starts a new thread for each node that sends a message to the server and calls the handle_node() method in that thread.
  fn start_node_thread<T: Server + Send + 'static>(
    &self,
    thread_pool: &ThreadPool,
    stream: TcpStream,
    server_process: Arc<ServerProcess<T, T::CustomMessageT>>,
  ) {
    debug!("ServerStarter::start_node_thread()");

    thread_pool.execute(move || {
      if let Err(e) = server_process.handle_node(stream) {
        error!("Error in handle_node(): {}", e);
      }
    });
  }
}

/// Takes care of all the heartbeat time stamps for all the registered nodes.
struct ServerHeartbeat {
  /// The socket for the server itself.
  server_socket: SocketAddr,
  /// heartbeat timeout duration * 2, this gives the node enough time to send their heartbeat messages.
  duration: Duration,
  /// Handles all the communication
  communicator: Mutex<Communicator>,
}

impl ServerHeartbeat {
  /// Creates a new ServerHeartbeat with the given configuration.
  fn new(config: &Configuration) -> Self {
    debug!("ServerHeartbeat::new()");

    let ip_addr: IpAddr = "127.0.0.1".parse().unwrap();
    let server_socket = SocketAddr::new(ip_addr, config.port);
    let duration = Duration::from_secs(2 * config.heartbeat);

    ServerHeartbeat {
      server_socket,
      duration,
      communicator: Mutex::new(Communicator::new(config)),
    }
  }

  /// The current thread sleeps for the configured amount of time:
  /// 2 * heartbeat
  fn sleep(&self) {
    debug!("ServerHeartbeat::sleep()");

    thread::sleep(self.duration);
  }

  /// Sends the NodeMessage::CheckHeartbeat message to itself, so that the server
  /// can check all the registered nodes.
  fn send_check_heartbeat_message(&self) -> Result<(), Error> {
    debug!("ServerHeartbeat::send_check_heartbeat_message()");
    let message: NodeMessage<(), ()> = NodeMessage::CheckHeartbeat;

    self
      .communicator
      .lock()?
      .send_data(&message, &self.server_socket)
  }
}

/// Some statistics about the server.
/// This is the data that will be send when a
/// NodeMessage::GetStatistics message arrived.
/// More items may be added in the future.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ServerStatistics {
  /// Total number of nodes, includes inactive nodes
  num_of_nodes: usize,
  /// total time from start of server as secs
  time_taken: f64,
  /// Node ids and time since last heartbeat as secs
  hb_time_stamps: Vec<(NodeID, f64)>,
}

/// In here the server handles all the messages and generates appropriate responses.
struct ServerProcess<T, U> {
  /// The port the server will listen to.
  port: u16,
  /// Every n seconds a heartbeat message is sent from the node to the server.
  heartbeat: u64,
  /// Time instance when the server was created
  time_start: Instant,
  /// The user defined data structure that implements the Server trait.
  server: Mutex<T>,
  /// Internal list of all the registered nodes.
  node_list: Mutex<NodeList<U>>,
  /// Indicates if the job is already done and the server can exit its main loop.
  job_done: AtomicBool,
  /// Optional setting if nodes have to move to a new server
  new_server: Mutex<Option<(String, u16)>>,
  /// Handles all the communication
  communicator: Mutex<Communicator>,
}

impl<T: Server> ServerProcess<T, T::CustomMessageT> {
  /// Creates a new ServerProcess with the given user defined server that implements the Server trait
  fn new(config: &Configuration, server: T) -> Self {
    debug!("ServerProcess::new()");

    ServerProcess {
      port: config.port,
      heartbeat: config.heartbeat,
      time_start: Instant::now(),
      server: Mutex::new(server),
      node_list: Mutex::new(NodeList::new()),
      job_done: AtomicBool::new(false),
      new_server: Mutex::new(None),
      communicator: Mutex::new(Communicator::new(config)),
    }
  }

  /// Returns true if the job is finished
  fn is_job_done(&self) -> bool {
    debug!("ServerProcess::is_job_done()");

    self.job_done.load(Ordering::Relaxed)
  }

  /// Returns the total time the server has been running
  fn calc_total_time(&self) -> f64 {
    self.time_start.elapsed().as_secs_f64()
  }

  /// Shut down the server gracefully when the job is done or
  /// when it is requested by the message NodeMessage::ShutDown
  fn shut_down(&self) {
    self.job_done.store(true, Ordering::Relaxed);
  }

  /// All the message that were sent from a node are handled here. It can be on of these types:
  /// - NodeMessage::Register: every new node has to register first, the server then assigns a new node id and sends some optional initial data back to the node with the
  ///   ServerMessage::InitialData message. The server trait method initial_data() is called here.
  /// - NodeMessage::NeedsData: the node needs some data to process and depending on the job state the server answers this request with a ServerMessage::JobStatus message.
  ///   The server trait method prepare_data_for_node() is called here.
  /// - NodeMessage::HeartBeat: the node sends a heartbeat message and the server updates the internal node list with the corresponding current time stamp.
  /// - NodeMessage::HasData: the node has finished processing the data and has sent the result back to the server.
  ///   The server trait method process_data_from_node() is called here.
  /// - NodeMessage::CheckHeartbeat: This message is sent from the check heartbeat thread to the server
  ///   itself. All the nodes will be checked for the heartbeat time stamp and if a node missed it, the Server trait
  ///   method heartbeat_timeout() is called where the node should be marked as offline.
  fn handle_node(&self, mut stream: TcpStream) -> Result<(), Error> {
    debug!("ServerProcess::handle_node()");

    let request: NodeMessage<T::ProcessedDataT, T::CustomMessageT> =
      self.communicator.lock()?.receive_data(&mut stream)?;

    match request {
      NodeMessage::Register => {
        let node_id = self.node_list.lock()?.register_new_node();
        let initial_data = self.server.lock()?.initial_data()?;
        info!("Registering new node: {}, {}", node_id, stream.peer_addr()?);
        self.send_initial_data_message(node_id, initial_data, stream)?;
      }
      NodeMessage::NeedsData(node_id) => {
        debug!("Node {} needs data to process", node_id);

        if let Some((server, port)) = self.new_server.lock()?.clone() {
          self.node_list.lock()?.remove_node(node_id);
          return self.send_new_server_message(server, port, stream);
        }

        if let Some(custom_message) = self.node_list.lock()?.get_message(node_id) {
          debug!("Send custom message to node: {}", node_id);
          return self.send_custom_message(custom_message, stream);
        }

        let data_for_node = self.server.lock()?.prepare_data_for_node(node_id)?;

        match data_for_node {
          JobStatus::Unfinished(data) => {
            debug!("Send data to node");
            self.send_job_status_unfinished(data, stream)?;
          }
          JobStatus::Waiting => {
            debug!("Waiting for other nodes to finish");
            self.send_job_status_waiting(stream)?;
          }
          JobStatus::Finished => {
            debug!("Job is done, will exit handle_node()");
            // Do not bother sending a message to the nodes, they will quit anyways after the retry counter is zero.
            // The counter will be decremented if there is an IO error.
            // Same for the server heartbeat thread, it will exit its loop if there is an IO error.
            self.shut_down();
          }
        }
      }
      NodeMessage::HeartBeat(node_id) => {
        debug!("Got heartbeat from node: {}", node_id);
        self.node_list.lock()?.update_heartbeat(node_id);
      }
      NodeMessage::HasData(node_id, data) => {
        debug!(
          "Node {} has processed some data and we received the results",
          node_id
        );
        self.server.lock()?.process_data_from_node(node_id, &data)?;
      }
      NodeMessage::CheckHeartbeat => {
        debug!("Message CheckHeartbeat received!");
        // Check the heartbeat for all the nodes and call the trait method heartbeat_timeout()
        // with those nodes to react accordingly.
        let nodes = self
          .node_list
          .lock()?
          .check_heartbeat(self.heartbeat)
          .collect::<Vec<NodeID>>();
        self.server.lock()?.heartbeat_timeout(nodes);
      }
      NodeMessage::GetStatistics => {
        debug!("Statistics requested");
        // Gather some statistics and send it to the node that requested it
        let num_of_nodes = self.node_list.lock()?.len();
        let time_taken = self.calc_total_time();
        let hb_time_stamps = self.node_list.lock()?.get_time_stamps();

        let server_statistics = ServerStatistics {
          num_of_nodes,
          time_taken,
          hb_time_stamps,
        };

        self.send_server_statistics(server_statistics, stream)?;
      }
      NodeMessage::ShutDown => {
        debug!("Shut down requested");
        // Shut down server gracefully
        self.shut_down();
      }
      NodeMessage::NewServer(server, port) => {
        debug!(
          "Move all nodes to a new server, address: {}, port: {}",
          server, port
        );
        let mut new_server = self.new_server.lock()?;
        *new_server = Some((server, port));
      }
      NodeMessage::NodeMigrated(node_id) => {
        debug!("Register migrated node: {}", node_id);
        self.node_list.lock()?.migrate_node(node_id);
      }
      NodeMessage::CustomMessage(message, destination) => match destination {
        Some(node_id) => {
          debug!("Add a custom message to node: {}", node_id);
          self.node_list.lock()?.add_message(message, node_id);
        }
        None => {
          debug!("Add a custom message to all nodes");
          self.node_list.lock()?.add_message_all(message);
        }
      },
    }
    Ok(())
  }

  /// Sends the ServerMessage::InitialData message to the node with the given node_id and optional initial_data
  fn send_initial_data_message(
    &self,
    node_id: NodeID,
    initial_data: Option<T::InitialDataT>,
    mut stream: TcpStream,
  ) -> Result<(), Error> {
    debug!("ServerProcess::send_initial_data_message()");
    let message: ServerMessage<T::InitialDataT, T::NewDataT, T::CustomMessageT> =
      ServerMessage::InitialData(node_id, initial_data);

    self.communicator.lock()?.send_data2(&message, &mut stream)
  }

  /// Sends the ServerMessage::JobStatus Unfinished message with the given data to the node,
  fn send_job_status_unfinished(
    &self,
    data: T::NewDataT,
    mut stream: TcpStream,
  ) -> Result<(), Error> {
    debug!("ServerProcess::send_job_status_unfinished_message()");
    let message: ServerMessage<T::InitialDataT, T::NewDataT, T::CustomMessageT> =
      ServerMessage::JobStatus(JobStatus::Unfinished(data));

    self.communicator.lock()?.send_data2(&message, &mut stream)
  }

  /// Send the ServerMessage::JobStatus Waiting message to the node.
  fn send_job_status_waiting(&self, mut stream: TcpStream) -> Result<(), Error> {
    debug!("ServerProcess::send_job_status_waiting()");
    let message: ServerMessage<T::InitialDataT, T::NewDataT, T::CustomMessageT> =
      ServerMessage::JobStatus(JobStatus::Waiting);

    self.communicator.lock()?.send_data2(&message, &mut stream)
  }

  /// Send the ServerMessage::Statistics to the node.
  fn send_server_statistics(
    &self,
    server_statistics: ServerStatistics,
    mut stream: TcpStream,
  ) -> Result<(), Error> {
    debug!("ServerProcess::send_server_statistics()");
    let message: ServerMessage<T::InitialDataT, T::NewDataT, T::CustomMessageT> =
      ServerMessage::Statistics(server_statistics);

    self.communicator.lock()?.send_data2(&message, &mut stream)
  }

  /// Send the ServerMessage::NewServer message to the node.
  fn send_new_server_message(
    &self,
    server: String,
    port: u16,
    mut stream: TcpStream,
  ) -> Result<(), Error> {
    debug!("ServerProcess::send_new_server_message()");
    let message: ServerMessage<T::InitialDataT, T::NewDataT, T::CustomMessageT> =
      ServerMessage::NewServer(server, port);

    self.communicator.lock()?.send_data2(&message, &mut stream)
  }

  /// Send the ServerMessage::Command message to the node.
  fn send_custom_message(
    &self,
    custom_message: T::CustomMessageT,
    mut stream: TcpStream,
  ) -> Result<(), Error> {
    debug!("ServerProcess::send_custom_message()");
    let message: ServerMessage<T::InitialDataT, T::NewDataT, T::CustomMessageT> =
      ServerMessage::CustomMessage(custom_message);

    self.communicator.lock()?.send_data2(&message, &mut stream)
  }
}

pub fn run_server() {
  let configuration = Configuration {
    ..Default::default()
  };

  let chunks: VecDeque<Chunk> = VecDeque::new();
}
