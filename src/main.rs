use pkger::Pkger;
use failure::Error;
use wharf::{opts::ContainerBuilderOpts, Docker};


#[tokio::main]
async fn main() -> Result<(), Error> {
    pretty_env_logger::init();
    //let toml = toml::from_str::<Recipe>(&fs::read_to_string("/home/wojtek/dev/rust/pkger/tmp/recipe.toml")?)?;

    let pkgr = Pkger::new("http://0.0.0.0:2376", "/home/wojtek/dev/rust/pkger/tmp/conf.toml")?;
    println!("{:?}", pkgr.config);


    

    Ok(())
}
