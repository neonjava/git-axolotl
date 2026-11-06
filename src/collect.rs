//! Collects a GitHub account's public commit/contribution history and language hex colors.
//!
//! Uses direct HTTPS requests (`https://api.github.com/graphql` when `GITHUB_TOKEN` or `GH_TOKEN`
//! is set, or the public GitHub REST/contributions endpoints when no token is provided), with
//! `gh api graphql` as an automatic fallback if local CLI auth is preferred.

use std::collections::BTreeMap;
use std::env;
use std::io::ErrorKind;
use std::process::Command;
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde_json::{json, Value};

use crate::history::{
    rank_languages, summarize_calendar_window, AccountSnapshot, ContributionDay,
    ContributionWindow, YearTotal,
};

const ACCOUNT_QUERY: &str = r#"
query($login: String!) {
  user(login: $login) {
    databaseId
    repositories(first: 100, ownerAffiliations: OWNER, isFork: false, privacy: PUBLIC, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        primaryLanguage {
          name
          color
        }
      }
    }
    contributionsCollection {
      contributionYears
      contributionCalendar {
        weeks {
          contributionDays {
            date
            contributionCount
          }
        }
      }
    }
  }
}
"#;

/// Every request gets a deadline so a stalled endpoint can't hang a scheduled run.
fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build()
}

/// Collect an `AccountSnapshot` for `login` using the default `Annual` window.
pub fn collect_account_snapshot(login: &str) -> Result<AccountSnapshot, String> {
    collect_account_snapshot_with_window(login, ContributionWindow::Annual)
}

/// Collect an `AccountSnapshot` for `login` across `window` (`Annual` or `Rolling365`).
pub fn collect_account_snapshot_with_window(
    login: &str,
    window: ContributionWindow,
) -> Result<AccountSnapshot, String> {
    let mut failures: Vec<String> = Vec::new();

    // 1. If GITHUB_TOKEN or GH_TOKEN is available, query https://api.github.com/graphql directly via HTTPS (no `gh` CLI required).
    if let Ok(token) = env::var("GITHUB_TOKEN").or_else(|_| env::var("GH_TOKEN")) {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            let fetch = |query: &str| fetch_graphql_via_https(query, login, trimmed);
            match collect_via_graphql(login, window, &fetch) {
                Ok(snapshot) => return Ok(snapshot),
                Err(err) => failures.push(format!("GraphQL over HTTPS: {err}")),
            }
        }
    }

    // 2. Otherwise try `gh api graphql` if the user has the GitHub CLI installed and logged in locally.
    let fetch = |query: &str| fetch_graphql_via_gh_cli(query, login);
    match collect_via_graphql(login, window, &fetch) {
        Ok(snapshot) => return Ok(snapshot),
        Err(err) => failures.push(format!("gh CLI: {err}")),
    }

    // 3. Fallback: tokenless public HTTPS endpoints (`api.github.com/users/<login>/repos` + `github-contributions-api.jogruber.de`).
    fetch_public_rest_snapshot(login, window).map_err(|err| {
        failures.push(format!("public endpoints: {err}"));
        format!(
            "could not collect a snapshot for `{login}`: {}",
            failures.join("; ")
        )
    })
}

/// Fetch the last year's calendar, then the total for every year the account has been active.
fn collect_via_graphql(
    login: &str,
    window: ContributionWindow,
    fetch: &dyn Fn(&str) -> Result<Value, String>,
) -> Result<AccountSnapshot, String> {
    let root = fetch(ACCOUNT_QUERY).and_then(require_graphql_data)?;
    let mut snapshot = parse_graphql_snapshot(login, &root, window)?;

    let years = parse_contribution_years(&root);
    if !years.is_empty() {
        let totals = fetch(&build_years_query(&years)).and_then(require_graphql_data)?;
        let (yearly, private) = parse_year_totals(&totals, &years)?;
        snapshot.apply_history(yearly, private, window);
    }
    Ok(snapshot)
}

fn parse_contribution_years(root: &Value) -> Vec<i32> {
    root.pointer("/data/user/contributionsCollection/contributionYears")
        .and_then(Value::as_array)
        .map(|years| {
            years
                .iter()
                .filter_map(Value::as_i64)
                .map(|y| y as i32)
                .collect()
        })
        .unwrap_or_default()
}

