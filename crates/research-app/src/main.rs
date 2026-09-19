use anyhow::Result;
use clap::{Parser, Subcommand};
use research_core::{Config, data_dir};
use research_server::client;
use serde_json::json;

#[derive(Parser)]
#[command(version, about = "ChatGPT browser research service for OpenCode")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Internal background service entrypoint.
    #[command(hide = true)]
    Serve,
    /// Ensure the service is ready and print its descriptor for the plugin.
    #[command(hide = true)]
    Connect,
    /// Open the persistent research Chrome profile for manual ChatGPT sign-in.
    Login,
    /// Display shared paths, configuration, and Chrome discovery.
    Doctor,
    /// Print status without starting the daemon.
    Status,
    /// Stop the daemon; research Chrome remains open.
    Shutdown,
    /// Write the default shared configuration if it does not exist.
    Configure,
    /// Exercise CDP launch/reconnect on a blank tab, without submitting a prompt.
    BrowserCheck,
    /// Inspect readiness and available controls on research-profile ChatGPT tabs (no transcript).
    BrowserInspect,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = data_dir()?;
    match cli.command {
        Command::Serve => research_server::serve(dir).await?,
        Command::Connect => {
            let service = client::ensure_running(dir, &std::env::current_exe()?).await?;
            println!("{}", serde_json::to_string(&service)?);
        }
        Command::Status => {
            let service = client::discover(&dir).await?;
            println!(
                "{}",
                client::rpc(&service, json!({"op":"health"}), 5).await?
            );
        }
        Command::Shutdown => {
            let service = client::discover(&dir).await?;
            println!(
                "{}",
                client::rpc(&service, json!({"op":"shutdown"}), 5).await?
            );
        }
        Command::Doctor => {
            let config = Config::load(&dir)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"data_directory":dir,
                "config":config,"chrome":research_browser::chrome_path(&config).ok(),
                "service_running":client::discover(&dir).await.is_ok()}))?
            );
        }
        Command::Configure => {
            research_server::secure_directory(&dir)?;
            let path = dir.join("config.json");
            if !path.exists() {
                std::fs::write(&path, serde_json::to_vec_pretty(&Config::default())?)?;
            }
            Config::load(&dir)?;
            println!("{}", path.display());
        }
        Command::Login | Command::BrowserCheck | Command::BrowserInspect => {
            research_server::secure_directory(&dir)?;
            let config = Config::load(&dir)?;
            let chrome = research_browser::Chrome::ensure(&dir, &config).await?;
            if matches!(cli.command, Command::Login) {
                let page = chrome.open_interactive("https://chatgpt.com/").await?;
                println!(
                    "Sign in to ChatGPT in the research Chrome window. Tab: {}",
                    page.id
                );
            } else if matches!(cli.command, Command::BrowserInspect) {
                for target in chrome.targets().await? {
                    if target["url"]
                        .as_str()
                        .is_some_and(research_browser::valid_chat_url)
                    {
                        let mut page = chrome.reconnect(target["id"].as_str(), None).await?;
                        let snapshot = research_chatgpt::inspect(&mut page).await?;
                        println!(
                            "{}",
                            json!({"target":page.id,"url":snapshot["url"],"composer":snapshot["composer"],
                            "login_required":snapshot["login_required"],"controls":snapshot["controls"]})
                        );
                    }
                }
            } else {
                let mut page = chrome.open("about:blank").await?;
                let result = page.eval("({userAgent:navigator.userAgent,url:location.href,webdriver:navigator.webdriver})").await?;
                anyhow::ensure!(
                    result["webdriver"] == false,
                    "Chrome was launched with test-automation flags"
                );
                let window_state = chrome.window_state(&page.id).await?;
                anyhow::ensure!(
                    window_state == "minimized",
                    "Research Chrome window is not minimized"
                );
                chrome.close(&page.id).await?;
                // Second ensure verifies ownership and reconnection through the saved endpoint.
                let reconnected = research_browser::Chrome::ensure(&dir, &config).await?;
                anyhow::ensure!(
                    chrome.port == reconnected.port,
                    "Reconnect launched a different browser"
                );
                println!(
                    "{}",
                    json!({"connected":true,"reconnected":true,"window_state":window_state,"browser":result})
                );
            }
        }
    }
    Ok(())
}
