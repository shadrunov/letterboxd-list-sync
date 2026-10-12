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
        let films = client.fetch_public_list(&pair.source).await?;
        let target_url = pair.target_url(&config.username);
        println!("{} -> {} ({} films)", pair.source, target_url, films.len());
        for (index, film) in films.iter().enumerate() {
            println!("{:>3}. {} [{}]", index + 1, film.name, film.slug);
        }

        if !args.dry_run {
            match client.inspect_target(&target_url).await? {
                Some(target) => {
                    client.replace_list(&target, &pair.target, &films).await?;
                    println!("Updated {target_url}");
                }
                None => {
                    client.create_list(&pair.target, &films).await?;
                    println!("Created {target_url}");
                }
            }
        }
    }

    if args.dry_run {
        println!("Dry run complete; targets were not changed");
    }

    Ok(())
}
