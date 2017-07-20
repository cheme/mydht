

// TODO implement : client mode in rules!!
#[derive(Debug,PartialEq,Eq,RustcEncodable,RustcDecodable,Clone)]
pub enum ClientMode {
  /// client run from PeerManagement and does not loop
  /// - bool say if we spawn a thread or not
  Local(bool),
  /// client stream run in its own stream
  /// one thread by client for sending
  ThreadedOne,
  /// max n clients by threads
  ThreadedMax(usize),
  /// n threads sharing all client
  ThreadPool(usize),
}
