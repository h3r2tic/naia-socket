use std::{
    io::Error as IoError,
    net::{IpAddr, SocketAddr, UdpSocket},
};

use log::debug;

use async_trait::async_trait;

use webrtc_unreliable::{
    MessageResult, MessageType, SendError, Server as InnerRtcServer, SessionEndpoint,
};

use futures_channel::mpsc;
use futures_util::{pin_mut, select, FutureExt, StreamExt};

use naia_socket_shared::LinkConditionerConfig;

use super::session::start_session_server;

use crate::{
    error::NaiaServerSocketError, link_conditioner::LinkConditioner, message_sender::MessageSender,
    Packet, ServerSocketTrait,
};

/// A socket server which communicates with clients using an underlying
/// unordered & unreliable network protocol
#[derive(Debug)]
pub struct ServerSocket {
    rtc_server: RtcServer,
    to_client_sender: mpsc::UnboundedSender<Packet>,
    to_client_receiver: mpsc::UnboundedReceiver<Packet>,
}

impl ServerSocket {
    /// Returns a new ServerSocket, listening at the given socket address
    pub async fn listen(
        socket_address: SocketAddr,
        public_address: SocketAddr,
    ) -> Box<dyn ServerSocketTrait> {
        let (to_client_sender, to_client_receiver) = mpsc::unbounded();

        let rtc_server = RtcServer::new(socket_address, public_address).await;

        let socket = ServerSocket {
            rtc_server,
            to_client_sender,
            to_client_receiver,
        };

        start_session_server(socket_address, socket.rtc_server.session_endpoint());

        Box::new(socket)
    }
}

#[async_trait]
impl ServerSocketTrait for ServerSocket {
    async fn receive(&mut self) -> Result<Packet, NaiaServerSocketError> {
        enum Next {
            FromClientMessage(Result<Packet, IoError>),
            ToClientMessage(Packet),
        }

        loop {
            let next = {
                let to_client_receiver_next = self.to_client_receiver.next().fuse();
                pin_mut!(to_client_receiver_next);

                let rtc_server = &mut self.rtc_server;
                let from_client_message_receiver_next = rtc_server.recv().fuse();
                pin_mut!(from_client_message_receiver_next);

                select! {
                    from_client_result = from_client_message_receiver_next => {
                        Next::FromClientMessage(
                            match from_client_result {
                                Ok(msg) => {
                                    Ok(Packet::new(msg.remote_addr, msg.message.as_ref().to_vec()))
                                }
                                Err(err) => { Err(err) }
                            }
                        )
                    }
                    to_client_message = to_client_receiver_next => {
                        Next::ToClientMessage(
                            to_client_message.expect("to server message receiver closed")
                        )
                    }
                }
            };

            match next {
                Next::FromClientMessage(from_client_message) => match from_client_message {
                    Ok(packet) => {
                        return Ok(packet);
                    }
                    Err(err) => {
                        return Err(NaiaServerSocketError::Wrapped(Box::new(err)));
                    }
                },
                Next::ToClientMessage(packet) => {
                    let address = packet.address();

                    match self
                        .rtc_server
                        .send(packet.payload(), MessageType::Binary, &address)
                        .await
                    {
                        Err(_) => {
                            return Err(NaiaServerSocketError::SendError(address));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn get_sender(&mut self) -> MessageSender {
        return MessageSender::new(self.to_client_sender.clone());
    }

    fn with_link_conditioner(
        self: Box<Self>,
        config: &LinkConditionerConfig,
    ) -> Box<dyn ServerSocketTrait> {
        Box::new(LinkConditioner::new(config, self))
    }
}

fn get_available_port(ip: &str) -> Option<u16> {
    (8000..9000).find(|port| port_is_available(ip, *port))
}

fn port_is_available(ip: &str, port: u16) -> bool {
    debug!("Trying to bind to {} {}", ip, port);

    match UdpSocket::bind((ip, port)) {
        Ok(_) => {
            debug!("Was able to bind to {} {}", ip, port);
            true
        }
        Err(_) => false,
    }
}

struct RtcServer {
    inner: InnerRtcServer,
}

impl RtcServer {
    pub async fn new(address: SocketAddr, public_address: SocketAddr) -> RtcServer {
        let inner = InnerRtcServer::new(address, public_address)
            .await
            .expect("could not start RTC server");

        return RtcServer { inner };
    }

    pub fn session_endpoint(&self) -> SessionEndpoint {
        self.inner.session_endpoint()
    }

    pub async fn recv(&mut self) -> Result<MessageResult<'_>, IoError> {
        self.inner.recv().await
    }

    pub async fn send(
        &mut self,
        message: &[u8],
        message_type: MessageType,
        remote_addr: &SocketAddr,
    ) -> Result<(), SendError> {
        self.inner.send(message, message_type, remote_addr).await
    }
}

use std::fmt;
impl fmt::Debug for RtcServer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RtcServer")
    }
}
