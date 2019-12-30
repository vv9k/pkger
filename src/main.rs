use failure::Error;
use pkger::Pkger;

#[tokio::main]
async fn main() -> Result<(), Error> {
    pretty_env_logger::init();
    //let toml = toml::from_str::<Recipe>(&fs::read_to_string("/home/wojtek/dev/rust/pkger/tmp/recipe.toml")?)?;

    let pkgr = Pkger::new(
        "http://0.0.0.0:2376",
        "/home/wojtek/dev/rust/pkger/tmp/conf.toml",
    )?;

    pkgr.build_recipe("curl").await?;

    Ok(())
}
