use std::{
    collections::BTreeMap,
    convert::TryInto,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use bitcoin::opcodes::all::{OP_CHECKSIG, OP_PUSHBYTES_33, OP_PUSHBYTES_65};
use chrono::{TimeZone, Utc};
use indicatif::ProgressBar;
use log::debug;
use nom::{
    bytes::complete::take,
    number::complete::{le_u16, le_u32, le_u64, le_u8},
    IResult,
};
use rayon::prelude::*;
use sha2::{Digest, Sha256};

const MAGIC_NUMBER: u32 = 0xD9B4BEF9; // Bitcoin Mainnet magic number

use crate::tx::{Transaction, TransactionInput, TransactionOutput, WitnessItem};

#[derive(Debug)]
pub struct BlockHeader {
    pub version: u32,
    pub previous_block_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub timestamp: u32,
    pub target: u32,
    pub nonce: u32,
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

pub type HeaderMap = Arc<RwLock<BTreeMap<[u8; 32], [u8; 32]>>>;
pub type TxMap = Arc<RwLock<BTreeMap<([u8; 32], u16), Transaction>>>;
pub type ResultMap = Arc<RwLock<BTreeMap<[u8; 32], Record>>>;

/// Parses a Bitcoin block header
fn parse_block_header(input: &[u8]) -> IResult<&[u8], BlockHeader> {
    let (input, version) = le_u32(input)?;
    let (input, previous_block_hash) = take(32usize)(input)?;
    let (input, merkle_root) = take(32usize)(input)?;
    let (input, timestamp) = le_u32(input)?;
    let (input, bits) = le_u32(input)?;
    let (input, nonce) = le_u32(input)?;

    Ok((
        input,
        BlockHeader {
            version,
            previous_block_hash: previous_block_hash.try_into().unwrap(),
            merkle_root: merkle_root.try_into().unwrap(),
            timestamp,
            target: bits,
            nonce,
        },
    ))
}

// fn last_block_header_prefix(file: &mut File, marker: u32) -> Option<u64> {
//     let file_len = file.metadata().unwrap().len();
//     let mut buffer = [0u8; 4];
//     let mut offset = file_len - 4;
//     file.seek(SeekFrom::End(-4)).unwrap();
//     while offset <= file_len - 4 {
//         file.read_exact(&mut buffer).unwrap();
//         let val = u32::from_le_bytes(buffer);

//         if val == marker {
//             return Some(offset);
//         }
//         offset -= 1;
//         file.seek(SeekFrom::Start(offset)).unwrap();
//     }
//     None
// }

/// Compute block hash from header
fn compute_block_hash(header: &BlockHeader) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(header.version.to_le_bytes());
    hasher.update(header.previous_block_hash);
    hasher.update(header.merkle_root);
    hasher.update(header.timestamp.to_le_bytes());
    hasher.update(header.target.to_le_bytes());
    hasher.update(header.nonce.to_le_bytes());
    let first_hash = hasher.finalize();

    let mut hasher = Sha256::new();
    hasher.update(first_hash);
    hasher.finalize().into()
}

/// Parses a varint (variable-length integer)
fn parse_varint(input: &[u8]) -> IResult<&[u8], u64> {
    let (input, first_byte) = le_u8(input)?;

    match first_byte {
        0..=0xfc => Ok((input, first_byte as u64)),
        0xfd => {
            let (input, value) = le_u16(input)?;
            Ok((input, value as u64))
        }
        0xfe => {
            let (input, value) = le_u32(input)?;
            Ok((input, value as u64))
        }
        0xff => {
            let (input, value) = le_u64(input)?;
            Ok((input, value))
        }
    }
}

/// Parses a transaction input
fn parse_transaction_input(input: &[u8]) -> IResult<&[u8], TransactionInput> {
    let (input, previous_output_txid) = take(32usize)(input)?;
    let (input, previous_output_vout) = le_u32(input)?;
    let (input, script_length) = parse_varint(input)?;
    let (input, script) = take(script_length as usize)(input)?;
    let (input, sequence) = le_u32(input)?;

    let mut previous_output_txid: [u8; 32] = previous_output_txid.try_into().unwrap();
    previous_output_txid.reverse();

    Ok((
        input,
        TransactionInput {
            previous_output_txid,
            previous_output_vout: previous_output_vout as u16,
            script: script.to_vec(),
            sequence,
            witness: Vec::new(),
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
            script: script.to_vec(),
        },
    ))
}

// /// Parses a witness item
// fn parse_witness_item(input: &[u8]) -> IResult<&[u8], WitnessItem> {
//     let (input, witness_field_length) = parse_varint(input)?;
//     let (input, witness) = take(witness_field_length as usize)(input)?;

//     Ok((
//         input,
//         WitnessItem {
//             witness: witness.to_vec(),
//         },
//     ))
// }

/// Parses a Bitcoin transaction referencing the marker and flag by array indices.
fn parse_transaction(input: &[u8]) -> IResult<&[u8], Transaction> {
    let mut offset = 0;

    // Ensure there are at least 4 bytes for the version
    if input.len() < offset + 4 {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TooLarge,
        )));
    }

    // Parse version (4 bytes, Little-Endian)
    let version = u32::from_le_bytes(input[offset..offset + 4].try_into().unwrap());
    debug!("Transaction Version: {}", version);
    offset += 4;

    // Initialize is_segwit flag
    let mut is_segwit = false;

    // Check if there are enough bytes to read marker and flag
    if input.len() > offset + 2 {
        let marker = input[offset];
        let flag = input[offset + 1];
        if marker == 0x00 && flag == 0x01 {
            is_segwit = true;
            debug!("SegWit Transaction Detected");
            offset += 2; // Consume marker and flag
        } else {
            debug!("Legacy Transaction Detected");
        }
    } else {
        debug!("Insufficient bytes for marker and flag, assuming Legacy Transaction");
    }

    // Parse input count (VarInt)
    let (remaining, input_count) = parse_varint(&input[offset..])?;
    let consumed = input.len() - remaining.len() - offset;
    offset += consumed;
    debug!("Input Count: {}", input_count);

    // Parse inputs
    let mut inputs = Vec::with_capacity(input_count as usize);
    for i in 0..input_count {
        let (new_remaining, txin) = parse_transaction_input(&input[offset..])?;
        let consumed = input[offset..].len() - new_remaining.len();
        offset += consumed;
        debug!("Parsed Input {}: {:?}", i + 1, txin.script.len());
        inputs.push(txin);
    }

    // Parse output count (VarInt)
    let (remaining, output_count) = parse_varint(&input[offset..])?;
    let consumed = input.len() - remaining.len() - offset;
    offset += consumed;
    debug!("Output Count: {}", output_count);

    // Parse outputs
    let mut outputs = Vec::with_capacity(output_count as usize);
    for i in 0..output_count {
        let (new_remaining, txout) = parse_transaction_output(&input[offset..])?;
        let consumed = input[offset..].len() - new_remaining.len();
        offset += consumed;
        debug!("Parsed Output {}: {:?}", i + 1, txout.script.len());
        outputs.push(txout);
    }

    // Parse witness data if SegWit
    if is_segwit {
        for (i, _txin) in inputs.iter_mut().enumerate() {
            let (new_remaining, witness) = parse_witness(&input[offset..])?;
            let consumed = input[offset..].len() - new_remaining.len();
            offset += consumed;
            debug!("Parsed Witness for Input {}: {:?}", i + 1, witness.len());
            // txin.witness = witness;
        }
    }

    // Parse lock_time (4 bytes, Little-Endian)
    if input.len() < offset + 4 {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TooLarge,
        )));
    }
    let lock_time = u32::from_le_bytes(input[offset..offset + 4].try_into().unwrap());
    debug!("Lock Time: {}", lock_time);
    offset += 4;

    // Ensure we've consumed the correct amount of bytes
    let remaining = &input[offset..];
    if !remaining.is_empty() {
        debug!(
            "Warning: Remaining bytes after parsing transaction: {}",
            remaining.len()
        );
    }

    let mut hasher = Sha256::new();
    hasher.update(version.to_le_bytes());

    // Use TransactionInputVec wrapper for inputs without cloning
    let input_vec = TransactionInputVec(&inputs);
    hasher.update(input_vec.as_ref());

    // Use TransactionOutputVec wrapper for outputs without cloning
    let output_vec = TransactionOutputVec(&outputs);
    hasher.update(output_vec.as_ref());

    hasher.update(lock_time.to_le_bytes());
    let txid: [u8; 32] = hasher.finalize().into();

    let mut hasher = Sha256::new();
    hasher.update(txid);
    let txid: [u8; 32] = hasher.finalize().into();

    Ok((
        remaining,
        Transaction {
            version,
            inputs,
            outputs,
            lock_time,
            txid,
        },
    ))
}

