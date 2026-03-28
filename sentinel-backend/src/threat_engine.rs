use crate::types::ThreatProfile;

/// Compute threat score 0-10000 (basis points) from a profile.
///
/// Tuned for EVE Frontier where 15 kills in 24h is extreme.
///
/// Factors:
///   Kill count:  0-2000  — log2(kills+1) * 600
///   Recency:     0-3500  — recent_kills_24h * 600 (dominant for active pilots)
///   K/D ratio:   0-1500  — kd * 400
///   Bounties:    0-1500  — bounty_count * 500
///   Movement:    0-500   — systems_visited * 100
pub fn compute_score(profile: &ThreatProfile) -> u64 {
    let kill_factor = ((profile.kill_count as f64 + 1.0).log2() * 600.0).min(2000.0) as u64;

    let recency_factor = (profile.recent_kills_24h * 600).min(3500);

    let kd = if profile.death_count > 0 {
        profile.kill_count as f64 / profile.death_count as f64
    } else {
        profile.kill_count as f64
    };
    let kd_factor = (kd * 400.0).min(1500.0) as u64;

    let bounty_factor = (profile.bounty_count * 500).min(1500);

    let movement_factor = (profile.systems_visited * 100).min(500);

    let total = recency_factor + kill_factor + kd_factor + bounty_factor + movement_factor;
    total.min(10000)
}

/// Return the threat tier label for a score.
#[allow(dead_code)]
pub fn threat_tier(score: u64) -> &'static str {
    match score {
        0..=2500 => "LOW",
        2501..=5000 => "MODERATE",
        5001..=7500 => "HIGH",
        _ => "CRITICAL",
    }
}

