use std::env;

use bitcoin::block::Header;
use reqwest::blocking::Client;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let onion = env::var("ONION")?;

    let hash = "000000000000000000067bea442af50a91377ac796e63b8d284354feff4042b3";
    let url = format!("https://{onion}.local/api/block/{hash}");

    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .build()?;

    let response = client.get(url).send()?.text()?;

    let json: Header = serde_json::from_str(&response)?;
    println!("{}", serde_json::to_string_pretty(&json)?);

    Ok(())
}
