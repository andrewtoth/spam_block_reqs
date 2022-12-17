use std::io::Write;

use anyhow::{anyhow, Result};
use bitcoin::consensus::encode::serialize;
use bitcoin::network::constants;
use bitcoin::network::constants::ServiceFlags;
use bitcoin::network::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::network::message_blockdata::Inventory;
use bitcoin::network::message_network::VersionMessage;
use bitcoin::{Block, BlockHash};
use log::trace;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum BroadcastState {
    Connected,
    AwaitingVersion,
    AwaitingVerack,
    AwaitingBlock,
    Done,
}

pub struct MessageHandler<W: Write + Unpin> {
    writer: W,
    magic: u32,
    block_hash: BlockHash,
    state: BroadcastState,
}

impl<W: Write + Unpin> MessageHandler<W> {
    pub fn new(writer: W, magic: u32, block_hash: BlockHash) -> Self {
        MessageHandler {
            writer,
            magic,
            block_hash,
            state: BroadcastState::Connected,
        }
    }

    pub fn state(&self) -> BroadcastState {
        self.state
    }

    pub fn send_version_msg(&mut self, msg: VersionMessage) -> Result<()> {
        let message = RawNetworkMessage {
            magic: self.magic,
            payload: NetworkMessage::Version(msg),
        };
        let _ = self.writer.write(&serialize(&message))?;
        trace!("Sent version message");
        self.state = BroadcastState::AwaitingVersion;
        Ok(())
    }

    pub fn handle_message(&mut self, msg: NetworkMessage) -> Result<()> {
        match msg {
            NetworkMessage::Version(msg) => {
                if self.state != BroadcastState::AwaitingVersion {
                    return Err(anyhow!("Received version msg out of order"));
                }
                self.handle_version_msg(msg)?;
                self.state = BroadcastState::AwaitingVerack;
            }
            NetworkMessage::Verack => {
                if self.state != BroadcastState::AwaitingVerack {
                    return Err(anyhow!("Received verack msg out of order"));
                }
                self.handle_verack_msg()?;
                self.state = BroadcastState::AwaitingBlock;
            }
            NetworkMessage::WtxidRelay => {
                if self.state != BroadcastState::AwaitingVerack {
                    return Err(anyhow!("Received wtxidrelay msg out of order"));
                }
                self.handle_wtxid_relay_msg()?;
            }
            NetworkMessage::SendAddrV2 => {
                if self.state != BroadcastState::AwaitingVerack {
                    return Err(anyhow!("Received sendaddrv2 msg out of order"));
                }
                trace!("Received sendaddrv2 message");
            }
            NetworkMessage::Block(block) => {
                if self.state != BroadcastState::AwaitingBlock {
                    return Err(anyhow!("Received block msg out of order"));
                }
                self.handle_block(block)?;
                trace!("Block received successfully");
            }
            NetworkMessage::Ping(nonce) => {
                self.handle_ping_msg(nonce)?;
            }
            NetworkMessage::Unknown { command, .. } => {
                let command = command.to_string();
                if self.state != BroadcastState::AwaitingBlock && self.state != BroadcastState::Done
                {
                    return Err(anyhow!(
                        "Received {} msg out of order {:#?}",
                        command,
                        self.state
                    ));
                }
                trace!("Received message: {}", command);
            }
            _ => {
                trace!("Received unknown message: {:?}", msg);
            }
        }
        Ok(())
    }

    fn handle_version_msg(&mut self, version_msg: VersionMessage) -> Result<()> {
        trace!("Received version message");

        if !version_msg.relay {
            return Err(anyhow!("Node does not relay transactions"));
        }

        if !version_msg.services.has(ServiceFlags::WITNESS) {
            return Err(anyhow!("Node does not support segwit"));
        }

        if version_msg.version == constants::PROTOCOL_VERSION {
            let msg = RawNetworkMessage {
                magic: self.magic,
                payload: NetworkMessage::WtxidRelay,
            };
            let _ = self.writer.write_all(serialize(&msg).as_slice())?;
            trace!("Sent wtxid message");
            let msg = RawNetworkMessage {
                magic: self.magic,
                payload: NetworkMessage::SendAddrV2,
            };
            let _ = self.writer.write_all(serialize(&msg).as_slice())?;
            trace!("Sent sendaddrv2 message");
        }

        let msg = RawNetworkMessage {
            magic: self.magic,
            payload: NetworkMessage::Verack,
        };
        let _ = self.writer.write_all(serialize(&msg).as_slice())?;
        trace!("Sent verack message");
        Ok(())
    }

    fn handle_verack_msg(&mut self) -> Result<()> {
        trace!("Received verack message");
        let get_data = Inventory::Block(self.block_hash);
        let msg = RawNetworkMessage {
            magic: self.magic,
            payload: NetworkMessage::GetData(vec![get_data]),
        };
        let _ = self.writer.write_all(serialize(&msg).as_slice())?;
        trace!("Sent getdata message");
        Ok(())
    }

    fn handle_wtxid_relay_msg(&mut self) -> Result<()> {
        trace!("Received wtxid message");
        Ok(())
    }

    fn handle_block(&mut self, block: Block) -> Result<()> {
        trace!("Received block message");
        let get_data = Inventory::Block(self.block_hash);
        let msg = RawNetworkMessage {
            magic: self.magic,
            payload: NetworkMessage::GetData(vec![get_data]),
        };
        let _ = self.writer.write_all(serialize(&msg).as_slice())?;
        trace!("Sent getdata message");
        if block.header.block_hash() != self.block_hash {
            return Err(anyhow!("Block message does not contain our block"));
        }
        Ok(())
    }

    fn handle_ping_msg(&mut self, nonce: u64) -> Result<()> {
        trace!("Received ping message");
        let msg = RawNetworkMessage {
            magic: self.magic,
            payload: NetworkMessage::Pong(nonce),
        };
        let _ = self.writer.write_all(serialize(&msg).as_slice())?;
        trace!("Sent pong message");
        Ok(())
    }
}