/// Implement AsRef<[u8]> for TransactionOutput
impl TransactionOutput {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(8 + 8 + self.script.len());
        buffer.extend_from_slice(&self.value.to_le_bytes());
        buffer.extend_from_slice(&(self.script.len() as u64).to_le_bytes());
        buffer.extend_from_slice(&self.script);
        buffer
    }
}

// We can't implement AsRef<[u8]> for Vec<TransactionOutput> directly
// because Vec is not defined in our crate. Instead, we'll create a wrapper type.
pub struct TransactionOutputVec<'a>(pub &'a [TransactionOutput]);

/// Implement AsRef<[u8]> for TransactionOutputVec
impl AsRef<[u8]> for TransactionOutputVec<'_> {
    fn as_ref(&self) -> &[u8] {
        // This implementation is not efficient and should be used with caution
        // It allocates a new buffer on each call
        let buffer: Vec<u8> = self.0.iter().flat_map(|output| output.to_bytes()).collect();
        Box::leak(buffer.into_boxed_slice())
    }
}

pub struct TransactionInputVec<'a>(pub &'a [TransactionInput]);

/// TransactionInputVec implements AsRef<[u8]>
impl AsRef<[u8]> for TransactionInputVec<'_> {
    fn as_ref(&self) -> &[u8] {
        // This implementation is not efficient and should be used with caution
        // It allocates a new buffer on each call
        let buffer: Vec<u8> = self.0.iter().flat_map(|input| input.to_bytes()).collect();
        Box::leak(buffer.into_boxed_slice())
    }
}