/// One aliased `contributionsCollection` per year, because each is limited to a one-year span.
fn build_years_query(years: &[i32]) -> String {
    let fields: String = years
        .iter()
        .map(|y| {
            format!(
                "y{y}: contributionsCollection(from: \"{y}-01-01T00:00:00Z\", to: \"{y}-12-31T23:59:59Z\") {{ restrictedContributionsCount contributionCalendar {{ totalContributions }} }}\n"
            )
        })
        .collect();
    format!("query($login: String!) {{ user(login: $login) {{ {fields} }} }}")
}

fn parse_year_totals(root: &Value, years: &[i32]) -> Result<(Vec<YearTotal>, u64), String> {
    let mut yearly = Vec::with_capacity(years.len());
    let mut private = 0;
    for &year in years {
        let entry = root
            .pointer(&format!("/data/user/y{year}"))
            .ok_or_else(|| format!("GitHub returned no total for {year}"))?;
        let count = entry
            .pointer("/contributionCalendar/totalContributions")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("GitHub returned no total for {year}"))?;
        private += entry
            .get("restrictedContributionsCount")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        yearly.push(YearTotal { year, count });
    }
    Ok((yearly, private))
}

/// Reject GraphQL responses that report `errors`, so partial data never becomes an empty snapshot.
fn require_graphql_data(root: Value) -> Result<Value, String> {
    if let Some(errors) = root.get("errors").and_then(Value::as_array) {
        if !errors.is_empty() {
            let messages: Vec<&str> = errors
                .iter()
                .filter_map(|e| e.get("message").and_then(Value::as_str))
                .collect();
            return Err(format!("GraphQL errors: {}", messages.join(", ")));
        }
    }
    Ok(root)
}

fn fetch_graphql_via_https(query: &str, login: &str, token: &str) -> Result<Value, String> {
    let payload = json!({
        "query": query,
        "variables": { "login": login }
    });
    let resp = http_agent()
        .post("https://api.github.com/graphql")
        .set("Authorization", &format!("Bearer {token}"))
        .set("User-Agent", "git-axolotl")
        .set("Accept", "application/vnd.github+json")
        .send_json(payload)
        .map_err(|e| format!("GitHub GraphQL HTTPS request failed: {e}"))?;

    resp.into_json::<Value>()
        .map_err(|e| format!("Invalid JSON from GitHub GraphQL API: {e}"))
}

fn fetch_graphql_via_gh_cli(query: &str, login: &str) -> Result<Value, String> {
    let output = Command::new("gh")
        .args(["api", "graphql", "-f"])
        .arg(format!("query={query}"))
        .arg("-f")
        .arg(format!("login={login}"))
        .output()
        .map_err(|err| {
            if err.kind() == ErrorKind::NotFound {
                "GitHub CLI (`gh`) is not installed or not in PATH".to_owned()
            } else {
                format!("Failed to execute `gh`: {err}")
            }
        })?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("Invalid JSON from `gh api graphql`: {err}"))
}

