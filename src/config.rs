use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub lists: Vec<ListPair>,
}

#[derive(Debug, Deserialize)]
pub struct ListPair {
    pub source: Url,
    pub target: Url,
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
        if self.lists.is_empty() {
            bail!("lists must contain at least one source/target pair");
        }
        for (index, pair) in self.lists.iter().enumerate() {
            validate_list_url(&pair.source, &format!("lists[{index}].source"))?;
            validate_list_url(&pair.target, &format!("lists[{index}].target"))?;
        }
        Ok(())
    }
}

impl ListPair {
    pub fn target_username(&self) -> &str {
        self.target
            .path_segments()
            .and_then(|mut parts| parts.next())
            .expect("validated target URL has a username")
    }
}

fn validate_list_url(url: &Url, field: &str) -> Result<()> {
    if url.scheme() != "https" || url.host_str() != Some("letterboxd.com") {
        bail!("{field} must be an https://letterboxd.com URL");
    }
    let parts: Vec<_> = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() != 3 || parts[1] != "list" {
        bail!("{field} must have the form https://letterboxd.com/user/list/slug/");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_url_pairs() {
        let config: Config = serde_yaml::from_str(
            r#"
lists:
  - source: https://letterboxd.com/alice/list/one/
    target: https://letterboxd.com/shadrunov/list/one/
  - source: https://letterboxd.com/bob/list/two/
    target: https://letterboxd.com/shadrunov/list/two/
"#,
        )
        .unwrap();
        config.validate().unwrap();
        assert_eq!(config.lists.len(), 2);
        assert_eq!(config.lists[0].target_username(), "shadrunov");
    }
}
