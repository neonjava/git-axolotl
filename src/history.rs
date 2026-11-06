//! Account snapshot and language share models.

use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Window mode used to sum contributions for Axolotl stage progression.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
pub enum ContributionWindow {
    /// Current calendar year (January 1 through `observed_on`), resetting every January 1st.
    #[default]
    Annual,
    /// Trailing 365-day rolling window ending on `observed_on`.
    Rolling365,
    /// Every year since the account's first contribution. Never resets.
    Lifetime,
}

/// Contributions made in one calendar year.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct YearTotal {
    pub year: i32,
    pub count: u64,
}

/// One language observed in a user's public repositories along with its GitHub hex color.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct LanguageShare {
    /// Language name (e.g., `"Rust"`, `"TypeScript"`).
    pub name: String,
    /// GitHub hex color string (e.g., `"#dea584"` or `"#3178c6"`).
    pub hex_color: String,
    /// Relative size of this language: bytes of code, or a repository count on the public fallback.
    #[serde(alias = "repo_count")]
    pub weight: u64,
}

/// One day of GitHub contribution activity.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct ContributionDay {
    pub date: NaiveDate,
    pub count: u32,
}

/// Complete snapshot of a GitHub user's public activity used to render the Axolotl.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct AccountSnapshot {
    pub login: String,
    pub account_id: u64,
    pub observed_on: NaiveDate,
    /// Total contributions across the observed window (drives growth from Hatchling to Elder).
    pub total_contributions: u64,
    /// Contributions made in the current calendar month.
    pub monthly_contributions: u64,
    /// Current daily contribution streak.
    pub current_streak: u32,
    /// Longest daily contribution streak.
    pub longest_streak: u32,
    /// Top languages ranked by repository share.
    pub languages: Vec<LanguageShare>,
    /// Contributions across every year on record.
    #[serde(default)]
    pub lifetime_contributions: u64,
    /// How many of the lifetime contributions are private, when GitHub reports them.
    #[serde(default)]
    pub lifetime_private_contributions: u64,
    /// Per-year history, oldest first.
    #[serde(default)]
    pub yearly_contributions: Vec<YearTotal>,
}

impl AccountSnapshot {
    /// Build a deterministic seed from the account ID and login.
    pub fn identity_seed(&self) -> u64 {
        let mut h = self.account_id ^ 0x9e37_79b9_7f4a_7c15;
        for b in self.login.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        }
        h
    }

    /// The first year with any contributions.
    pub fn first_active_year(&self) -> Option<i32> {
        self.yearly_contributions
            .iter()
            .filter(|y| y.count > 0)
            .map(|y| y.year)
            .min()
    }

    /// Record the all-time history, and make it the growth total for the `Lifetime` window.
    pub fn apply_history(
        &mut self,
        mut yearly: Vec<YearTotal>,
        private: u64,
        window: ContributionWindow,
    ) {
        yearly.sort_by_key(|y| y.year);
        self.lifetime_contributions = yearly.iter().map(|y| y.count).sum();
        self.lifetime_private_contributions = private;
        self.yearly_contributions = yearly;
        if window == ContributionWindow::Lifetime {
            self.total_contributions = self.lifetime_contributions;
        }
    }
}

/// Summarize daily contributions in the current calendar year into total, current month, current streak, and longest streak.
pub fn summarize_calendar(
    days: &[ContributionDay],
    observed_on: NaiveDate,
) -> (u64, u64, u32, u32) {
    summarize_calendar_window(days, observed_on, ContributionWindow::Annual)
}

/// Summarize daily contributions across `window` (`Annual` or `Rolling365`).
pub fn summarize_calendar_window(
    days: &[ContributionDay],
    observed_on: NaiveDate,
    window: ContributionWindow,
) -> (u64, u64, u32, u32) {
    let mut sorted = days.to_vec();
    sorted.sort_by_key(|d| d.date);

    let mut total = 0_u64;
    let mut monthly = 0_u64;
    let mut current_streak = 0_u32;
    let mut longest_streak = 0_u32;
    let rolling_cutoff = observed_on - Duration::days(364);

    for day in &sorted {
        if day.date > observed_on {
            continue;
        }
        let in_window = match window {
            ContributionWindow::Annual => day.date.year_ce() == observed_on.year_ce(),
            ContributionWindow::Rolling365 => day.date >= rolling_cutoff,
            ContributionWindow::Lifetime => true,
        };
        if in_window {
            total += u64::from(day.count);
        }
        if day.date.year_ce() == observed_on.year_ce() && day.date.month0() == observed_on.month0()
        {
            monthly += u64::from(day.count);
        }
        if day.count > 0 {
            current_streak += 1;
            longest_streak = longest_streak.max(current_streak);
        } else {
            current_streak = 0;
        }
    }

    (total, monthly, current_streak, longest_streak)
}

/// Aggregate repository languages and rank from highest share to lowest.
pub fn rank_languages(raw_counts: BTreeMap<String, (String, u64)>) -> Vec<LanguageShare> {
    let mut list: Vec<LanguageShare> = raw_counts
        .into_iter()
        .map(|(name, (hex_color, weight))| LanguageShare {
            name,
            hex_color,
            weight,
        })
        .collect();
    list.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
    list
}
