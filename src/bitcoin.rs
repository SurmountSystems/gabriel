use std::{
    convert::TryInto,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

use bitcoin::opcodes::all::{OP_CHECKSIG, OP_PUSHBYTES_33, OP_PUSHBYTES_65};
use chrono::{TimeZone, Utc};
use indicatif::ProgressBar;
use lock_freedom::map::Map;
use nom::{
    bytes::complete::take,
    number::complete::{le_u32, le_u64},
    IResult,
};
use rayon::prelude::*;

const MAGIC_NUMBER: u32 = 0xD9B4BEF9; // Bitcoin Mainnet magic number

use crate::tx::{Transaction, TransactionInput, TransactionOutput};

#[derive(Debug)]
pub struct BlockHeader {
    #[allow(dead_code)]
    pub version: u32,
    pub previous_block_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub timestamp: u32,
    // bits: u32,
    // nonce: u32,
}

#[derive(Debug)]
pub struct BitcoinBlock {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

pub struct Record {
    pub date: String,
    pub p2pk_addresses_added: u32,
    pub p2pk_sats_added: u64,
    pub p2pk_addresses_spent: u32,
    pub p2pk_sats_spent: u64,
}

pub type HeaderMap = Map<[u8; 32], [u8; 32]>;
pub type TxMap = Map<([u8; 32], u32), u64>;
pub type ResultMap = Map<[u8; 32], Record>;

/// Parses a Bitcoin block header
fn parse_block_header(input: &[u8]) -> IResult<&[u8], BlockHeader> {
    let (input, version) = le_u32(input)?;
    let (input, previous_block_hash) = take(32usize)(input)?;
    let (input, merkle_root) = take(32usize)(input)?;
    let (input, timestamp) = le_u32(input)?;
    let (input, _bits) = le_u32(input)?;
    let (input, _nonce) = le_u32(input)?;

    Ok((
        input,
        BlockHeader {
            version,
            previous_block_hash: previous_block_hash.try_into().unwrap(),
            merkle_root: merkle_root.try_into().unwrap(),
            timestamp,
            // bits,
            // nonce,
        },
    ))
}

/// Parses a varint (variable-length integer)
fn parse_varint(input: &[u8]) -> IResult<&[u8], u64> {
    let (input, first_byte) = take(1usize)(input)?;
    let first_byte = first_byte[0];

    if first_byte < 0xfd {
        Ok((input, first_byte as u64))
    } else if first_byte == 0xfd || first_byte == 0xfe {
        let (input, value) = le_u32(input)?;
        Ok((input, value as u64))
    } else {
        let (input, value) = le_u64(input)?;
        Ok((input, value))
    }
}

/// Parses a transaction input
fn parse_transaction_input(input: &[u8]) -> IResult<&[u8], TransactionInput> {
    let (input, previous_output_txid) = take(32usize)(input)?;
    let (input, previous_output_vout) = le_u32(input)?;
    let (input, script_length) = parse_varint(input)?;
    let (input, script) = take(script_length as usize)(input)?;
    let (input, sequence) = le_u32(input)?;

    Ok((
        input,
        TransactionInput {
            previous_output_txid: previous_output_txid.try_into().unwrap(),
            previous_output_vout,
            script: script.to_vec(),
            sequence,
        },
    ))
}

/// Parses a transaction output
fn parse_transaction_output(input: &[u8]) -> IResult<&[u8], TransactionOutput> {
    let (input, value) = le_u64(input)?;
    let (input, script_length) = parse_varint(input)?;
    let (input, script) = take(script_length as usize)(input)?;

    Ok((
        input,
        TransactionOutput {
            value,
            // script_length,
            script: script.to_vec(),
        },
    ))
}

/// Parses a Bitcoin transaction
fn parse_transaction(input: &[u8]) -> IResult<&[u8], Transaction> {
    let (input, version) = le_u32(input)?;

    let (input, input_count) = parse_varint(input)?;
    let (input, inputs) = nom::multi::count(parse_transaction_input, input_count as usize)(input)?;

    let (input, output_count) = parse_varint(input)?;
    let (input, outputs) =
        nom::multi::count(parse_transaction_output, output_count as usize)(input)?;

    let (input, lock_time) = le_u32(input)?;

    Ok((
        input,
        Transaction {
            version,
            inputs,
            outputs,
            lock_time,
        },
    ))
}

/// Parse the block size and return the size in bytes
fn parse_block_size(input: &[u8]) -> IResult<&[u8], u32> {
    le_u32(input)
}

/// Parses a Bitcoin block
fn parse_block(input: &[u8]) -> IResult<&[u8], BitcoinBlock> {
    let (input, header) = parse_block_header(input)?;

    let (input, transaction_count) = parse_varint(input)?;
    let (input, transactions) =
        nom::multi::count(parse_transaction, transaction_count as usize)(input)?;

    Ok((
        input,
        BitcoinBlock {
            header,
            transactions,
        },
    ))
}

/// Parses a single block, including the magic number and block size
fn parse_block_with_magic(input: &[u8]) -> IResult<&[u8], BitcoinBlock> {
    let (input, magic) = le_u32(input)?;
    if magic != MAGIC_NUMBER {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )));
    }

    let (input, block_size) = parse_block_size(input)?;
    let block_size = block_size as usize;

    let (remaining, block_data) = take(block_size)(input)?;
    let (_, block) = parse_block(block_data)?;

    Ok((remaining, block))
}

