use pkger::{Pkger, Result};

#[tokio::main]
async fn main() -> Result<()> {
    Pkger::main().await
}
