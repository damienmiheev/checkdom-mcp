//! checkdom-mcp: an unofficial MCP server for the CheckDom resident dashboard.

#![forbid(unsafe_code)]

mod client;
mod config;
mod error;
mod mcp;
mod tools;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "checkdom-mcp", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Log in interactively and store a session token locally.
    Login,
    /// Remove the stored session token.
    Logout,
    /// Show whether a valid session is stored.
    Status,
    /// Run the MCP server over stdio.
    Serve {
        /// Enable tools that modify data (create / comment on support requests).
        #[arg(long)]
        allow_writes: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Logs to stderr; stdout is reserved for the MCP JSON stream.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    match Cli::parse().command {
        Command::Login => cmd_login().await,
        Command::Logout => cmd_logout(),
        Command::Status => cmd_status().await,
        Command::Serve { allow_writes } => mcp::serve(allow_writes).await,
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
        config::credentials_path()?.display()
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
