use std::error::Error;

use crate::Packet;

use futures_channel;

/// Handles sending messages to a Client that has established a connection with
/// the Server socket
#[derive(Debug)]
pub struct MessageSender {
    internal: futures_channel::mpsc::UnboundedSender<Packet>,
}

impl MessageSender {
    /// Create a new MessageSender, given a reference to a async channel
    /// connected to the RtcServer
    pub fn new(sender: futures_channel::mpsc::UnboundedSender<Packet>) -> MessageSender {
        MessageSender { internal: sender }
    }

    /// Send a Packet to a client
    pub fn send(&mut self, packet: Packet) -> Result<(), Box<dyn Error + Send>> {
        match self.internal.unbounded_send(packet) {
            Ok(content) => Ok(content),
            Err(error) => {
                return Err(Box::new(error));
            }
        }
    }
}
