use pkger::{Error, Pkger};

#[tokio::main]
async fn main() -> Result<(), Error> {
    Pkger::main().await
}
