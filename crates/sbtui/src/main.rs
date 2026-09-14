//! sbtui binary entry point; the TUI itself lives in the library so the `ly`
//! binary can share exactly the same client.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    sbtui::run().await
}
