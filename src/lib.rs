mod message_handler;

use anyhow::Result;
use bitcoin::consensus::Decodable;
use bitcoin::hashes::hex::FromHex;
use bitcoin::network::address::Address;
use bitcoin::network::constants::ServiceFlags;
use bitcoin::network::message::RawNetworkMessage;
use bitcoin::network::message_network::VersionMessage;
use bitcoin::secp256k1::rand::Rng;
use bitcoin::Network;
use bitcoin::{secp256k1, BlockHash};
use log::{info, trace};
use message_handler::{BroadcastState, MessageHandler};
use std::io::{BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Config options for sending
pub struct Config {
    /// The user agent for the initial version message
    ///
    /// Defaults to `"/Satoshi:23.0.0/"`
    pub user_agent: String,
    /// The block height for the initial version message
    ///
    /// Defaults to `749_000`
    pub block_height: i32,
    /// The network to use
    ///
    /// Defaults to [`Network::Bitcoin`]
    pub network: Network,
    /// The timeout duration for the initial connection to the node
    ///
    /// Default is 30 seconds but that might not be long enough for tor
    pub connection_timeout: Duration,
    /// The timeout duration for the handshake, sending inv, receiving getdata,
    /// and finally sending the tx message
    /// Note that if a node already has the tx then it will not respond with
    /// getdata so a timeout here does not necessarily mean the node does not
    /// have the tx
    ///
    /// Default is 30 seconds
    pub send_tx_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            user_agent: String::from("/Satoshi:23.0.0/"),
            block_height: 749_000,
            network: Network::Bitcoin,
            connection_timeout: Duration::from_secs(30),
            send_tx_timeout: Duration::from_secs(30),
        }
    }
}

pub fn begin_requesting(address: SocketAddr, config: Option<Config>) -> Result<()> {
    let config = config.unwrap_or_default();
    let stream = TcpStream::connect(address)?;

    info!("Connected to node at {:?}", address);

    let version_message = build_version_message(&config, Some(address))?;

    let mut message_handler = MessageHandler::new(
        stream.try_clone()?,
        config.network.magic(),
        BlockHash::from_hex("0000000000000000000592a974b1b9f087cb77628bb4a097d5c2c11b3476a58e")?,
    );
    message_handler.send_version_msg(version_message)?;

    let result = message_loop(stream.try_clone()?, &mut message_handler);

    if let Ok(_) = result {
        info!("Received block successfully");
    }

    trace!("Disconnecting");
    // Ignore error on shutdown, since we might have already broadcasted successfully
    let _ = stream.shutdown(std::net::Shutdown::Both);

    result
}

fn message_loop<W: Write + Unpin, R: Read + Unpin + Send>(
    read_stream: R,
    message_handler: &mut MessageHandler<W>,
) -> Result<()> {
    let mut reader = BufReader::with_capacity(4_000_000, read_stream);
    loop {
        let reply = RawNetworkMessage::consensus_decode(&mut reader)?;
        message_handler.handle_message(reply.payload)?;
        if message_handler.state() == BroadcastState::Done {
            break;
        }
    }
    Ok(())
}

fn build_version_message(config: &Config, address: Option<SocketAddr>) -> Result<VersionMessage> {
    let empty_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);

    let services = ServiceFlags::WITNESS;
    let addr_recv = match address {
        Some(addr) => Address::new(&addr, services),
        None => Address::new(&empty_address, services),
    };
    let addr_from = Address::new(&empty_address, services);
    let nonce: u64 = secp256k1::rand::thread_rng().gen();
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let msg = VersionMessage::new(
        services,
        timestamp.try_into().unwrap(),
        addr_recv,
        addr_from,
        nonce,
        config.user_agent.clone(),
        config.block_height,
    );
    Ok(msg)
}
