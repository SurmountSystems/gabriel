use std::env;
use std::fs::File;
use std::io::Write;

use bitcoin::consensus::deserialize;
use bitcoin::hex::FromHex;
use bitcoin::Transaction;
use bitcoin::{block::Version, Block, BlockHash, CompactTarget, TxMerkleNode};
use chrono::{TimeZone, Utc};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

#[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Header {
    /// Block version, now repurposed for soft fork signalling.
    pub version: Version,
    /// Reference to the previous block in the chain.
    #[serde(alias = "previousblockhash")]
    pub prev_blockhash: BlockHash,
    /// The root hash of the merkle tree of transactions in the block.
    pub merkle_root: TxMerkleNode,
    /// The timestamp of the block, as claimed by the miner.
    #[serde(alias = "timestamp")]
    pub time: u32,
    /// The target value below which the blockhash must lie.
    pub bits: CompactTarget,
    /// The nonce, selected to obtain a low enough blockhash.
    pub nonce: u32,
}

impl Header {
    /// Convert the `time` field to a date string in the format `MM/DD/YYYY`.
    pub fn formatted_date(&self) -> String {
        let datetime = Utc
            .timestamp_opt(self.time as i64, 0)
            .single()
            .expect("Invalid timestamp");

        datetime.format("%m/%d/%Y %H:%M:%S").to_string()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut out: Vec<String> = vec![];
    out.push("Date,Total P2PK addresses,Total P2PK coins".to_owned());

    let onion = env::var("ONION")?;

    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .build()?;

    let mut p2pk_addresses = 0;
    let mut p2pk_coins = 0;

    for i in 540..1000 {
        let url = format!("https://{onion}.local/api/block-height/{i}");
        println!("{}", url);
        let hash = client.get(url).send()?.text()?;
        let url = format!("https://{onion}.local/api/block/{hash}");
        println!("{}", url);
        let response = client.get(url).send()?.text()?;
        let json: Header = serde_json::from_str(&response)?;
        // println!("{}", serde_json::to_string_pretty(&json)?);
        let url = format!("https://{onion}.local/api/block/{hash}/raw");
        let response = client.get(url).send()?.bytes()?;
        let block: Block = deserialize(&response)?;

        for (i, tx) in block.txdata.iter().enumerate() {
            for outpoint in &tx.output {
                let out_script = outpoint.script_pubkey.to_asm_string();
                if out_script.starts_with("OP_PUSHBYTES_65") {
                    p2pk_addresses += 1;
                }
            }

            println!("tx len: {}", tx.input.len());

            if i > 1 {
                for txin in &tx.input {
                    if !tx.is_coinbase() {
                        let txid = txin.previous_output.txid;
                        let url = format!("https://{onion}.local/api/tx/{txid}/raw");
                        let response = client.get(url).send()?.bytes()?;
                        println!("res len: {}", response.len());
                        const SOME_TX: &str = "0100000001a15d57094aa7a21a28cb20b59aab8fc7d1149a3bdbcddba9c622e4f5f6a99ece010000006c493046022100f93bb0e7d8db7bd46e40132d1f8242026e045f03a0efe71bbb8e3f475e970d790221009337cd7f1f929f00cc6ff01f03729b069a7c21b59b1736ddfee5db5946c5da8c0121033b9b137ee87d5a812d6f506efdd37f0affa7ffc310711c06c7f3e097c9447c52ffffffff0100e1f505000000001976a9140389035a9225b3839e2bbf32d826a1e222031fd888ac00000000";
                        let raw_tx = Vec::from_hex(SOME_TX).unwrap();
                        println!("hex: {}", hex::encode(&response));
                        println!("real len: {}", &response.len());
                        println!("test len: {}", &raw_tx.len());
                        let transaction: Transaction = deserialize(&response)?;
                        for outpoint in transaction.output {
                            let out_script = outpoint.script_pubkey.to_asm_string();
                            if out_script.starts_with("OP_PUSHBYTES_65") {
                                p2pk_addresses -= 1;
                            }
                        }
                    }
                }
            }
        }

        out.push(format!(
            "{},{},{}",
            json.formatted_date(),
            p2pk_addresses,
            p2pk_coins
        ));
    }

    let content = out.join("\n");
    let mut file = File::create("out.csv")?;
    file.write_all(content.as_bytes())?;

    Ok(())
}
