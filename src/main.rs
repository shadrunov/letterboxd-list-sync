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

    for pair in &config.lists {
        let source = client.fetch_public_list(&pair.source).await?;
        let target_url = if args.dry_run {
            pair.target_url(&config.username)
        } else {
            client
                .find_list_url(&config.username, &pair.target)
                .await?
                .unwrap_or_else(|| pair.target_url(&config.username))
        };
        let description = if source.description.is_empty() {
            format!("synced from {}", pair.source)
        } else {
            format!("{}\n\nsynced from {}", source.description, pair.source)
        };
        println!(
            "{} -> {} ({} films)",
            pair.source,
            target_url,
            source.films.len()
        );
        for (index, film) in source.films.iter().enumerate() {
            println!("{:>3}. {} [{}]", index + 1, film.name, film.slug);
        }

        if !args.dry_run {
            match client.inspect_target(&target_url).await? {
                Some(target) => {
                    client
                        .replace_list(&target, &pair.target, &description, &source.films)
                        .await?;
                    println!("Updated {target_url}");
                }
                None => {
                    let created = client
                        .create_list(&pair.target, &description, &source.films)
                        .await?;
                    println!("Created https://boxd.it/{}", created.lid);
                }
            }
        }
    }

    if args.dry_run {
        println!("Dry run complete; targets were not changed");
    }

    Ok(())
}
