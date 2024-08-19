use std::env;
use std::fs::File;
use std::io::Write;

use anyhow::Result;
use bitcoincore_rpc::{
    json::{GetChainTipsResultStatus, GetChainTipsResultTip},
    Auth, Client, RpcApi,
};
use chrono::{Duration, TimeZone, Utc};
use indicatif::ProgressBar;

fn main() -> Result<()> {
    let mut out: Vec<String> = vec![];
    out.push("Date,Total P2PK addresses,Total P2PK coins".to_owned());

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

    let mut p2pk_addresses: i32 = 0;
    let mut p2pk_coins: f64 = 0.0;

    let result = rpc.get_chain_tips()?;
    let tip_height = result
        .iter()
        .filter(|fork: &&GetChainTipsResultTip| fork.status == GetChainTipsResultStatus::Active)
        .collect::<Vec<_>>()
        .first()
        .unwrap()
        .height;

    let pb = ProgressBar::new(tip_height);
    pb.println(format!("Syncing from blocks 1 to {tip_height}"));

    for height in 1..tip_height {
        let hash = rpc.get_block_hash(height)?;
        let block = rpc.get_block(&hash)?;

        for (i, tx) in block.txdata.iter().enumerate() {
            for outpoint in &tx.output {
                if outpoint.script_pubkey.is_p2pk() {
                    p2pk_addresses += 1;
                    p2pk_coins += outpoint.value.to_btc();
                }
            }

            if i > 1 {
                for txin in &tx.input {
                    let txid = txin.previous_output.txid;
                    let transaction = rpc.get_raw_transaction(&txid, None)?;

                    if transaction.is_coinbase() {
                        continue;
                    }

                    for outpoint in transaction.output {
                        if outpoint.script_pubkey.is_p2pk() {
                            p2pk_addresses -= 1;
                            p2pk_coins -= outpoint.value.to_btc();
                        }
                    }
                }
            }
        }

        let datetime = Utc
            .timestamp_opt(block.header.time as i64, 0)
            .single()
            .expect("Invalid timestamp");

        let formatted_date = datetime.format("%m/%d/%Y %H:%M:%S").to_string();

        out.push(format!(
            "{},{},{}",
            formatted_date, p2pk_addresses, p2pk_coins
        ));

        pb.inc(1);

        let eta_duration = pb.eta();
        let eta_datetime =
            Utc::now() + Duration::from_std(eta_duration).unwrap_or_else(|_| Duration::seconds(0));
        let eta_hms = eta_datetime.format("%d:%H:%M:%S").to_string();

        pb.println(format!("Block: {height} - ETA: {eta_hms}"));
    }

    let content = out.join("\n");
    let mut file = File::create("out.csv")?;
    file.write_all(content.as_bytes())?;

    Ok(())
}
