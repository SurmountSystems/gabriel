use std::env;
use std::fs::File;
use std::io::Write;

use bitcoin::{
    block::Version, consensus::deserialize, Block, BlockHash, CompactTarget, TxMerkleNode,
};
use chrono::{TimeZone, Utc};
use indicatif::ProgressBar;
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

#[derive(Serialize, Deserialize, Debug)]
pub struct Transaction {
    pub vsize: u32,
    #[serde(rename = "feePerVsize")]
    pub fee_per_vsize: f64,
    #[serde(rename = "effectiveFeePerVsize")]
    pub effective_fee_per_vsize: f64,
    pub txid: String,
    pub version: u32,
    pub locktime: u32,
    pub size: u32,
    pub weight: u32,
    pub fee: u64,
    pub vin: Vec<Vin>,
    pub vout: Vec<Vout>,
    pub status: Status,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Vin {
    pub is_coinbase: bool,
    pub prevout: Option<Prevout>,
    pub scriptsig: String,
    pub scriptsig_asm: String,
    pub sequence: u32,
    pub txid: String,
    pub vout: u32,
    pub witness: Vec<String>,
    pub inner_redeemscript_asm: String,
    pub inner_witnessscript_asm: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Prevout {
    pub value: u64,
    pub scriptpubkey: String,
    pub scriptpubkey_address: String,
    pub scriptpubkey_asm: String,
    pub scriptpubkey_type: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Vout {
    pub value: u64,
    pub scriptpubkey: String,
    pub scriptpubkey_address: String,
    pub scriptpubkey_asm: String,
    pub scriptpubkey_type: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Status {
    pub confirmed: bool,
    pub block_height: u64,
    pub block_hash: String,
    pub block_time: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut out: Vec<String> = vec![];
    out.push("Date,Total P2PK addresses,Total P2PK coins".to_owned());

    let onion = env::var("ONION")?;

    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .build()?;

    let mut p2pk_addresses: i32 = 0;
    let mut p2pk_coins: f64 = 0.0;

    let url = format!("https://{onion}.local/api/blocks/tip/height");
    let response = client.get(url).send()?.text()?;
    let height = response.parse()?;

    let pb = ProgressBar::new(height);

    pb.println(format!("Syncing from blocks 1 to {height}"));

    for i in 1..height {
        let url = format!("https://{onion}.local/api/block-height/{i}");
        pb.println(&url);
        let hash = client.get(url).send()?.text()?;
        let url = format!("https://{onion}.local/api/block/{hash}");
        pb.println(&url);
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
                    p2pk_coins += outpoint.value.to_btc();
                }
            }

            if i > 1 {
                for txin in &tx.input {
                    let txid = txin.previous_output.txid;
                    let url = format!("https://{onion}.local/api/tx/{txid}");
                    pb.println(&url);
                    let response = client.get(url).send()?.text()?;
                    pb.println(&response);
                    let transaction: Transaction = serde_json::from_str(&response)?;

                    for vin in transaction.vin {
                        if vin.is_coinbase {
                            continue;
                        }
                    }

                    for outpoint in transaction.vout {
                        let out_script = outpoint.scriptpubkey_asm;
                        if out_script.starts_with("OP_PUSHBYTES_65") {
                            p2pk_addresses -= 1;
                            p2pk_coins -= (outpoint.value as f64) / 100_000_000.0;
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

        pb.inc(1);
    }

    let content = out.join("\n");
    let mut file = File::create("out.csv")?;
    file.write_all(content.as_bytes())?;

    Ok(())
}
