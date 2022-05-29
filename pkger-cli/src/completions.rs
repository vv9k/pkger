use crate::opts::{CompletionsOpts, Opts, APP_NAME};
use crate::Error;

use clap::{IntoApp, Parser};
use std::io;
use std::str::FromStr;

#[derive(Debug, Parser)]
#[allow(clippy::enum_variant_names)]
pub enum Shell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}

impl FromStr for Shell {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match &s.to_lowercase()[..] {
            "bash" => Ok(Shell::Bash),
            "elvish" => Ok(Shell::Elvish),
            "fish" => Ok(Shell::Fish),
            "powershell" => Ok(Shell::PowerShell),
            "zsh" => Ok(Shell::Zsh),
            _ => Err(Error::msg(format!("invalid shell `{}`", s))),
        }
    }
}

pub fn print(opts: &CompletionsOpts) {
    use clap_complete::{
        generate,
        shells::{Bash, Elvish, Fish, PowerShell, Zsh},
    };

    let mut app = Opts::command();

    match opts.shell {
        Shell::Bash => generate(Bash, &mut app, APP_NAME, &mut io::stdout()),
        Shell::Elvish => generate(Elvish, &mut app, APP_NAME, &mut io::stdout()),
        Shell::Fish => generate(Fish, &mut app, APP_NAME, &mut io::stdout()),
        Shell::PowerShell => generate(PowerShell, &mut app, APP_NAME, &mut io::stdout()),
        Shell::Zsh => generate(Zsh, &mut app, APP_NAME, &mut io::stdout()),
    }
}
