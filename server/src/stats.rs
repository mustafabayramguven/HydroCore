//! Summary statistics. Averages follow the Android app's History screen: they are taken
//! over *finished* days (strictly before today) that have data.

use crate::db::DayRow;
use chrono::{Duration, NaiveDate};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DayBrief {
    pub date: String,
    pub intake_ml: i64,
    pub daily_goal_ml: i64,
    pub goal_reached: bool,
    /// `0.0..=1.0`
    pub progress: f64,
}

impl From<&DayRow> for DayBrief {
    fn from(d: &DayRow) -> Self {
        DayBrief {
            date: d.date.clone(),
            intake_ml: d.intake_ml,
            daily_goal_ml: d.daily_goal_ml,
            goal_reached: d.intake_ml >= d.daily_goal_ml,
            progress: if d.daily_goal_ml > 0 {
                (d.intake_ml as f64 / d.daily_goal_ml as f64).min(1.0)
            } else {
                1.0
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Summary {
    pub today: Option<DayBrief>,
    pub avg_last7_ml: Option<i64>,
    pub avg_last30_ml: Option<i64>,
    pub avg_older_than7_ml: Option<i64>,
    pub avg_all_time_ml: Option<i64>,
    pub total_volume_ml: i64,
    pub recorded_days: usize,
    pub goal_met_days: usize,
    pub best_day: Option<DayBrief>,
    /// Consecutive goal-met days ending today (or yesterday while today is still open).
    pub current_streak_days: u32,
    pub longest_streak_days: u32,
}

fn parse(d: &DayRow) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").ok()
}

fn avg<'a>(it: impl Iterator<Item = &'a DayRow>) -> Option<i64> {
    let (sum, n) = it.fold((0i64, 0i64), |(s, n), d| (s + d.intake_ml, n + 1));
    (n > 0).then(|| sum / n)
}

pub fn summarize(days: &[DayRow], today: NaiveDate) -> Summary {
    let dated: Vec<(NaiveDate, &DayRow)> = days.iter().filter_map(|d| parse(d).map(|n| (n, d))).collect();

    let cutoff7 = today - Duration::days(7);
    let cutoff30 = today - Duration::days(30);
    let finished = |lo: NaiveDate| {
        dated
            .iter()
            .filter(move |(n, _)| *n < today && *n >= lo)
            .map(|(_, d)| *d)
    };

    let met: HashSet<NaiveDate> = dated
        .iter()
        .filter(|(_, d)| d.intake_ml >= d.daily_goal_ml)
        .map(|(n, _)| *n)
        .collect();

    let mut cursor = if met.contains(&today) { today } else { today - Duration::days(1) };
    let mut current = 0u32;
    while met.contains(&cursor) {
        current += 1;
        cursor -= Duration::days(1);
    }

    let mut sorted: Vec<&NaiveDate> = met.iter().collect();
    sorted.sort();
    let (mut longest, mut run, mut prev): (u32, u32, Option<NaiveDate>) = (0, 0, None);
    for d in sorted {
        run = if prev.is_some_and(|p| p + Duration::days(1) == *d) { run + 1 } else { 1 };
        longest = longest.max(run);
        prev = Some(*d);
    }

    Summary {
        today: dated.iter().find(|(n, _)| *n == today).map(|(_, d)| DayBrief::from(*d)),
        avg_last7_ml: avg(finished(cutoff7)),
        avg_last30_ml: avg(finished(cutoff30)),
        avg_older_than7_ml: avg(dated.iter().filter(|(n, _)| *n < cutoff7).map(|(_, d)| *d)),
        avg_all_time_ml: avg(dated.iter().map(|(_, d)| *d)),
        total_volume_ml: dated.iter().map(|(_, d)| d.intake_ml).sum(),
        recorded_days: dated.len(),
        goal_met_days: met.len(),
        best_day: dated
            .iter()
            .max_by_key(|(n, d)| (d.intake_ml, *n))
            .map(|(_, d)| DayBrief::from(*d)),
        current_streak_days: current,
        longest_streak_days: longest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str, intake: i64, goal: i64) -> DayRow {
        DayRow {
            date: date.into(),
            intake_ml: intake,
            daily_goal_ml: goal,
            entry_count: 1,
            last_sync_utc: "x".into(),
        }
    }
    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn empty_is_all_none() {
        let s = summarize(&[], d("2026-11-01"));
        assert_eq!(s.today, None);
        assert_eq!(s.avg_all_time_ml, None);
        assert_eq!((s.recorded_days, s.current_streak_days, s.longest_streak_days), (0, 0, 0));
    }

    #[test]
    fn averages_use_finished_days_only() {
        let days = vec![
            day("2026-10-20", 1000, 2000), // older than 7 days
            day("2026-10-30", 2000, 2000),
            day("2026-10-31", 3000, 2000),
            day("2026-11-01", 500, 2000), // today: excluded from 7/30 averages
        ];
        let s = summarize(&days, d("2026-11-01"));
        assert_eq!(s.avg_last7_ml, Some(2500));
        assert_eq!(s.avg_last30_ml, Some(2000));
        assert_eq!(s.avg_older_than7_ml, Some(1000));
        assert_eq!(s.avg_all_time_ml, Some(1625));
        assert_eq!(s.total_volume_ml, 6500);
        assert_eq!(s.goal_met_days, 2);
        assert_eq!(s.today.as_ref().unwrap().intake_ml, 500);
        assert_eq!(s.best_day.unwrap().date, "2026-10-31");
    }

    #[test]
    fn streak_survives_an_open_day_but_not_a_gap() {
        let days = vec![
            day("2026-10-28", 2000, 2000),
            day("2026-10-30", 2000, 2000),
            day("2026-10-31", 2100, 2000),
            day("2026-11-01", 100, 2000),
        ];
        let s = summarize(&days, d("2026-11-01"));
        assert_eq!(s.current_streak_days, 2); // 30th + 31st; today still open
        assert_eq!(s.longest_streak_days, 2);

        let s = summarize(&days, d("2026-11-03")); // 2 days with nothing: streak is broken
        assert_eq!(s.current_streak_days, 0);
    }

    #[test]
    fn today_counts_once_goal_is_met() {
        let days = vec![day("2026-10-31", 2000, 2000), day("2026-11-01", 2500, 2000)];
        assert_eq!(summarize(&days, d("2026-11-01")).current_streak_days, 2);
    }
}
