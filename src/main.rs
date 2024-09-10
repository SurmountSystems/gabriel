use std::{
    fs::OpenOptions,
    io::{Seek, Write},
    path::PathBuf,
};

use anyhow::Result;
use block::{process_blocks_in_parallel, Record};
use clap::Parser; // Updated import

mod block;
mod tx;

use block::{HeaderMap, ResultMap, TxMap};
use lock_freedom::map::Map;

const HEADER: &str = "Height,Date,Total P2PK addresses,Total P2PK coins";

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)] // Updated attribute
struct Args {
    /// Bitcoin directory path
    #[arg(short, long)] // Updated attribute
    input: PathBuf,

    /// CSV output file path
    #[arg(short, long)] // Updated attribute
    output: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse(); // Ensure `clap::Parser` trait is in scope

    // Maps previous block hash to next merkle root
    let header_map: HeaderMap = Map::new();
    // Maps txid to tx value
    let tx_map: TxMap = Map::new();
    // Maps header hash to result Record
    let result_map: ResultMap = Map::new();

    if let Err(e) = process_blocks_in_parallel(&args.input, &result_map, &tx_map, &header_map) {
        eprintln!("Failed to process blocks: {:?}", e);
    }
    let mut out: Vec<String> = vec![];
    let mut last_block_hash: [u8; 32] =
        hex::decode("6fe28c0ab6f1b372c1a6a246ae63f74f931e8365e15a089c68d6190000000000")
            .unwrap()
            .try_into()
            .expect("slice with incorrect length"); // Genesis block
    let mut height = 0;
    let mut p2pk_addresses = 0;
    let mut p2pk_coins = 0.0;
    while let Some(next_block_hash) = header_map.get(&last_block_hash) {
        // println!("Next block hash: {:?}", hex::encode(next_block_hash.1));
        let record = result_map.get(&next_block_hash.1);
        if let Some(record) = record {
            let Record {
                date,
                p2pk_addresses_added,
                p2pk_sats_added,
                p2pk_addresses_spent,
                p2pk_sats_spent,
            } = &record.1;
            p2pk_addresses += p2pk_addresses_added;
            p2pk_addresses -= p2pk_addresses_spent;
            p2pk_coins += p2pk_sats_added.to_owned() as f64 / 100_000_000.0;
            p2pk_coins -= p2pk_sats_spent.to_owned() as f64 / 100_000_000.0;
            out.push(format!("{height},{date},{p2pk_addresses},{p2pk_coins}"));
        }
        height += 1;
        last_block_hash = next_block_hash.1;
    }

    println!("Height: {}", height);

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&args.output)?;

    // When writing back to the file, ensure we start from the beginning
    file.seek(std::io::SeekFrom::Start(0))?;
    file.set_len(0)?; // Truncate the file

    file.write_all(HEADER.as_bytes())?;
    for line in &out {
        writeln!(file, "{}", line)?;
    }

    Ok(())
}