// Add a to_bytes method to TransactionInput
impl TransactionInput {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&self.previous_output_txid);
        buffer.extend_from_slice(&self.previous_output_vout.to_le_bytes());
        buffer.extend_from_slice(&(self.script.len() as u64).to_le_bytes());
        buffer.extend_from_slice(&self.script);
        buffer.extend_from_slice(&self.sequence.to_le_bytes());
        buffer
    }
}

/// Parses the witness data for a single input
fn parse_witness(input: &[u8]) -> IResult<&[u8], Vec<WitnessItem>> {
    let (input, stack_item_count) = parse_varint(input)?;
    let mut witness_items = Vec::new();

    let mut remaining = input;
    for _ in 0..stack_item_count {
        let (new_input, size) = parse_varint(remaining)?;
        let (new_input, data) = take(size as usize)(new_input)?;
        witness_items.push(WitnessItem {
            witness: data.to_vec(),
        });
        remaining = new_input;
    }

    Ok((remaining, witness_items))
}

/// Parse the block size and return the size in bytes
fn parse_block_size(input: &[u8]) -> IResult<&[u8], u32> {
    le_u32(input)
}

/// Parses a Bitcoin block
fn parse_block(input: &[u8]) -> IResult<&[u8], BitcoinBlock> {
    let (input, header) = parse_block_header(input)?;

    debug!("Parse block input len: {:?}", input.len());

    let (input, transaction_count) = parse_varint(input)?;

    debug!("Transaction count: {:?}", transaction_count);

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

    debug!("Block size: {:?}", block_size);

    let (remaining, block_data) = take(block_size)(input)?;

    debug!("Block data len: {:?}", block_data.len());
    debug!("Remaining len: {:?}", remaining.len());

    let (block_remainder, block) = parse_block(block_data)?;

    assert!(block_remainder.is_empty());

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
            Err(err) => {
                // write error to error.txt
                debug!("Input len: {:?}", input.len());
                let mut file = File::create("error.txt").expect("Failed to create error file");
                file.write_all(err.to_string().as_bytes())
                    .expect("Failed to write to error file");
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
) -> usize {
    let mut blocks_processed = 0;

    match parse_blk_file(input) {
        Ok((_, blocks)) => {
            for block in blocks {
                let block_hash = compute_block_hash(&block.header);

                header_map
                    .write()
                    .unwrap()
                    .insert(block.header.previous_block_hash, block_hash);

                // pb.println(format!(
                //     "Block: {:?} - Hash: {:?}",
                //     hex::encode(block.header.previous_block_hash),
                //     hex::encode(compute_block_hash(&block.header))
                // ));

                let mut p2pk_addresses_added = 0;
                let mut p2pk_sats_added = 0;
                let mut p2pk_addresses_spent = 0;
                let mut p2pk_sats_spent = 0;

                for tx in block.transactions {
                    for (i, txout) in tx.outputs.iter().enumerate() {
                        if is_p2pk(&txout.script) {
                            p2pk_addresses_added += 1;
                            p2pk_sats_added += txout.value;
                            tx_map
                                .write()
                                .unwrap()
                                .insert((tx.txid(), i as u16), tx.clone());
                        }
                    }

                    let tx_map_read = tx_map.read().unwrap();

                    for txin in &tx.inputs {
                        let txid = txin.previous_output_txid;
                        let vout = txin.previous_output_vout;
                        let prev_tx = tx_map_read.get(&(txid, vout));

                        // Check if the specific output being spent was P2PK
                        if let Some(prev_output) = prev_tx {
                            let prevout = &prev_output.outputs[vout as usize];
                            if is_p2pk(&prevout.script) {
                                p2pk_addresses_spent += 1;
                                p2pk_sats_spent += prevout.value;
                            }
                        }
                    }
                }

                // Format block header timestamp
                let datetime = Utc
                    .timestamp_opt(block.header.timestamp as i64, 0)
                    .single()
                    .expect("Invalid timestamp");

                let date = datetime.format("%m/%d/%Y %H:%M:%S").to_string();

                result_map.write().unwrap().insert(
                    block_hash,
                    Record {
                        date,
                        p2pk_addresses_added,
                        p2pk_sats_added,
                        p2pk_addresses_spent,
                        p2pk_sats_spent,
                    },
                );

                blocks_processed += 1;
            }
        }
        Err(e) => {
            pb.println(format!("Error parsing blk file: {:#?}", e));
        }
    }

    blocks_processed
}

/// Process a single block file (blkxxxxx.dat)
fn process_block_file(
    path: &Path,
    pb: &ProgressBar,
    result_map: &ResultMap,
    tx_map: &TxMap,
    header_map: &HeaderMap,
) -> usize {
    let mut file = File::open(path).expect("Failed to open block file");
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .expect("Failed to read block file");
    // Process the blk file containing multiple blocks
    process_block(&buffer, pb, result_map, tx_map, header_map)
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

    let pb = ProgressBar::new(blk_files.len() as u64);

    // Process each file in parallel using Rayon
    blk_files.par_iter().for_each(|path| {
        let blocks_processed = process_block_file(path, &pb, result_map, tx_map, header_map);

        // Calculate ETA
        let eta_duration = pb.eta();
        let eta_seconds = eta_duration.as_secs();
        let hours = (eta_seconds % 86400) / 3600;
        let minutes = (eta_seconds % 3600) / 60;
        let seconds = eta_seconds % 60;
        let eta = format!("{:02}:{:02}:{:02}", hours, minutes, seconds);

        pb.println(format!(
            "Blockfile: {:?} - ETA: {} - Blocks processed: {}",
            path, eta, blocks_processed
        ));
        pb.inc(1);
    });

    Ok(())
}
