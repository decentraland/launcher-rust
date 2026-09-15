//! Cohort assignment for canary releases.
//!
//! The split has to be stable (the same user must not flip between the canary and the regular
//! build across launches) and it has to work without a backend counter: the only thing the
//! backend owns is `canary.json` — the download url and the cohort percentage. The launcher
//! derives the bucket locally from the anonymous analytics id, which is generated once and
//! persisted in `config.json`, so the assignment is deterministic per install.

/// Number of buckets an anonymous id is hashed into. A cohort of `N` claims buckets `0..N`,
/// so the cohort value reads directly as a percentage of users.
const BUCKETS: u64 = 100;

/// Maximum meaningful cohort value — every user falls under the canary.
pub const COHORT_MAX: u8 = 100;

/// FNV-1a (64-bit). Rolled by hand instead of using [`std::hash::DefaultHasher`] because the
/// std hasher gives no stability guarantee across toolchain versions — a cohort that reshuffles
/// on a launcher rebuild would move users between builds behind our back.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Stable bucket in `0..100` for an anonymous user id.
pub fn cohort_bucket(anon_id: &str) -> u64 {
    fnv1a_64(anon_id.as_bytes()) % BUCKETS
}

/// Whether the given anonymous id falls under a cohort of `cohort` percent of users.
///
/// `cohort == 0` excludes everyone, `cohort >= 100` includes everyone.
pub fn is_in_cohort(anon_id: &str, cohort: u8) -> bool {
    cohort_bucket(anon_id) < u64::from(cohort)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_is_stable_for_the_same_id() {
        let id = "0d1b2e3f-4a5b-6c7d-8e9f-a0b1c2d3e4f5";
        let first = cohort_bucket(id);
        for _ in 0..10 {
            assert_eq!(cohort_bucket(id), first);
        }
    }

    #[test]
    fn bucket_is_always_within_range() {
        for i in 0..1000 {
            assert!(cohort_bucket(&format!("user-{}", i)) < BUCKETS);
        }
    }

    #[test]
    fn empty_cohort_excludes_everyone() {
        for i in 0..1000 {
            assert!(!is_in_cohort(&format!("user-{}", i), 0));
        }
    }

    #[test]
    fn full_cohort_includes_everyone() {
        for i in 0..1000 {
            assert!(is_in_cohort(&format!("user-{}", i), COHORT_MAX));
        }
    }

    #[test]
    fn cohort_above_max_still_includes_everyone() {
        for i in 0..1000 {
            assert!(is_in_cohort(&format!("user-{}", i), u8::MAX));
        }
    }

    #[test]
    fn half_cohort_splits_roughly_in_half() {
        const SAMPLES: usize = 20_000;
        // Deterministic ids, so the assertion cannot flake on a random draw.
        let assigned = (0..SAMPLES)
            .filter(|i| is_in_cohort(&format!("dcl-anon-{}", i), 50))
            .count();

        assert!(
            (SAMPLES * 45 / 100..=SAMPLES * 55 / 100).contains(&assigned),
            "50% cohort assigned {} of {} users",
            assigned,
            SAMPLES
        );
    }

    #[test]
    fn cohort_is_monotonic_in_size() {
        // Growing the cohort may only ever add users, never move someone out of it.
        let ids: Vec<String> = (0..500).map(|i| format!("dcl-anon-{}", i)).collect();
        for cohort in 0..COHORT_MAX {
            for id in &ids {
                if is_in_cohort(id, cohort) {
                    assert!(is_in_cohort(id, cohort + 1));
                }
            }
        }
    }
}
