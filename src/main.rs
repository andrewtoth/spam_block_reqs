use anyhow::{anyhow, Result};
use bitcoin::{hashes::hex::FromHex, BlockHash, Network};
use clap::Parser;
use spam_block_reqs::{
    request_blocks, request_blocktxns, request_compact_blocks, request_witness_blocks,
};
use std::{net::TcpStream, sync::mpsc::channel, thread, time::Instant};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Type of request to send
    #[arg(short, long, value_enum, default_value_t = RequestType::WitnessBlock)]
    request_type: RequestType,

    /// Number of connections to create
    #[arg(short, long, default_value_t = 4)]
    connections: u8,

    /// Number of requests to make
    #[arg(short, long, default_value_t = 1000)]
    number: usize,

    /// Block hash to request
    #[arg(
        short,
        long,
        default_value_t = String::from("0000000000000000000592a974b1b9f087cb77628bb4a097d5c2c11b3476a58e")
    )]
    block_hash: String,

    /// ip:port of bitcoind to connect to
    #[arg(short, long, default_value_t = String::from("127.0.0.1:8333"))]
    address: String,

    /// Network to use (bitcoin, testnet, signet, regtest)
    #[arg(short, long, default_value_t = String::from("bitcoin"))]
    network: String,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum RequestType {
    WitnessBlock,
    CompactBlock,
    BlockTransactions,
    LegacyBlock,
}

fn main() -> Result<()> {
    let _ = env_logger::builder()
        .target(env_logger::Target::Stdout)
        .try_init();

    let args = Args::parse();

    let req = args.request_type;
    let connections = args.connections as usize;
    let number = args.number;
    let block_hash = &args.block_hash;
    let address = args.address;
    let magic = match args.network.as_str() {
        "bitcoin" => Network::Bitcoin.magic(),
        "testnet" => Network::Testnet.magic(),
        "signet" => Network::Signet.magic(),
        "regtest" => Network::Regtest.magic(),
        _ => {
            return Err(anyhow!("Invalid network {}", args.network));
        }
    };

    let number = number - number % connections;
    let reqs_per_connection = number / connections;
    let block_hash = BlockHash::from_hex(block_hash)?;

    let (tx, rx) = channel();

    for _ in 0..connections {
        let tx_clone = tx.clone();
        let req_clone = req.clone();
        let address_clone = address.clone();
        thread::spawn(move || {
            let mut stream = match TcpStream::connect(address_clone) {
                Err(e) => {
                    let _ = tx_clone.send(Some(anyhow!("Could not connect: {e}")));
                    return;
                }
                Ok(stream) => stream,
            };
            let res = match req_clone {
                RequestType::WitnessBlock => request_witness_blocks(
                    &mut stream,
                    block_hash,
                    reqs_per_connection,
                    &tx_clone,
                    magic,
                ),
                RequestType::CompactBlock => request_compact_blocks(
                    &mut stream,
                    block_hash,
                    reqs_per_connection,
                    &tx_clone,
                    magic,
                ),
                RequestType::BlockTransactions => request_blocktxns(
                    &mut stream,
                    block_hash,
                    vec![1],
                    reqs_per_connection,
                    &tx_clone,
                    magic,
                ),
                RequestType::LegacyBlock => request_blocks(
                    &mut stream,
                    block_hash,
                    reqs_per_connection,
                    &tx_clone,
                    magic,
                ),
            };
            if res.is_err() {
                let _ = tx_clone.send(res.err());
            }
        });
    }

    let now = Instant::now();
    for _ in 0..number {
        if let Some(err) = rx.recv()? {
            return Err(err);
        }
    }
    let elapsed = now.elapsed();
    println!("Received {number} responses in {:.2?}", elapsed);

    Ok(())
}
