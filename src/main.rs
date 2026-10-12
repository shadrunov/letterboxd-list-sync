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
        println!(
            "{} -> {} ({} films)",
            pair.source,
            pair.target,
            source.films.len()
        );
        for (index, film) in source.films.iter().enumerate() {
            println!("{:>3}. {} [{}]", index + 1, film.name, film.slug);
        }

        if !args.dry_run {
            client.verify_session(pair.target_username()).await?;
            match client.inspect_target(&pair.target).await? {
                Some(target) => {
                    client
                        .replace_list(&target, &source.name, &source.films)
                        .await?;
                    println!("Updated {}", pair.target);
                }
                None => {
                    client.create_list(&source.name, &source.films).await?;
                    println!("Created {}", pair.target);
                }
            }
        }
    }

    if args.dry_run {
        println!("Dry run complete; targets were not changed");
    }

    Ok(())
}
