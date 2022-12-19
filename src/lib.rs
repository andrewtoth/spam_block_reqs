use anyhow::{anyhow, Error, Result};
use bitcoin::consensus::encode::CheckedData;
use bitcoin::consensus::{serialize, Decodable};
use bitcoin::network::address::Address;
use bitcoin::network::constants::ServiceFlags;
use bitcoin::network::message::{CommandString, NetworkMessage, RawNetworkMessage, MAX_MSG_SIZE};
use bitcoin::network::message_blockdata::Inventory;
use bitcoin::network::message_compact_blocks::GetBlockTxn;
use bitcoin::network::message_network::VersionMessage;
use bitcoin::secp256k1::rand::Rng;
use bitcoin::util::bip152::BlockTransactionsRequest;
use bitcoin::{secp256k1, BlockHash};
use log::trace;
use std::io::{BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn request_witness_blocks(
    stream: &mut TcpStream,
    block_hash: BlockHash,
    number: usize,
    sender: &Sender<Option<Error>>,
    magic: u32,
) -> Result<()> {
    perform_handshake(stream, magic)?;

    let msg = RawNetworkMessage {
        magic,
        payload: NetworkMessage::GetData(vec![Inventory::WitnessBlock(block_hash)]),
    };
    make_requests(stream, msg, number)?;

    receive_responses(stream, "block", sender)?;

    Ok(())
}

pub fn request_blocks(
    stream: &mut TcpStream,
    block_hash: BlockHash,
    number: usize,
    sender: &Sender<Option<Error>>,
    magic: u32,
) -> Result<()> {
    perform_handshake(stream, magic)?;

    let msg = RawNetworkMessage {
        magic,
        payload: NetworkMessage::GetData(vec![Inventory::Block(block_hash)]),
    };
    make_requests(stream, msg, number)?;

    receive_responses(stream, "block", sender)?;

    Ok(())
}

pub fn request_compact_blocks(
    stream: &mut TcpStream,
    block_hash: BlockHash,
    number: usize,
    sender: &Sender<Option<Error>>,
    magic: u32,
) -> Result<()> {
    perform_handshake(stream, magic)?;

    let msg = RawNetworkMessage {
        magic,
        payload: NetworkMessage::GetData(vec![Inventory::CompactBlock(block_hash)]),
    };
    make_requests(stream, msg, number)?;

    receive_responses(stream, "cmpctblock", sender)?;

    Ok(())
}

pub fn request_blocktxns(
    stream: &mut TcpStream,
    block_hash: BlockHash,
    indexes: Vec<u64>,
    number: usize,
    sender: &Sender<Option<Error>>,
    magic: u32,
) -> Result<()> {
    perform_handshake(stream, magic)?;

    let msg = RawNetworkMessage {
        magic,
        payload: NetworkMessage::GetBlockTxn(GetBlockTxn {
            txs_request: BlockTransactionsRequest {
                block_hash,
                indexes,
            },
        }),
    };
    make_requests(stream, msg, number)?;

    receive_responses(stream, "blocktxn", sender)?;

    Ok(())
}

fn perform_handshake(stream: &mut TcpStream, magic: u32) -> Result<()> {
    let version_message = build_version_message()?;
    let message = RawNetworkMessage {
        magic,
        payload: NetworkMessage::Version(version_message),
    };
    let _ = stream.write(&serialize(&message))?;
    trace!("Sent version message");
    let mut reader = BufReader::with_capacity(MAX_MSG_SIZE, stream.try_clone()?);
    loop {
        let reply = RawNetworkMessage::consensus_decode(&mut reader)?;
        match reply.payload {
            NetworkMessage::Version(_) => {
                trace!("Received version message");
                let message = RawNetworkMessage {
                    magic,
                    payload: NetworkMessage::Verack,
                };
                let _ = stream.write(&serialize(&message))?;
                trace!("Sent verack message");
            }
            NetworkMessage::Verack => {
                trace!("Received verack message");
                break;
            }
            _ => {
                trace!("Received message {:?}", reply.payload);
            }
        }
    }
    trace!("Handshake complete");
    Ok(())
}

fn build_version_message() -> Result<VersionMessage> {
    let empty_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);

    let services = ServiceFlags::WITNESS;
    let addr_recv = Address::new(&empty_address, services);
    let addr_from = Address::new(&empty_address, services);
    let nonce: u64 = secp256k1::rand::thread_rng().gen();
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let msg = VersionMessage::new(
        services,
        timestamp.try_into().unwrap(),
        addr_recv,
        addr_from,
        nonce,
        String::from("/BlockSpammer:1.0/"),
        0,
    );
    Ok(msg)
}

fn make_requests<W: Write>(writer: &mut W, msg: RawNetworkMessage, number: usize) -> Result<()> {
    let mut msgs = Vec::with_capacity(number);
    for _ in 0..number {
        msgs.push(serialize(&msg.clone()));
    }
    writer.write(&msgs.into_iter().flatten().collect::<Vec<_>>())?;

    trace!("Sent {number} msgs");

    Ok(())
}

fn receive_responses<R: Read>(
    reader: R,
    command: &str,
    sender: &Sender<Option<Error>>,
) -> Result<()> {
    let mut reader = BufReader::with_capacity(MAX_MSG_SIZE, reader);

    loop {
        let _: u32 = Decodable::consensus_decode_from_finite_reader(&mut reader)?;
        let cmd = CommandString::consensus_decode_from_finite_reader(&mut reader)?;
        let _ = CheckedData::consensus_decode_from_finite_reader(&mut reader)?;
        if cmd.to_string() == command {
            trace!("Received {command} msg");
            let Ok(_) = sender.send(None) else { break; };
        } else if (command == "cmpctblock" || command == "blocktxn") && cmd.to_string() == "block" {
            return Err(anyhow!("Received block response instead of expected {command}. Requested block is too deep in the chain. Try with a block that is <10 blocks deep from chain tip."));
        }
    }

    trace!("Finished receiving");

    Ok(())
}
