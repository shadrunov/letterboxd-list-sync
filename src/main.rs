mod config;
mod letterboxd;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use config::Config;
use letterboxd::LetterboxdClient;
use url::Url;

const REPOSITORY_URL: &str = "https://github.com/shadrunov/letterboxd-list-syncer";

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
        let description =
            sync_description(&source.description, pair.source_username(), &pair.source);
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

fn sync_description(source_description: &str, source_username: &str, source_url: &Url) -> String {
    let credit = format!(
        "synced from [{source_username}'s list]({source_url}) by [letterboxd-list-syncer]({REPOSITORY_URL})"
    );
    if source_description.is_empty() {
        credit
    } else {
        format!("{source_description}\n\n{credit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_linked_sync_credit() {
        let source =
            Url::parse("https://letterboxd.com/cokewithglitter/list/aa-vampire-season/").unwrap();
        assert_eq!(
            sync_description("Original description", "cokewithglitter", &source),
            "Original description\n\nsynced from [cokewithglitter's list](https://letterboxd.com/cokewithglitter/list/aa-vampire-season/) by [letterboxd-list-syncer](https://github.com/shadrunov/letterboxd-list-syncer)"
        );
    }
}