/// Compute earned titles based on profile stats.
/// Returns a list of title strings — a pilot can earn multiple.
pub fn earned_titles(profile: &ThreatProfile) -> Vec<&'static str> {
    let mut titles = Vec::new();

    // Combat titles
    if profile.kill_count >= 10 && profile.death_count <= 2 {
        titles.push("Serial Killer");
    }
    if profile.kill_count >= 50 {
        titles.push("Apex Predator");
    }
    if profile.kill_count >= 5 && profile.death_count == 0 {
        titles.push("Untouchable");
    }
    if profile.recent_kills_24h >= 5 {
        titles.push("Rampage");
    }

    // Bounty titles
    if profile.bounty_count >= 3 {
        titles.push("Most Wanted");
    }
    if profile.bounty_count >= 1 {
        titles.push("Bounty Magnet");
    }

    // Movement titles
    if profile.systems_visited >= 10 {
        titles.push("Nomad");
    }
    if profile.systems_visited >= 20 {
        titles.push("Cartographer");
    }
    if profile.systems_visited >= 5 && profile.kill_count == 0 {
        titles.push("Ghost");
    }

    // Threat titles
    if profile.threat_score >= 7500 {
        titles.push("Gate Threat");
    }
    if profile.threat_score >= 9000 {
        titles.push("Public Enemy");
    }

    // Survival titles
    if profile.death_count >= 10 && profile.kill_count <= 2 {
        titles.push("Frequent Victim");
    }
    if profile.death_count >= 5 && profile.kill_count >= 5 {
        titles.push("Warrior");
    }

    // K/D titles
    let kd = if profile.death_count > 0 {
        profile.kill_count as f64 / profile.death_count as f64
    } else {
        profile.kill_count as f64
    };
    if kd >= 3.0 && profile.kill_count >= 5 {
        titles.push("Efficient Killer");
    }

    titles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_profile_scores_zero() {
        let p = ThreatProfile::default();
        assert_eq!(compute_score(&p), 0);
    }

    #[test]
    fn high_kill_count_scores_high() {
        let p = ThreatProfile {
            kill_count: 100,
            death_count: 10,
            recent_kills_24h: 3,
            bounty_count: 2,
            systems_visited: 5,
            ..Default::default()
        };
        let score = compute_score(&p);
        assert!(score > 5000, "score was {score}");
    }

    #[test]
    fn score_capped_at_theoretical_max() {
        let p = ThreatProfile {
            kill_count: 10000,
            death_count: 1,
            recent_kills_24h: 100,
            bounty_count: 100,
            systems_visited: 100,
            ..Default::default()
        };
        // Max: 2000 + 3500 + 1500 + 1500 + 500 = 9000
        assert_eq!(compute_score(&p), 9000);
    }

    #[test]
    fn tier_labels() {
        assert_eq!(threat_tier(0), "LOW");
        assert_eq!(threat_tier(2500), "LOW");
        assert_eq!(threat_tier(2501), "MODERATE");
        assert_eq!(threat_tier(5001), "HIGH");
        assert_eq!(threat_tier(7501), "CRITICAL");
    }

    #[test]
    fn kill_factor_is_logarithmic() {
        // Isolate kill factor: equal kills and deaths so kd=1 for all
        let p1 = ThreatProfile {
            kill_count: 2,
            death_count: 2,
            ..Default::default()
        };
        let p2 = ThreatProfile {
            kill_count: 8,
            death_count: 8,
            ..Default::default()
        };
        let p3 = ThreatProfile {
            kill_count: 64,
            death_count: 64,
            ..Default::default()
        };
        let s1 = compute_score(&p1);
        let s2 = compute_score(&p2);
        let s3 = compute_score(&p3);
        // kd_factor is constant (1.0 * 500 = 500 for all)
        // kill_factor: log2(3)*900=1423, log2(9)*900=2849, log2(65)*900=3000(capped)
        let gain_1_to_2 = s2 - s1;
        let gain_2_to_3 = s3 - s2;
        assert!(gain_1_to_2 > 0, "should have positive gain from 2->8 kills");
        assert!(
            gain_2_to_3 < gain_1_to_2,
            "8->64 gain ({gain_2_to_3}) should be less than 2->8 gain ({gain_1_to_2})"
        );
    }

    #[test]
    fn kd_factor_caps_at_1500() {
        let p = ThreatProfile {
            kill_count: 1000,
            death_count: 1,
            ..Default::default()
        };
        let score = compute_score(&p);
        // kill_factor (log2(1001)*600 capped at 2000) + kd_factor (capped 1500) = 3500
        assert_eq!(score, 3500);
    }

    #[test]
    fn zero_deaths_uses_kills_as_kd() {
        let p = ThreatProfile {
            kill_count: 3,
            death_count: 0,
            ..Default::default()
        };
        let score = compute_score(&p);
        // kill_factor: log2(4)*600 = 1200
        // kd_factor: 3 * 400 = 1200
        assert_eq!(score, 1200 + 1200);
    }

    #[test]
    fn recency_factor_caps_at_3500() {
        let p = ThreatProfile {
            recent_kills_24h: 100,
            ..Default::default()
        };
        let score = compute_score(&p);
        assert_eq!(score, 3500);
    }

    #[test]
    fn bounty_factor_caps_at_1500() {
        let p = ThreatProfile {
            bounty_count: 100,
            ..Default::default()
        };
        let score = compute_score(&p);
        assert_eq!(score, 1500);
    }

    #[test]
    fn movement_factor_caps_at_500() {
        let p = ThreatProfile {
            systems_visited: 100,
            ..Default::default()
        };
        let score = compute_score(&p);
        assert_eq!(score, 500);
    }

    #[test]
    fn each_factor_contributes_independently() {
        let kills_only = compute_score(&ThreatProfile {
            kill_count: 50,
            ..Default::default()
        });
        let recency_only = compute_score(&ThreatProfile {
            recent_kills_24h: 2,
            ..Default::default()
        });
        let bounty_only = compute_score(&ThreatProfile {
            bounty_count: 1,
            ..Default::default()
        });

        assert!(kills_only > 0);
        assert!(recency_only > 0);
        assert!(bounty_only > 0);

        // Combined should be sum of individual factors
        let combined = ThreatProfile {
            kill_count: 50,
            recent_kills_24h: 2,
            bounty_count: 1,
            ..Default::default()
        };
        assert_eq!(
            compute_score(&combined),
            kills_only + recency_only + bounty_only
        );
    }

    // === Title tests ===

    #[test]
    fn no_titles_for_empty_profile() {
        let p = ThreatProfile::default();
        assert!(earned_titles(&p).is_empty());
    }

    #[test]
    fn serial_killer_title() {
        let p = ThreatProfile {
            kill_count: 10,
            death_count: 2,
            ..Default::default()
        };
        assert!(earned_titles(&p).contains(&"Serial Killer"));
    }

    #[test]
    fn most_wanted_title() {
        let p = ThreatProfile {
            bounty_count: 3,
            ..Default::default()
        };
        let titles = earned_titles(&p);
        assert!(titles.contains(&"Most Wanted"));
        assert!(titles.contains(&"Bounty Magnet"));
    }

    #[test]
    fn ghost_title() {
        let p = ThreatProfile {
            systems_visited: 8,
            kill_count: 0,
            ..Default::default()
        };
        assert!(earned_titles(&p).contains(&"Ghost"));
    }

    #[test]
    fn rampage_title() {
        let p = ThreatProfile {
            recent_kills_24h: 6,
            ..Default::default()
        };
        assert!(earned_titles(&p).contains(&"Rampage"));
    }

    #[test]
    fn multiple_titles() {
        let p = ThreatProfile {
            kill_count: 55,
            death_count: 1,
            recent_kills_24h: 6,
            bounty_count: 4,
            systems_visited: 12,
            threat_score: 9500,
            ..Default::default()
        };
        let titles = earned_titles(&p);
        assert!(titles.contains(&"Apex Predator"));
        assert!(titles.contains(&"Rampage"));
        assert!(titles.contains(&"Most Wanted"));
        assert!(titles.contains(&"Nomad"));
        assert!(titles.contains(&"Public Enemy"));
    }
}
