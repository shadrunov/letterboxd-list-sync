mod config;
mod letterboxd;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use config::Config;
use letterboxd::LetterboxdClient;

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    #[arg(long, default_value = "config.yaml")]
    config: PathBuf,

    /// Read and compare without changing the target list.
    #[arg(long)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    let client = LetterboxdClient::from_env()?;

    let films = client.fetch_public_list(&config.source.url).await?;
    println!("Source contains {} films", films.len());
    for (index, film) in films.iter().enumerate() {
        println!("{:>3}. {} [{}]", index + 1, film.name, film.slug);
    }

    if args.dry_run {
        println!("Dry run complete; target was not changed");
        return Ok(());
    }

    client.verify_session(&config.target.username).await?;
    let target_url = config.target_url();
    match client.inspect_target(&target_url).await? {
        Some(target) => {
            client
                .replace_list(&target, &config.target.name, true, false, &films)
                .await?;
            println!("Updated {target_url}");
        }
        None => {
            client
                .create_list(&config.target.name, true, false, &films)
                .await?;
            println!("Created {target_url}");
        }
    }

    Ok(())
}
