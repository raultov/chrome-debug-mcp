use cdp_lite::client::CdpClient;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Testing CDP connection to 127.0.0.1:9222...");
    let client = CdpClient::new("127.0.0.1:9222", Duration::from_secs(5)).await?;
    println!("Connected successfully!");

    let result = client
        .send_raw_command("Browser.getVersion", cdp_lite::protocol::NoParams)
        .await?;
    println!("Browser version: {:?}", result);

    Ok(())
}