fn parse_graphql_snapshot(
    login: &str,
    root: &Value,
    window: ContributionWindow,
) -> Result<AccountSnapshot, String> {
    let user = root
        .get("data")
        .and_then(|d| d.get("user"))
        .filter(|u| !u.is_null())
        .ok_or_else(|| format!("GitHub user `{login}` not found"))?;

    let account_id = user.get("databaseId").and_then(Value::as_u64).unwrap_or(1);

    let mut lang_counts: BTreeMap<String, (String, u32)> = BTreeMap::new();
    if let Some(nodes) = user
        .get("repositories")
        .and_then(|r| r.get("nodes"))
        .and_then(Value::as_array)
    {
        for node in nodes {
            if let Some(lang) = node.get("primaryLanguage").filter(|l| !l.is_null()) {
                let name = lang
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown")
                    .to_owned();
                let color = lang
                    .get("color")
                    .and_then(Value::as_str)
                    .unwrap_or("#3178c6")
                    .to_owned();
                let entry = lang_counts.entry(name).or_insert((color, 0));
                entry.1 += 1;
            }
        }
    }

    let weeks = user
        .get("contributionsCollection")
        .and_then(|c| c.get("contributionCalendar"))
        .and_then(|cal| cal.get("weeks"))
        .and_then(Value::as_array)
        .ok_or_else(|| format!("GitHub returned no contribution calendar for `{login}`"))?;

    let mut days = Vec::new();
    for week in weeks {
        if let Some(c_days) = week.get("contributionDays").and_then(Value::as_array) {
            for d in c_days {
                if let (Some(date_str), Some(count)) = (
                    d.get("date").and_then(Value::as_str),
                    d.get("contributionCount").and_then(Value::as_u64),
                ) {
                    if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                        days.push(ContributionDay {
                            date,
                            count: count as u32,
                        });
                    }
                }
            }
        }
    }
    if days.is_empty() {
        return Err(format!("contribution calendar for `{login}` was empty"));
    }

    let today: NaiveDate = DateTime::<Utc>::from(std::time::SystemTime::now()).date_naive();
    let (total_contributions, monthly_contributions, current_streak, longest_streak) =
        summarize_calendar_window(&days, today, window);

    Ok(AccountSnapshot {
        login: login.to_owned(),
        account_id,
        observed_on: today,
        total_contributions,
        monthly_contributions,
        current_streak,
        longest_streak,
        languages: rank_languages(lang_counts),
        lifetime_contributions: 0,
        lifetime_private_contributions: 0,
        yearly_contributions: Vec::new(),
    })
}

fn fetch_public_rest_snapshot(
    login: &str,
    window: ContributionWindow,
) -> Result<AccountSnapshot, String> {
    let user_url = format!("https://api.github.com/users/{login}");
    let user_json: Value = http_agent()
        .get(&user_url)
        .set("User-Agent", "git-axolotl")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Failed to fetch GitHub user `{login}`: {e}"))?
        .into_json()
        .map_err(|e| format!("Invalid user JSON: {e}"))?;

    let account_id = user_json.get("id").and_then(Value::as_u64).unwrap_or(1);

    let repos_url = format!("https://api.github.com/users/{login}/repos?per_page=100&sort=updated");
    let repos: Value = http_agent()
        .get(&repos_url)
        .set("User-Agent", "git-axolotl")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Failed to fetch repositories for `{login}`: {e}"))?
        .into_json()
        .map_err(|e| format!("Invalid repositories JSON: {e}"))?;
    let repos = repos
        .as_array()
        .ok_or_else(|| "Repositories response was not a list".to_owned())?;

    let mut lang_counts: BTreeMap<String, (String, u32)> = BTreeMap::new();
    for repo in repos {
        if repo.get("fork").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        if let Some(lang_name) = repo.get("language").and_then(Value::as_str) {
            let color = default_language_hex(lang_name).to_owned();
            let entry = lang_counts
                .entry(lang_name.to_owned())
                .or_insert((color, 0));
            entry.1 += 1;
        }
    }

    let contribs_url = format!("https://github-contributions-api.jogruber.de/v4/{login}");
    let contrib_json: Value = http_agent()
        .get(&contribs_url)
        .set("User-Agent", "git-axolotl")
        .call()
        .map_err(|e| format!("Failed to fetch contributions for `{login}`: {e}"))?
        .into_json()
        .map_err(|e| format!("Invalid contributions JSON: {e}"))?;
    let days = parse_public_contributions(&contrib_json)?;

    let today: NaiveDate = DateTime::<Utc>::from(std::time::SystemTime::now()).date_naive();
    let (total_contributions, monthly_contributions, current_streak, longest_streak) =
        summarize_calendar_window(&days, today, window);

    let mut snapshot = AccountSnapshot {
        login: login.to_owned(),
        account_id,
        observed_on: today,
        total_contributions,
        monthly_contributions,
        current_streak,
        longest_streak,
        languages: rank_languages(lang_counts),
        lifetime_contributions: 0,
        lifetime_private_contributions: 0,
        yearly_contributions: Vec::new(),
    };
    snapshot.apply_history(yearly_from_days(&days), 0, window);
    Ok(snapshot)
}

fn yearly_from_days(days: &[ContributionDay]) -> Vec<YearTotal> {
    let mut by_year: BTreeMap<i32, u64> = BTreeMap::new();
    for day in days {
        *by_year.entry(day.date.year()).or_insert(0) += u64::from(day.count);
    }
    by_year
        .into_iter()
        .map(|(year, count)| YearTotal { year, count })
        .collect()
}

