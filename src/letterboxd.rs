use std::{collections::HashSet, env, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use regex::Regex;
use reqwest::{Client, StatusCode, header};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

const BASE: &str = "https://letterboxd.com";
const API_BASE: &str = "https://api.letterboxd.com/api/v0";
const USER_AGENT: &str =
    "letterboxd-list-sync/0.1 (+https://github.com/shadrunov/letterboxd-list-sync)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Film {
    pub lid: String,
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SourceList {
    pub name: String,
    pub films: Vec<Film>,
}

#[derive(Debug, Clone)]
pub struct TargetList {
    pub lid: String,
    pub version: u64,
}

#[derive(Clone)]
pub struct LetterboxdClient {
    http: Client,
    cookie_header: Option<String>,
    csrf: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateList<'a> {
    published: bool,
    name: &'a str,
    share_policy: &'a str,
    ranked: bool,
    description: &'a str,
    tags: Vec<String>,
    entries: Vec<ListUpdateEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateList<'a> {
    version: u64,
    published: bool,
    name: &'a str,
    share_policy: &'a str,
    ranked: bool,
    description: &'a str,
    tags: Vec<String>,
    entries: Vec<ListUpdateEntry>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "action")]
enum ListUpdateEntry {
    #[serde(rename = "CLEAR")]
    Clear,
    #[serde(rename = "ADD")]
    Add { listable: String },
}

#[derive(Debug, Deserialize)]
struct SaveResponse {
    #[serde(default)]
    messages: Vec<ApiMessage>,
    data: Option<SavedList>,
}

#[derive(Debug, Deserialize)]
struct ApiMessage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct SavedList {
    id: String,
    name: String,
    version: Option<u64>,
}

