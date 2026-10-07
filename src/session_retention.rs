use crate::session::SessionSummary;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use uuid::Uuid;

pub const MAX_RETENTION_DAYS: u64 = 36_500;
pub const MAX_RETENTION_COUNT: usize = 100_000;
pub const MAX_RETENTION_PREVIEW: usize = 10_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionSettings {
    pub days: Option<u64>,
    pub count: Option<usize>,
}

impl RetentionSettings {
    pub fn validate(self) -> Result<Self, String> {
        if self.days == Some(0) {
            return Err("retention days must be a positive integer or blank".into());
        }
        if self.days.is_some_and(|days| days > MAX_RETENTION_DAYS) {
            return Err(format!("retention days cannot exceed {MAX_RETENTION_DAYS}"));
        }
        if self.count == Some(0) {
            return Err("retention count must be a positive integer or blank".into());
        }
        if self.count.is_some_and(|count| count > MAX_RETENTION_COUNT) {
            return Err(format!(
                "retention count cannot exceed {MAX_RETENTION_COUNT}"
            ));
        }
        Ok(self)
    }

    pub fn enabled(self) -> bool {
        self.days.is_some() || self.count.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionReason {
    Age,
    Count,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionCandidate {
    pub storage_key: String,
    pub id: Uuid,
    pub title: String,
    pub updated_at: DateTime<Utc>,
    pub reason: RetentionReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPreview {
    pub settings: RetentionSettings,
    pub generated_at: DateTime<Utc>,
    pub active_count: usize,
    pub protected_current: usize,
    pub protected_pinned: usize,
    pub candidates: Vec<RetentionCandidate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionDeleteOutcome {
    pub removed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionDeleteFailure {
    pub message: String,
    pub removed: usize,
    pub remaining: usize,
}

impl std::fmt::Display for RetentionDeleteFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RetentionDeleteFailure {}

pub fn build_preview(
    summaries: &[SessionSummary],
    current_id: Option<Uuid>,
    settings: RetentionSettings,
    now: DateTime<Utc>,
) -> Result<RetentionPreview, String> {
    let settings = settings.validate()?;
    let mut protected_current = 0;
    let mut protected_pinned = 0;
    let eligible = summaries
        .iter()
        .filter(|summary| {
            if current_id == Some(summary.id) {
                protected_current += 1;
                false
            } else if summary.pinned {
                protected_pinned += 1;
                false
            } else {
                true
            }
        })
        .collect::<Vec<_>>();

    let age_ids = if let Some(days) = settings.days {
        let days = i64::try_from(days).map_err(|_| "retention days are too large")?;
        let cutoff = now
            .checked_sub_signed(
                chrono::Duration::try_days(days)
                    .ok_or_else(|| "retention days are too large".to_string())?,
            )
            .ok_or_else(|| "retention cutoff is outside the supported date range".to_string())?;
        eligible
            .iter()
            .filter(|summary| summary.updated_at < cutoff)
            .map(|summary| summary.id)
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };

    let count_ids = if let Some(count) = settings.count {
        let mut newest_first = eligible.clone();
        newest_first.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        newest_first
            .into_iter()
            .skip(count)
            .map(|summary| summary.id)
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };

    let mut candidates = eligible
        .into_iter()
        .filter_map(|summary| {
            let age = age_ids.contains(&summary.id);
            let count = count_ids.contains(&summary.id);
            let reason = match (age, count) {
                (true, true) => RetentionReason::Both,
                (true, false) => RetentionReason::Age,
                (false, true) => RetentionReason::Count,
                (false, false) => return None,
            };
            Some(RetentionCandidate {
                storage_key: summary.storage_key.clone(),
                id: summary.id,
                title: summary.title.clone(),
                updated_at: summary.updated_at,
                reason,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.updated_at
            .cmp(&right.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    if candidates.len() > MAX_RETENTION_PREVIEW {
        return Err(format!(
            "retention would delete {} sessions; narrow the rules to at most {MAX_RETENTION_PREVIEW} per preview",
            candidates.len()
        ));
    }

    Ok(RetentionPreview {
        settings,
        generated_at: now,
        active_count: summaries.len(),
        protected_current,
        protected_pinned,
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn summary(name: &str, seconds_ago: i64, pinned: bool, now: DateTime<Utc>) -> SessionSummary {
        SessionSummary {
            storage_key: name.into(),
            id: Uuid::new_v4(),
            title: name.into(),
            pinned,
            workspace: "/workspace".into(),
            updated_at: now - Duration::seconds(seconds_ago),
        }
    }

    #[test]
    fn disabled_retention_has_an_empty_complete_preview() {
        let now = DateTime::from_timestamp(2_000_000, 0).unwrap();
        let preview = build_preview(
            &[summary("ordinary", 100, false, now)],
            None,
            RetentionSettings::default(),
            now,
        )
        .unwrap();

        assert!(preview.candidates.is_empty());
        assert_eq!(preview.active_count, 1);
    }

    #[test]
    fn age_is_strict_and_current_and_pinned_are_always_protected() {
        let now = DateTime::from_timestamp(4_000_000, 0).unwrap();
        let expired = summary("expired", 31 * 86_400, false, now);
        let boundary = summary("boundary", 30 * 86_400, false, now);
        let pinned = summary("pinned", 90 * 86_400, true, now);
        let current = summary("current", 90 * 86_400, false, now);
        let preview = build_preview(
            &[expired.clone(), boundary, pinned, current.clone()],
            Some(current.id),
            RetentionSettings {
                days: Some(30),
                count: None,
            },
            now,
        )
        .unwrap();

        assert_eq!(preview.protected_current, 1);
        assert_eq!(preview.protected_pinned, 1);
        assert_eq!(preview.candidates.len(), 1);
        assert_eq!(preview.candidates[0].id, expired.id);
        assert_eq!(preview.candidates[0].reason, RetentionReason::Age);
    }

    #[test]
    fn count_keeps_newest_eligible_and_unions_with_age_deterministically() {
        let now = DateTime::from_timestamp(8_000_000, 0).unwrap();
        let newest = summary("newest", 1, false, now);
        let middle = summary("middle", 2, false, now);
        let old = summary("old", 40 * 86_400, false, now);
        let oldest = summary("oldest", 50 * 86_400, false, now);
        let preview = build_preview(
            &[middle, oldest.clone(), newest, old.clone()],
            None,
            RetentionSettings {
                days: Some(45),
                count: Some(2),
            },
            now,
        )
        .unwrap();

        assert_eq!(
            preview
                .candidates
                .iter()
                .map(|candidate| (candidate.id, candidate.reason))
                .collect::<Vec<_>>(),
            vec![
                (oldest.id, RetentionReason::Both),
                (old.id, RetentionReason::Count),
            ]
        );
    }

    #[test]
    fn invalid_settings_and_partial_overlarge_previews_fail_closed() {
        for settings in [
            RetentionSettings {
                days: Some(0),
                count: None,
            },
            RetentionSettings {
                days: Some(MAX_RETENTION_DAYS + 1),
                count: None,
            },
            RetentionSettings {
                days: None,
                count: Some(0),
            },
            RetentionSettings {
                days: None,
                count: Some(MAX_RETENTION_COUNT + 1),
            },
        ] {
            assert!(settings.validate().is_err());
        }

        let now = DateTime::from_timestamp(10_000_000, 0).unwrap();
        let summaries = (0..(MAX_RETENTION_PREVIEW + 2))
            .map(|index| summary(&format!("saved-{index}"), index as i64 + 1, false, now))
            .collect::<Vec<_>>();
        let error = build_preview(
            &summaries,
            None,
            RetentionSettings {
                days: None,
                count: Some(1),
            },
            now,
        )
        .unwrap_err();
        assert!(error.contains("narrow the rules"));
    }
}
