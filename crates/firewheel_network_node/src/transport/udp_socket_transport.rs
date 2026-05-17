use crate::transport::{NetworkNodeTransport, TransportError};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

pub struct UdpSocketTransportConfig {
    pub receive_port: u16,
}

impl Default for UdpSocketTransportConfig {
    fn default() -> Self {
        Self {
            receive_port: 16805,
        }
    }
}

/// A network transport that uses straight UDP sockets
pub struct UdpSocketTransport {
    socket: UdpSocket,
}

impl NetworkNodeTransport for UdpSocketTransport {
    type Addr = SocketAddr;
    type Config = UdpSocketTransportConfig;
    const NAME: &'static str = "Udp Socket Transport";

    fn send(&mut self, data: &[u8], addr: &Self::Addr) -> Result<(), TransportError> {
        self.socket.send_to(data, addr)?;

        Ok(())
    }

    fn try_receive(&mut self) -> Result<Vec<(Self::Addr, Vec<u8>)>, TransportError> {
        let mut messages = Vec::new();

        loop {
            let mut buf = Vec::new();

            let (_, address) = match self.socket.recv_from(&mut buf) {
                Ok(pair) => pair,
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        break;
                    }

                    return Err(e.into());
                }
            };

            messages.push((address, buf));
        }

        Ok(messages)
    }

    fn construct(config: &Self::Config) -> Result<Self, TransportError> {
        let socket = UdpSocket::bind(SocketAddr::from((
            IpAddr::from(Ipv4Addr::UNSPECIFIED),
            config.receive_port,
        )))?;

        socket.set_nonblocking(true)?;

        Ok(Self { socket })
    }
}