impl LetterboxdClient {
    pub fn from_env() -> Result<Self> {
        let user_cookie = env::var("LETTERBOXD_USER_COOKIE").ok();
        let csrf = env::var("LETTERBOXD_CSRF_COOKIE").ok();
        let cookie_header = match (&user_cookie, &csrf) {
            (Some(user), Some(csrf)) => Some(format!(
                "letterboxd.user={user}; com.xk72.webparts.csrf={csrf}"
            )),
            (None, None) => None,
            _ => bail!("LETTERBOXD_USER_COOKIE and LETTERBOXD_CSRF_COOKIE must be set together"),
        };

        let http = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            http,
            cookie_header,
            csrf,
        })
    }

    pub async fn fetch_public_list(&self, url: &Url) -> Result<SourceList> {
        let mut films = Vec::new();
        let mut seen = HashSet::new();
        let mut page = 1_u32;
        let mut name = None;

        loop {
            let page_url = if page == 1 {
                url.clone()
            } else {
                url.join(&format!("page/{page}/"))?
            };
            let response = self.http.get(page_url.clone()).send().await?;
            if page > 1 && response.status() == StatusCode::NOT_FOUND {
                break;
            }
            let response = response
                .error_for_status()
                .with_context(|| format!("failed to fetch {page_url}"))?;
            let html = response.text().await?;
            if page == 1 {
                name = Some(parse_list_name(&html)?);
            }
            let parsed = parse_films(&html)?;
            if parsed.is_empty() {
                if page == 1 {
                    bail!("no films found at {url}; check that the list is public");
                }
                break;
            }

            let before = films.len();
            for film in parsed {
                if seen.insert(film.lid.clone()) {
                    films.push(film);
                }
            }
            if films.len() == before || !has_next_page(&html) {
                break;
            }
            page += 1;
        }
        Ok(SourceList {
            name: name.context("source list name was not found")?,
            films,
        })
    }

    pub async fn verify_session(&self, expected_username: &str) -> Result<()> {
        let cookie = self
            .cookie_header
            .as_deref()
            .context("Letterboxd session cookies are not configured")?;
        let response = self
            .http
            .get(format!("{BASE}/"))
            .header(header::COOKIE, cookie)
            .send()
            .await?
            .error_for_status()?;
        let html = response.text().await?;
        let username = parse_logged_in_username(&html)
            .context("Letterboxd rejected the session cookies; refresh the GitHub secrets")?;
        if !username.eq_ignore_ascii_case(expected_username) {
            bail!(
                "configured username is {expected_username}, but the session belongs to {username}"
            );
        }
        Ok(())
    }

    pub async fn inspect_target(&self, target_url: &Url) -> Result<Option<TargetList>> {
        let cookie = self
            .cookie_header
            .as_deref()
            .context("Letterboxd session cookies are not configured")?;
        let edit_url = target_url.join("edit/")?;
        let response = self
            .http
            .get(edit_url.clone())
            .header(header::COOKIE, cookie)
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response
            .error_for_status()
            .with_context(|| format!("failed to inspect {edit_url}"))?;
        let html = response.text().await?;
        parse_target_list(&html).map(Some)
    }

    pub async fn create_list(&self, name: &str, films: &[Film]) -> Result<TargetList> {
        let payload = CreateList {
            published: true,
            name,
            share_policy: "You",
            ranked: false,
            description: "",
            tags: vec![],
            entries: films
                .iter()
                .map(|film| ListUpdateEntry::Add {
                    listable: film.lid.clone(),
                })
                .collect(),
        };
        let saved: SavedList = self.send_save("POST", "/lists", &payload).await?;
        Ok(TargetList {
            lid: saved.id,
            version: saved.version.unwrap_or(1),
        })
    }

    pub async fn replace_list(
        &self,
        target: &TargetList,
        name: &str,
        films: &[Film],
    ) -> Result<TargetList> {
        let mut entries = Vec::with_capacity(films.len() + 1);
        entries.push(ListUpdateEntry::Clear);
        entries.extend(films.iter().map(|film| ListUpdateEntry::Add {
            listable: film.lid.clone(),
        }));
        let payload = UpdateList {
            version: target.version,
            published: true,
            name,
            share_policy: "You",
            ranked: false,
            description: "",
            tags: vec![],
            entries,
        };
        let saved: SavedList = self
            .send_save("PATCH", &format!("/list/{}", target.lid), &payload)
            .await?;
        Ok(TargetList {
            lid: saved.id,
            version: saved.version.unwrap_or(target.version + 1),
        })
    }

    async fn send_save<T: Serialize + ?Sized>(
        &self,
        method: &str,
        path: &str,
        payload: &T,
    ) -> Result<SavedList> {
        let method = reqwest::Method::from_bytes(method.as_bytes())?;
        let cookie = self
            .cookie_header
            .as_deref()
            .context("Letterboxd session cookies are not configured")?;
        let csrf = self
            .csrf
            .as_deref()
            .context("Letterboxd CSRF cookie is not configured")?;
        let request = self
            .http
            .request(method, format!("{API_BASE}{path}"))
            .header(header::ORIGIN, BASE)
            .header(header::REFERER, format!("{BASE}/"))
            .header(header::ACCEPT, "application/json")
            .header(header::COOKIE, cookie)
            .header("x-csrf-token", csrf)
            .json(payload);

        let response = request.send().await?;
        let status = response.status();
        let body = response.text().await?;
        if status == StatusCode::UNAUTHORIZED {
            bail!("Letterboxd authentication expired or cannot authorize API writes (HTTP 401)");
        }
        if !status.is_success() {
            bail!(
                "Letterboxd save failed with HTTP {status}: {}",
                compact(&body)
            );
        }
        let parsed: SaveResponse = serde_json::from_str(&body)
            .with_context(|| format!("invalid Letterboxd save response: {}", compact(&body)))?;
        let errors: Vec<_> = parsed
            .messages
            .iter()
            .filter(|message| message.kind == "Error")
            .collect();
        if !errors.is_empty() {
            let text = errors
                .iter()
                .map(|error| format!("{}: {}", error.code, error.message))
                .collect::<Vec<_>>()
                .join("; ");
            bail!("Letterboxd rejected the list: {text}");
        }
        let saved = parsed
            .data
            .context("Letterboxd returned success without list data")?;
        eprintln!("Saved '{}' (LID {})", saved.name, saved.id);
        Ok(saved)
    }
}

fn compact(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect()
}

