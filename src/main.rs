use std::{
    env,
    fs::{File, OpenOptions},
    io::{Read, Seek, Write},
};

use anyhow::Result;
use bitcoincore_rpc::{
    json::{GetChainTipsResultStatus, GetChainTipsResultTip},
    Auth, Client, RpcApi,
};
use chrono::{TimeZone, Utc};
use indicatif::ProgressBar;

const HEADER: &str = "Height,Date,Total P2PK addresses,Total P2PK coins";

fn main() -> Result<()> {
    let mut out: Vec<String> = vec![];

    // Open the file if it exists, otherwise create it
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open("out.csv")?;

    // Read the file content into a string
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    // Check if the file is empty or doesn't start with the header
    if content.is_empty() || !content.starts_with(HEADER) {
        // If empty or no header, add the header to the beginning of out
        out.push(HEADER.to_owned());
    }

    // Split the content into lines and collect into the out vector
    out.extend(content.lines().map(|line| line.to_string()));

    // Get the last line of the CSV file and parse the height from it
    let resume_height = if let Some(last_line) = out.last() {
        let fields: Vec<&str> = last_line.split(',').collect();
        if let Some(height_str) = fields.first() {
            height_str.parse::<u64>().unwrap_or(1)
        } else {
            1
        }
    } else {
        1
    };

    // If the file only contains the header, set the resume height to 1
    let resume_height = if resume_height == 0 { 1 } else { resume_height };

    // Get the last line of the CSV file and parse the P2PK addresses and coins from it
    let mut p2pk_addresses: i32 = if let Some(last_line) = out.last() {
        let fields: Vec<&str> = last_line.split(',').collect();
        if fields.len() >= 3 {
            fields[2].parse().unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };
    let mut p2pk_coins: f64 = if let Some(last_line) = out.last() {
        let fields: Vec<&str> = last_line.split(',').collect();
        if fields.len() >= 4 {
            fields[3].parse().unwrap_or(0.0)
        } else {
            0.0
        }
    } else {
        0.0
    };

    // RPC connection
    let url = env::var("URL")?;
    let cookie = env::var("COOKIE");
    let auth = match cookie {
        Ok(cookiefile) => Auth::CookieFile(cookiefile.into()),
        Err(_) => {
            let user = env::var("USER")?;
            let pass = env::var("PASS")?;

            Auth::UserPass(user, pass)
        }
    };
    let rpc = Client::new(&url, auth)?;

    // Get chain height from chain tip
    let result = rpc.get_chain_tips()?;
    let tip_height = result
        .iter()
        .filter(|fork: &&GetChainTipsResultTip| fork.status == GetChainTipsResultStatus::Active)
        .collect::<Vec<_>>()
        .first()
        .unwrap()
        .height;

    // Progress bar
    let pb = ProgressBar::new(tip_height);
    pb.inc(resume_height - 1);
    pb.println(format!(
        "Syncing from blocks {resume_height} to {tip_height}"
    ));

    // For each block, account for P2PK coins
    for height in resume_height..tip_height {
        let hash = rpc.get_block_hash(height)?;
        let block = rpc.get_block(&hash)?;

        // Account for the new P2PK coins
        for tx in block.txdata.iter() {
            for outpoint in &tx.output {
                if outpoint.script_pubkey.is_p2pk() {
                    p2pk_addresses += 1;
                    p2pk_coins += outpoint.value.to_btc();
                }
            }

            // If the transaction is not coinbase, account for the spent coins
            if !tx.is_coinbase() {
                for txin in &tx.input {
                    let txid = txin.previous_output.txid;
                    let vout = txin.previous_output.vout;
                    let prev_tx = rpc.get_raw_transaction(&txid, None)?;

                    // Check if the specific output being spent was P2PK
                    if let Some(prev_output) = prev_tx.output.get(vout as usize) {
                        if prev_output.script_pubkey.is_p2pk() {
                            p2pk_addresses -= 1;
                            p2pk_coins -= prev_output.value.to_btc();
                        }
                    }
                }
            }
        }

        // Format block header timestamp
        let datetime = Utc
            .timestamp_opt(block.header.time as i64, 0)
            .single()
            .expect("Invalid timestamp");

        let formatted_date = datetime.format("%m/%d/%Y %H:%M:%S").to_string();

        // Append the new line to the CSV file
        out.push(format!(
            "{height},{formatted_date},{p2pk_addresses},{p2pk_coins}",
        ));

        // Calculate ETA
        let eta_duration = pb.eta();
        let eta_seconds = eta_duration.as_secs();
        let days = eta_seconds / 86400;
        let hours = (eta_seconds % 86400) / 3600;
        let minutes = (eta_seconds % 3600) / 60;
        let seconds = eta_seconds % 60;
        let eta = format!("{:02}:{:02}:{:02}:{:02}", days, hours, minutes, seconds);

        pb.println(format!("Block: {height} - ETA: {eta}"));

        // Write the new content to the file for every 1000 blocks
        if height % 1000 == 0 {
            let content = out.join("\n");
            let mut file = File::create("out.csv")?;
            file.write_all(content.as_bytes())?;
            pb.println("FILE SUCCESSFULLY SAVED TO DISK");
        }

        pb.inc(1);
    }

    // When writing back to the file, ensure we start from the beginning
    file.seek(std::io::SeekFrom::Start(0))?;
    file.set_len(0)?; // Truncate the file
    for line in &out {
        writeln!(file, "{}", line)?;
    }

    Ok(())
}