/// Parse the entire blkxxxx.dat file, returning a list of blocks and any remaining input
fn parse_blk_file(input: &[u8]) -> IResult<&[u8], Vec<BitcoinBlock>> {
    let mut blocks = Vec::new();
    let mut remaining_input = input;

    while !remaining_input.is_empty() {
        match parse_block_with_magic(remaining_input) {
            Ok((remaining, block)) => {
                blocks.push(block);
                remaining_input = remaining;
            }
            Err(_) => {
                break; // Stop if we can't parse more blocks
            }
        }
    }

    Ok((remaining_input, blocks))
}

fn is_p2pk(script: &[u8]) -> bool {
    match script.len() {
        65 if script[0] == 0x04 => true,
        67 if script[0] == OP_PUSHBYTES_65.to_u8() && script[66] == OP_CHECKSIG.to_u8() => true,
        35 if script[0] == OP_PUSHBYTES_33.to_u8() && script[34] == OP_CHECKSIG.to_u8() => true,
        _ => false,
    }
}

/// Process a single block from the input data
fn process_block(
    input: &[u8],
    pb: &ProgressBar,
    result_map: &ResultMap,
    tx_map: &TxMap,
    header_map: &HeaderMap,
) {
    match parse_blk_file(input) {
        Ok((_, blocks)) => {
            for block in blocks {
                header_map.insert(block.header.previous_block_hash, block.header.merkle_root);

                let mut p2pk_addresses_added = 0;
                let mut p2pk_sats_added = 0;
                let mut p2pk_addresses_spent = 0;
                let mut p2pk_sats_spent = 0;

                for tx in block.transactions {
                    for (i, txout) in tx.outputs.iter().enumerate() {
                        if is_p2pk(&txout.script) {
                            p2pk_addresses_added += 1;
                            p2pk_sats_added += txout.value;
                            tx_map.insert((tx.txid(), i as u32), txout.value);
                        }
                    }

                    for txin in &tx.inputs {
                        let txid = txin.previous_output_txid;
                        let vout = txin.previous_output_vout;
                        let prev_tx = tx_map.get(&(txid, vout));

                        // Check if the specific output being spent was P2PK
                        if let Some(prev_output) = prev_tx {
                            p2pk_addresses_spent += 1;
                            p2pk_sats_spent += prev_output.1;
                        }
                    }
                }

                // Format block header timestamp
                let datetime = Utc
                    .timestamp_opt(block.header.timestamp as i64, 0)
                    .single()
                    .expect("Invalid timestamp");

                let date = datetime.format("%m/%d/%Y %H:%M:%S").to_string();

                result_map.insert(
                    block.header.merkle_root,
                    Record {
                        date,
                        p2pk_addresses_added,
                        p2pk_sats_added,
                        p2pk_addresses_spent,
                        p2pk_sats_spent,
                    },
                );

                pb.inc(1);
            }
        }
        Err(e) => {
            pb.println(format!("Error parsing blk file: {e:?}"));
        }
    }
}

/// Process a single block file (blkxxxxx.dat)
fn process_block_file(
    path: &Path,
    pb: &ProgressBar,
    result_map: &ResultMap,
    tx_map: &TxMap,
    header_map: &HeaderMap,
) {
    let mut file = File::open(path).expect("Failed to open block file");
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .expect("Failed to read block file");
    // Process the blk file containing multiple blocks
    process_block(&buffer, pb, result_map, tx_map, header_map);
}

/// Iterate through the blocks directory and process each blkxxxxx.dat file in parallel
pub fn process_blocks_in_parallel(
    blocks_dir: &Path,
    result_map: &ResultMap,
    tx_map: &TxMap,
    header_map: &HeaderMap,
) -> io::Result<()> {
    let mut blk_files: Vec<PathBuf> = vec![];

    // Iterate through the directory for blkxxxxx.dat files
    for i in 0.. {
        let filename = format!("blk{:05}.dat", i);
        let path = blocks_dir.join(filename);
        if path.exists() {
            blk_files.push(path);
        } else {
            break;
        }
    }

    let pb = ProgressBar::new(blk_files.len() as u64 * 128 + 300_000); // Heuristic

    // Process each file in parallel using Rayon
    blk_files.par_iter().for_each(|path| {
        process_block_file(path, &pb, result_map, tx_map, header_map);

        // Calculate ETA
        let eta_duration = pb.eta();
        let eta_seconds = eta_duration.as_secs();
        let days = eta_seconds / 86400;
        let hours = (eta_seconds % 86400) / 3600;
        let minutes = (eta_seconds % 3600) / 60;
        let seconds = eta_seconds % 60;
        let eta = format!("{:02}:{:02}:{:02}:{:02}", days, hours, minutes, seconds);

        pb.println(format!("Blockfile: {path:?} - ETA: {eta}"));
    });

    Ok(())
}
