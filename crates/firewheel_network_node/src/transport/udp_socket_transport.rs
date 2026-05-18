use crate::transport::{NetworkNodeTransport, TransportError};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

pub(crate) const UDP_SOCKET_TRANSPORT_RECV_BUFF_SIZE: usize = 512;

pub struct UdpSocketTransportConfig {
    pub port: u16,
}

impl Default for UdpSocketTransportConfig {
    fn default() -> Self {
        Self { port: 16805 }
    }
}

/// A network transport that uses straight UDP sockets
pub struct UdpSocketTransport {
    socket: UdpSocket,
    port: u16,

    recv_buffer: [u8; UDP_SOCKET_TRANSPORT_RECV_BUFF_SIZE],
}

impl NetworkNodeTransport for UdpSocketTransport {
    type Addr = IpAddr;
    type Config = UdpSocketTransportConfig;
    const NAME: &'static str = "Udp Socket Transport";

    fn send(&mut self, data: &[u8], addr: &Self::Addr) -> Result<(), TransportError> {
        self.socket
            .send_to(data, SocketAddr::new(*addr, self.port))?;

        Ok(())
    }

    fn try_receive(&mut self) -> Result<Vec<(Self::Addr, Vec<u8>)>, TransportError> {
        let mut messages = Vec::new();

        loop {
            let (len, address) = match self.socket.recv_from(&mut self.recv_buffer) {
                Ok(pair) => pair,
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        break;
                    }

                    return Err(e.into());
                }
            };

            messages.push((address.ip(), self.recv_buffer[0..len].to_vec()));
        }

        Ok(messages)
    }

    fn construct(config: &Self::Config) -> Result<Self, TransportError> {
        let socket = UdpSocket::bind(SocketAddr::from((
            IpAddr::from(Ipv4Addr::UNSPECIFIED),
            config.port,
        )))?;

        socket.set_nonblocking(true)?;

        Ok(Self {
            socket,
            port: config.port,
            recv_buffer: [0u8; UDP_SOCKET_TRANSPORT_RECV_BUFF_SIZE],
        })
    }
}
