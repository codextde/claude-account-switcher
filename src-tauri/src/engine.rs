//! Pure auto-switch decision logic: threshold plus hysteresis, no side effects.
//!
//! When the active account's binding window (the higher of its 5-hour and 7-day
//! utilization) reaches the threshold, pick the account with the most quota left,
//! but only one sitting at least `hysteresis` below the threshold so two accounts
//! hovering at the line never ping-pong.

use uuid::Uuid;

use crate::models::Usage;

/// The utilization that binds an account: the max over the windows we watch.
///
/// A window whose `resets_at` is in the past is discarded: the quota it reports as
/// consumed no longer exists. With `require_known_window` a window without a parseable
/// reset time is discarded too. That is required for the ACTIVE account when the sample
/// is stale, so a retained high reading with unknown expiry can never trigger a switch
/// long after the real window reset.
pub fn binding_utilization(usage: Option<&Usage>, now_ms: i64, require_known_window: bool) -> Option<f64> {
    let usage = usage?;
    [usage.five_hour.as_ref(), usage.seven_day.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let util = w.utilization?;
            match w.resets_at_ms() {
                Some(reset) if reset < now_ms => None,
                Some(_) => Some(util),
                None if require_known_window => None,
                None => Some(util),
            }
        })
        .fold(None, |acc: Option<f64>, u| Some(acc.map_or(u, |a| a.max(u))))
}

pub struct Candidate<'a> {
    pub id: Uuid,
    pub usage: Option<&'a Usage>,
    /// Has a stored backup and is not flagged as needing re-authentication.
    pub switchable: bool,
}

pub struct Decision {
    pub active_utilization: f64,
    /// Best target first (lowest known utilization).
    pub targets: Vec<(Uuid, f64)>,
}

/// Returns `None` when the active account is below the threshold or its usage is unknown.
/// Otherwise returns the ranked list of eligible targets (which may be empty).
pub fn decide(
    active_usage: Option<&Usage>,
    active_sampled_this_cycle: bool,
    candidates: &[Candidate<'_>],
    threshold: f64,
    hysteresis: f64,
    now_ms: i64,
) -> Option<Decision> {
    let active_utilization = binding_utilization(active_usage, now_ms, !active_sampled_this_cycle)?;
    if active_utilization < threshold {
        return None;
    }
    let ceiling = threshold - hysteresis;
    let mut targets: Vec<(Uuid, f64)> = candidates
        .iter()
        .filter(|c| c.switchable)
        .filter_map(|c| {
            // No sample means "not polled yet", never "idle": an unknown account is not a target.
            let util = binding_utilization(c.usage, now_ms, false)?;
            (util <= ceiling).then_some((c.id, util))
        })
        .collect();
    targets.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    Some(Decision {
        active_utilization,
        targets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UsageWindow;

    fn usage(five: f64, seven: f64, reset_ms: i64) -> Usage {
        let reset = chrono::DateTime::from_timestamp_millis(reset_ms).unwrap().to_rfc3339();
        Usage {
            five_hour: Some(UsageWindow {
                utilization: Some(five),
                resets_at: Some(reset.clone()),
            }),
            seven_day: Some(UsageWindow {
                utilization: Some(seven),
                resets_at: Some(reset),
            }),
            ..Default::default()
        }
    }

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn binding_is_max_of_windows() {
        let u = usage(30.0, 75.0, NOW + 1000);
        assert_eq!(binding_utilization(Some(&u), NOW, false), Some(75.0));
    }

    #[test]
    fn expired_windows_are_ignored() {
        let u = usage(95.0, 95.0, NOW - 1000);
        assert_eq!(binding_utilization(Some(&u), NOW, false), None);
    }

    #[test]
    fn unknown_reset_requires_fresh_sample_for_active() {
        let u = Usage {
            five_hour: Some(UsageWindow {
                utilization: Some(99.0),
                resets_at: None,
            }),
            ..Default::default()
        };
        assert_eq!(binding_utilization(Some(&u), NOW, true), None);
        assert_eq!(binding_utilization(Some(&u), NOW, false), Some(99.0));
    }

    #[test]
    fn picks_lowest_candidate_under_ceiling() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let active = usage(92.0, 40.0, NOW + 1000);
        let ua = usage(50.0, 10.0, NOW + 1000);
        let ub = usage(20.0, 30.0, NOW + 1000);
        let uc = usage(85.0, 0.0, NOW + 1000); // above ceiling 80
        let cands = vec![
            Candidate {
                id: a,
                usage: Some(&ua),
                switchable: true,
            },
            Candidate {
                id: b,
                usage: Some(&ub),
                switchable: true,
            },
            Candidate {
                id: c,
                usage: Some(&uc),
                switchable: true,
            },
        ];
        let d = decide(Some(&active), true, &cands, 90.0, 10.0, NOW).unwrap();
        assert_eq!(d.active_utilization, 92.0);
        assert_eq!(d.targets.iter().map(|t| t.0).collect::<Vec<_>>(), vec![b, a]);
    }

    #[test]
    fn stays_put_below_threshold_or_without_samples() {
        let a = Uuid::new_v4();
        let active = usage(50.0, 40.0, NOW + 1000);
        let cands = vec![Candidate {
            id: a,
            usage: None,
            switchable: true,
        }];
        assert!(decide(Some(&active), true, &cands, 90.0, 10.0, NOW).is_none());
        let hot = usage(95.0, 40.0, NOW + 1000);
        let d = decide(Some(&hot), true, &cands, 90.0, 10.0, NOW).unwrap();
        assert!(d.targets.is_empty());
    }
}
