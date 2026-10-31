//! checkdom-mcp: an unofficial MCP server for the CheckDom resident dashboard.

#![forbid(unsafe_code)]

mod client;
mod config;
mod error;
mod mcp;
mod tools;

use anyhow::{Context, Result};
use std::process::ExitCode;

const USAGE: &str = "\
Unofficial MCP server for the CheckDom resident dashboard (checkdom.in.ua)

Usage: checkdom-mcp <COMMAND>

Commands:
  login                  Log in interactively and store a session token locally
  logout                 Remove the stored session token
  status                 Show whether a valid session is stored
  serve [--allow-writes] Run the MCP server over stdio; --allow-writes enables the
                         tools that modify data (create / comment on support requests)

Options:
  -h, --help     Print help
  -V, --version  Print version";

#[derive(Debug, PartialEq)]
enum Command {
    Login,
    Logout,
    Status,
    Serve { allow_writes: bool },
    Help,
    Version,
}

/// Parse the arguments after the program name.
fn parse_args(args: &[String]) -> std::result::Result<Command, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["login"] => Ok(Command::Login),
        ["logout"] => Ok(Command::Logout),
        ["status"] => Ok(Command::Status),
        ["serve"] => Ok(Command::Serve {
            allow_writes: false,
        }),
        ["serve", "--allow-writes"] => Ok(Command::Serve { allow_writes: true }),
        [] | ["-h" | "--help" | "help"] => Ok(Command::Help),
        ["-V" | "--version"] => Ok(Command::Version),
        [cmd, ..] => Err(format!("unexpected arguments starting at '{cmd}'")),
    }
}

// One request at a time on stdio: a single-threaded runtime is enough.
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = match command {
        Command::Help => {
            println!("{USAGE}");
            Ok(())
        }
        Command::Version => {
            println!("checkdom-mcp {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Login => cmd_login().await,
        Command::Logout => cmd_logout(),
        Command::Status => cmd_status().await,
        // Logs go to stderr; stdout is reserved for the MCP JSON stream.
        Command::Serve { allow_writes } => mcp::serve(allow_writes).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn cmd_login() -> Result<()> {
    // Read the prompts off the real terminal, off the async runtime.
    let (username, password) = tokio::task::spawn_blocking(|| -> Result<(String, String)> {
        use std::io::Write;
        print!("CIF or email: ");
        std::io::stdout().flush()?;
        let mut username = String::new();
        std::io::stdin()
            .read_line(&mut username)
            .context("reading username")?;
        let password = rpassword::prompt_password("Password: ").context("reading password")?;
        Ok((username.trim().to_string(), password))
    })
    .await
    .context("prompt task failed")??;

    if username.is_empty() {
        anyhow::bail!("no username entered");
    }

    let token = client::CheckDomClient::new()?
        .login(&username, &password)
        .await?;
    drop(password);
    // A failed login can still set a guest `session` cookie; only keep a token
    // that actually authenticates.
    if !client::CheckDomClient::with_token(token.clone())?
        .check_session()
        .await?
    {
        anyhow::bail!(
            "login failed: CheckDom did not accept the new session; check your CIF/email and password"
        );
    }
    config::save_token(&token)?;
    println!(
        "Logged in. Session token stored at {}",
        config::token_path()?.display()
    );
    Ok(())
}

fn cmd_logout() -> Result<()> {
    if config::delete_token()? {
        println!("Logged out; stored token removed.");
    } else {
        println!("No stored token.");
    }
    Ok(())
}

async fn cmd_status() -> Result<()> {
    match config::load_token()? {
        None => println!("Not logged in. Run `checkdom-mcp login`."),
        Some(token) => match client::CheckDomClient::with_token(token)?
            .check_session()
            .await
        {
            Ok(true) => println!("Session is valid."),
            Ok(false) => {
                println!("A token is stored but the session is expired. Run `checkdom-mcp login`.")
            }
            Err(e) => println!("A token is stored but the check failed: {e}"),
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> std::result::Result<Command, String> {
        parse_args(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_every_command() {
        assert_eq!(parse(&["login"]), Ok(Command::Login));
        assert_eq!(parse(&["logout"]), Ok(Command::Logout));
        assert_eq!(parse(&["status"]), Ok(Command::Status));
        assert_eq!(
            parse(&["serve"]),
            Ok(Command::Serve {
                allow_writes: false
            })
        );
        assert_eq!(
            parse(&["serve", "--allow-writes"]),
            Ok(Command::Serve { allow_writes: true })
        );
        assert_eq!(parse(&[]), Ok(Command::Help));
        assert_eq!(parse(&["--help"]), Ok(Command::Help));
        assert_eq!(parse(&["-V"]), Ok(Command::Version));
    }

    #[test]
    fn rejects_unknown_or_misplaced_arguments() {
        for bad in [
            &["serve", "--allow-write"][..],
            &["--allow-writes", "serve"],
            &["login", "--allow-writes"],
            &["serve", "--allow-writes", "extra"],
            &["pay"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
