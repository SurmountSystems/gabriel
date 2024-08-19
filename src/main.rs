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
use chrono::{Duration, TimeZone, Utc};
use indicatif::ProgressBar;

const HEADER: &str = "Height,Date,Total P2PK addresses,Total P2PK coins";

fn main() -> Result<()> {
    let mut out: Vec<String> = vec![];
    out.push(HEADER.to_owned());

    // Open the file if it exists, otherwise create it and write the HEADER.
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open("out.csv")?;

    // Check if the file is empty by checking its length
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let lines = content.split("\n").collect::<Vec<&str>>();
    if lines.is_empty() {
        out.push(HEADER.to_owned());
    }

    // Rewind the file to the beginning so you can read from it again
    file.rewind()?;

    // Read the file content into a vector of strings
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    out = content.lines().map(|line| line.to_string()).collect();

    // Get the last line of the CSV file and set it as the resume height, parsing the height from the date
    let last_line = lines.last().unwrap().split(",").collect::<Vec<&str>>();
    let resume_height: &&str = last_line.first().unwrap_or(&"1");

    // If the file is empty, set the resume height to 1
    let resume_height = if resume_height.contains("") {
        1
    } else {
        resume_height.parse::<u64>()?
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

    // Results
    let mut p2pk_addresses: i32 = 0;
    let mut p2pk_coins: f64 = 0.0;

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
    pb.println(format!(
        "Syncing from blocks {resume_height} to {tip_height}"
    ));

    // For each block, account for P2PK coins
    for height in resume_height..tip_height {
        let hash = rpc.get_block_hash(height)?;
        let block = rpc.get_block(&hash)?;

        // Account for the new P2PK coins
        for (i, tx) in block.txdata.iter().enumerate() {
            for outpoint in &tx.output {
                if outpoint.script_pubkey.is_p2pk() {
                    p2pk_addresses += 1;
                    p2pk_coins += outpoint.value.to_btc();
                }
            }

            // If the transaction is not from the coinbase, account for the spent coins
            if i > 1 {
                for txin in &tx.input {
                    let txid = txin.previous_output.txid;
                    let transaction = rpc.get_raw_transaction(&txid, None)?;

                    if transaction.is_coinbase() {
                        continue;
                    }

                    // Account for the spent P2PK coins
                    for outpoint in transaction.output {
                        if outpoint.script_pubkey.is_p2pk() {
                            p2pk_addresses -= 1;
                            p2pk_coins -= outpoint.value.to_btc();
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
        let eta_datetime =
            Utc::now() + Duration::from_std(eta_duration).unwrap_or_else(|_| Duration::seconds(0));
        let eta_hms = eta_datetime.format("%d:%H:%M:%S").to_string();

        pb.println(format!("Block: {height} - ETA: {eta_hms}"));

        // Write the new content to the file
        let content = out.join("\n");
        let mut file = File::create("out.csv")?;
        file.write_all(content.as_bytes())?;

        pb.inc(1);
    }

    Ok(())
}
