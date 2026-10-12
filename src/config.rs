use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub source: Source,
    pub target: Target,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Source {
    pub url: Url,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    pub username: String,
    pub url: Option<Url>,
    pub name: String,
    pub slug: String,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: Self = serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        validate_letterboxd_url(&self.source.url, "source.url")?;
        if let Some(url) = &self.target.url {
            validate_letterboxd_url(url, "target.url")?;
        }
        if self.target.username.trim().is_empty() {
            bail!("target.username must not be empty");
        }
        if self.target.name.trim().is_empty() {
            bail!("target.name must not be empty");
        }
        if self.target.slug.trim().is_empty()
            || !self
                .target
                .slug
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            bail!("target.slug must contain only letters, numbers, and hyphens");
        }
        Ok(())
    }

    pub fn target_url(&self) -> Url {
        self.target.url.clone().unwrap_or_else(|| {
            Url::parse(&format!(
                "https://letterboxd.com/{}/list/{}/",
                self.target.username, self.target.slug
            ))
            .expect("validated username and slug form a URL")
        })
    }
}

fn validate_letterboxd_url(url: &Url, field: &str) -> Result<()> {
    if url.scheme() != "https" || url.host_str() != Some("letterboxd.com") {
        bail!("{field} must be an https://letterboxd.com URL");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_target_url() {
        let config: Config = serde_yaml::from_str(
            r#"
source:
  url: https://letterboxd.com/a/list/b/
target:
  username: alice
  name: Test
  slug: test
"#,
        )
        .unwrap();
        config.validate().unwrap();
        assert_eq!(
            config.target_url().as_str(),
            "https://letterboxd.com/alice/list/test/"
        );
    }
}
