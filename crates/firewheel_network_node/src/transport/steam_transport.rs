use crate::transport::{NetworkNodeTransport, TransportError};
use std::fmt::{Display, Formatter};
use steamworks::networking_messages::NetworkingMessages;
use steamworks::networking_types::{NetworkingIdentity, SendFlags};
use steamworks::{Client, SteamId};
use thiserror::Error;

const RECV_BATCH_SIZE: usize = 10;

#[derive(Error, Debug)]
pub struct SteamTransportConstructionError;

impl Display for SteamTransportConstructionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "The steam transport requires that the SteamNetworkingMessagesTransportConfig is supplied with a valid handle to the steam client"
        )
    }
}

pub struct SteamNetworkingMessagesTransport {
    // Handle for Steam Networking Messages
    networking_messages: NetworkingMessages,

    // Steam Networking Messages channel to send audio data over. This channel should be unique for networking node data
    channel: u32,
}

pub struct SteamNetworkingMessagesTransportConfig {
    /// The handle to the steam networking messages API. This must be Some, or the node creation will fail
    pub steam_client: Option<Client>,

    // Steam Networking Messages channel to send audio data over. This channel should be unique for networking node data
    pub channel: u32,
}

impl Default for SteamNetworkingMessagesTransportConfig {
    fn default() -> Self {
        Self {
            steam_client: None,
            channel: 0,
        }
    }
}

impl NetworkNodeTransport for SteamNetworkingMessagesTransport {
    type Addr = SteamId;
    type Config = SteamNetworkingMessagesTransportConfig;

    fn send(&mut self, data: &[u8], addr: &Self::Addr) {
        self.networking_messages
            .send_message_to_user(
                NetworkingIdentity::new_steam_id(*addr),
                SendFlags::UNRELIABLE_NO_NAGLE,
                data,
                self.channel,
            )
            .unwrap();
    }

    fn receive(&mut self) -> Vec<(Self::Addr, Vec<u8>)> {
        self.networking_messages
            .receive_messages_on_channel(self.channel, RECV_BATCH_SIZE)
            .iter()
            .map(|x| (x.identity_peer().steam_id().unwrap(), x.data().to_vec()))
            .collect()
    }

    fn construct(config: &Self::Config) -> Result<Self, TransportError> {
        Ok(Self {
            networking_messages: config
                .steam_client
                .as_ref()
                .ok_or_else(|| SteamTransportConstructionError)?
                .networking_messages(),
            channel: 0,
        })
    }
}