fn parse_films(html: &str) -> Result<Vec<Film>> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("[data-postered-identifier][data-item-slug]")
        .map_err(|error| anyhow!("invalid selector: {error}"))?;
    let mut films = Vec::new();
    let mut seen = HashSet::new();

    for element in document.select(&selector) {
        let Some(identifier) = element.value().attr("data-postered-identifier") else {
            continue;
        };
        let parsed: Value = serde_json::from_str(identifier)
            .with_context(|| format!("invalid data-postered-identifier: {identifier}"))?;
        if parsed.get("type").and_then(Value::as_str) != Some("film") {
            continue;
        }
        let Some(lid) = parsed.get("lid").and_then(Value::as_str) else {
            continue;
        };
        if !seen.insert(lid.to_owned()) {
            continue;
        }
        films.push(Film {
            lid: lid.to_owned(),
            slug: element
                .value()
                .attr("data-item-slug")
                .unwrap_or_default()
                .to_owned(),
            name: element
                .value()
                .attr("data-item-name")
                .unwrap_or_default()
                .to_owned(),
        });
    }
    Ok(films)
}

fn parse_list_name(html: &str) -> Result<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(r#"meta[property="og:title"]"#)
        .map_err(|error| anyhow!("invalid selector: {error}"))?;
    document
        .select(&selector)
        .next()
        .and_then(|element| element.value().attr("content"))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .context("source list has no Open Graph title")
}

fn has_next_page(html: &str) -> bool {
    let document = Html::parse_document(html);
    let selector = Selector::parse("a.next").expect("static selector is valid");
    document.select(&selector).next().is_some()
}

fn parse_logged_in_username(html: &str) -> Option<String> {
    let pattern = Regex::new(r#"username:\s*\"([^\"]+)\"\s*,\s*loggedIn:\s*true"#).ok()?;
    pattern
        .captures(html)
        .and_then(|capture| capture.get(1))
        .map(|value| value.as_str().to_owned())
}

fn parse_target_list(html: &str) -> Result<TargetList> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("[data-component-class=ListEditor]")
        .map_err(|error| anyhow!("invalid selector: {error}"))?;
    let element = document
        .select(&selector)
        .next()
        .context("target list editor was not found; check ownership and session cookies")?;
    let lid = element
        .value()
        .attr("data-list-lid")
        .context("target list has no Letterboxd ID")?
        .to_owned();
    let version = element
        .value()
        .attr("data-list-version")
        .context("target list has no version")?
        .parse()
        .context("target list version is not a number")?;
    Ok(TargetList { lid, version })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_films_in_order_and_deduplicates() {
        let html = r#"
<div data-postered-identifier='{&quot;lid&quot;:&quot;2b34&quot;,&quot;type&quot;:&quot;film&quot;}' data-item-slug="dracula" data-item-name="Dracula (1931)"></div>
<div data-postered-identifier='{&quot;lid&quot;:&quot;1Azs&quot;,&quot;type&quot;:&quot;film&quot;}' data-item-slug="draculas-daughter" data-item-name="Dracula's Daughter (1936)"></div>
<div data-postered-identifier='{&quot;lid&quot;:&quot;2b34&quot;,&quot;type&quot;:&quot;film&quot;}' data-item-slug="dracula" data-item-name="Dracula (1931)"></div>
"#;
        let films = parse_films(html).unwrap();
        assert_eq!(films.len(), 2);
        assert_eq!(films[0].lid, "2b34");
        assert_eq!(films[1].slug, "draculas-daughter");
    }

    #[test]
    fn parses_list_name() {
        let html = r#"<meta property="og:title" content="a&amp;a: vampire season">"#;
        assert_eq!(parse_list_name(html).unwrap(), "a&a: vampire season");
    }

    #[test]
    fn parses_logged_in_user() {
        let html = r#"person = { username: "Alice", loggedIn: true, role: "member" }"#;
        assert_eq!(parse_logged_in_username(html).as_deref(), Some("Alice"));
    }

    #[test]
    fn parses_target_metadata() {
        let html = r#"<div data-component-class="ListEditor" data-list-lid="abc123" data-list-version="7"></div>"#;
        let list = parse_target_list(html).unwrap();
        assert_eq!(list.lid, "abc123");
        assert_eq!(list.version, 7);
    }
}