fn parse_public_contributions(root: &Value) -> Result<Vec<ContributionDay>, String> {
    let items = root
        .get("contributions")
        .and_then(Value::as_array)
        .ok_or_else(|| "Contributions response had no `contributions` list".to_owned())?;

    let mut days = Vec::with_capacity(items.len());
    for item in items {
        if let (Some(date_str), Some(count)) = (
            item.get("date").and_then(Value::as_str),
            item.get("count").and_then(Value::as_u64),
        ) {
            if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                days.push(ContributionDay {
                    date,
                    count: count as u32,
                });
            }
        }
    }
    if days.is_empty() {
        return Err("Contributions response had no usable days".to_owned());
    }
    Ok(days)
}

fn default_language_hex(name: &str) -> &'static str {
    match name {
        "Rust" => "#dea584",
        "TypeScript" => "#3178c6",
        "JavaScript" => "#f1e05a",
        "Python" => "#3572A5",
        "Go" => "#00ADD8",
        "Java" => "#b07219",
        "C++" => "#f34b7d",
        "C" => "#555555",
        "C#" => "#178600",
        "Ruby" => "#701516",
        "PHP" => "#4F5D95",
        "Swift" => "#F05138",
        "Kotlin" => "#A97BFF",
        "Dart" => "#00B4AB",
        "HTML" => "#e34c26",
        "CSS" => "#563d7c",
        "Shell" => "#89e051",
        _ => "#3178c6",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphql_errors_are_rejected_even_with_partial_data() {
        let root = json!({
            "errors": [{ "message": "API rate limit exceeded" }],
            "data": { "user": { "contributionsCollection": null } }
        });
        let err = require_graphql_data(root).unwrap_err();
        assert!(err.contains("rate limit"));
    }

    #[test]
    fn missing_calendar_fails_instead_of_producing_zero_contributions() {
        let root = json!({
            "data": { "user": { "databaseId": 1, "contributionsCollection": null } }
        });
        let result = parse_graphql_snapshot("someone", &root, ContributionWindow::Annual);
        assert!(result.is_err());
    }

    #[test]
    fn year_totals_sum_every_year_and_count_private_work() {
        let root = json!({ "data": { "user": {
            "y2024": {
                "restrictedContributionsCount": 40,
                "contributionCalendar": { "totalContributions": 100 }
            },
            "y2025": {
                "restrictedContributionsCount": 2,
                "contributionCalendar": { "totalContributions": 50 }
            }
        }}});
        let (yearly, private) = parse_year_totals(&root, &[2025, 2024]).unwrap();
        assert_eq!(yearly.iter().map(|y| y.count).sum::<u64>(), 150);
        assert_eq!(private, 42);
    }

    #[test]
    fn a_missing_year_fails_instead_of_undercounting() {
        let root = json!({ "data": { "user": {} } });
        assert!(parse_year_totals(&root, &[2024]).is_err());
    }

    #[test]
    fn years_query_asks_for_each_year() {
        let query = build_years_query(&[2021, 2026]);
        assert!(query.contains("y2021: contributionsCollection(from: \"2021-01-01T00:00:00Z\""));
        assert!(query.contains("y2026:"));
    }

    #[test]
    fn public_days_group_by_year() {
        let root = json!({ "contributions": [
            { "date": "2026-01-02", "count": 3 },
            { "date": "2021-05-01", "count": 1 },
            { "date": "2026-03-02", "count": 4 }
        ]});
        let yearly = yearly_from_days(&parse_public_contributions(&root).unwrap());
        assert_eq!(
            yearly[0],
            YearTotal {
                year: 2021,
                count: 1
            }
        );
        assert_eq!(
            yearly[1],
            YearTotal {
                year: 2026,
                count: 7
            }
        );
    }

    #[test]
    fn empty_public_contributions_fail() {
        assert!(parse_public_contributions(&json!({ "contributions": [] })).is_err());
        assert!(parse_public_contributions(&json!({ "error": "nope" })).is_err());
    }

    #[test]
    fn public_contributions_parse() {
        let root = json!({ "contributions": [{ "date": "2026-01-02", "count": 3 }] });
        let days = parse_public_contributions(&root).unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].count, 3);
    }
}
