//! `ly` — the short launcher name for the sbtui client, mirroring the
//! server-side sbctl `ly` shortcut. It runs the same TUI as `sbtui`.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    sbtui::run().await
}
