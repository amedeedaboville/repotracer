use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use gix::Commit;
use gix::Repository;
use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;
use std::option::Option;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub enum Granularity {
    #[serde(alias = "infinite", alias = "INFINITE", alias = "all", alias = "ALL")]
    Infinite,
    #[serde(alias = "daily", alias = "DAILY", alias = "day", alias = "DAY")]
    Daily,
    #[serde(alias = "hourly", alias = "HOURLY", alias = "hour", alias = "HOUR")]
    Hourly,
    EveryXHours(i64),
    #[serde(alias = "weekly", alias = "WEEKLY", alias = "week", alias = "WEEK")]
    Weekly,
    #[serde(alias = "monthly", alias = "MONTHLY", alias = "month", alias = "MONTH")]
    Monthly,
}
impl Granularity {
    pub fn from_string(s: &str) -> Option<Self> {
        match s {
            "infinite" | "all" => Some(Self::Infinite),
            "day" | "daily" => Some(Self::Daily),
            "hour" | "hourly" => Some(Self::Hourly),
            // "every_x_hours" => Some(Self::EveryXHours(x)),
            "week" | "weekly" => Some(Self::Weekly),
            "month" | "monthly" => Some(Self::Monthly),
            _ => None,
        }
    }
}

pub fn list_commits_with_granularity(
    repo: &Repository,
    granularity: Granularity,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Result<Vec<Commit>, Box<dyn std::error::Error>> {
    let revwalk = repo
        .rev_walk(repo.head_id())
        .first_parent_only()
        .use_commit_graph(true)
        .all()?;

    let mut commits_by_period = BTreeMap::new();
    let mut all_commits = Vec::new();

    // let commit_oids =
    //     revwalk.filter_map(|info_res| match info_res {
    //         Ok(info) => Some((info.id, info.object().unwrap().tree().unwrap().id)),
    //         Err(e) => {
    //             println!("Error with commit: {:?}", e);
    //             None
    //         }
    //     });
    for info_result in revwalk {
        let info = info_result?;
        let commit = info.object().unwrap();
        let _tree = commit.tree().unwrap();
        let commit_time = commit.time()?;
        let datetime = DateTime::from_timestamp(commit_time.seconds, 0).unwrap();

        // If the commit is before the start time, end the loop early
        if let Some(start) = start {
            if datetime < start {
                break;
            }
        }

        // If the commit is after the end time, skip this commit
        if let Some(end) = end {
            if datetime > end {
                continue;
            }
        }

        match granularity {
            Granularity::Infinite => {
                all_commits.push(commit);
                continue;
            }
            _ => {}
        }

        let key = match granularity {
            Granularity::Daily => datetime.format("%Y-%m-%d").to_string(),
            Granularity::Hourly => datetime.format("%Y-%m-%d %H").to_string(),
            Granularity::EveryXHours(x) => {
                let hour_rounded = datetime.hour() / x as u32 * x as u32;
                format!("{} {:02}", datetime.format("%Y-%m-%d"), hour_rounded)
            }
            Granularity::Weekly => {
                let num_days = datetime.weekday().num_days_from_sunday();
                let start_of_week = datetime - Duration::days(num_days.into());
                start_of_week.format("%Y-%m-%d").to_string()
            }
            Granularity::Monthly => datetime.format("%Y-%m").to_string(),
            Granularity::Infinite => unreachable!(), // Handled above
        };

        commits_by_period.entry(key).or_insert_with(|| commit);
    }

    let commits = if granularity == Granularity::Infinite {
        all_commits
    } else {
        commits_by_period.into_values().collect()
    };

    Ok(commits)
}

/*
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use gix::Repository;

    fn test_repo_path() -> String {
        // Specify the path to a test repository
        "/path/to/your/test/repo".into()
    }

    #[test]
    fn test_infinite_granularity() {
        let repo = Repository::open(test_repo_path()).unwrap();
        let commits =
            list_commits_with_granularity(&repo, Granularity::Infinite, None, None).unwrap();
        assert!(
            !commits.is_empty(),
            "Should return all commits for infinite granularity"
        );
    }

    #[test]
    fn test_daily_granularity() {
        let repo = Repository::open(test_repo_path()).unwrap();
        let start = Utc.ymd(2022, 1, 1).and_hms(0, 0, 0);
        let end = Utc.ymd(2022, 1, 3).and_hms(23, 59, 59);
        let commits =
            list_commits_with_granularity(&repo, Granularity::Daily, Some(start), Some(end))
                .unwrap();
        assert!(
            commits.len() <= 3,
            "Should return at most one commit per day within the range"
        );
    }

    #[test]
    fn test_hourly_granularity_no_commits() {
        let repo = Repository::open(test_repo_path()).unwrap();
        let start = Utc.ymd(2099, 1, 1).and_hms(0, 0, 0); // Future date to ensure no commits
        let end = Utc.ymd(2099, 1, 1).and_hms(23, 59, 59);
        let commits =
            list_commits_with_granularity(&repo, Granularity::Hourly, Some(start), Some(end))
                .unwrap();
        assert!(
            commits.is_empty(),
            "Should return no commits for a future date range"
        );
    }
}

*/
