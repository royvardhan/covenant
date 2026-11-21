mod common;

use billing_service::{BillingError, Ledger, Org};
use common::Lcg;

#[test]
fn usage_never_exceeds_limit_under_random_charges() {
    for seed in 1..=200 {
        let mut rng = Lcg::new(seed);
        let org = Org {
            id: 1,
            limit: 1 + rng.below(10_000) as i64,
        };
        let mut ledger = Ledger::new();
        for i in 0..100 {
            let amount = 1 + rng.below(org.limit as u64 * 2) as i64;
            let key = if rng.below(4) == 0 {
                format!("k{}", rng.below(5))
            } else {
                format!("k{i}")
            };
            match ledger.charge(&org, &key, amount) {
                Ok(usage) => assert!(
                    usage <= org.limit,
                    "seed {seed}: usage {usage} > limit {}",
                    org.limit
                ),
                Err(BillingError::OverLimit {
                    requested,
                    available,
                }) => {
                    assert!(
                        requested > available,
                        "seed {seed}: rejected a charge that fit"
                    );
                }
                Err(e) => panic!("seed {seed}: unexpected {e:?}"),
            }
            assert!(
                ledger.usage(org.id) <= org.limit,
                "seed {seed}: ledger drifted over limit"
            );
        }
    }
}

#[test]
fn charge_exactly_to_the_limit_is_allowed_and_next_cent_is_rejected() {
    let org = Org { id: 7, limit: 500 };
    let mut ledger = Ledger::new();
    assert_eq!(ledger.charge(&org, "a", 300), Ok(300));
    assert_eq!(ledger.charge(&org, "b", 200), Ok(500));
    assert_eq!(
        ledger.charge(&org, "c", 1),
        Err(BillingError::OverLimit {
            requested: 1,
            available: 0
        })
    );
    assert_eq!(ledger.usage(org.id), 500);
}

#[test]
fn single_charge_larger_than_limit_is_rejected_even_on_fresh_period() {
    let org = Org { id: 7, limit: 500 };
    let mut ledger = Ledger::new();
    assert!(ledger.charge(&org, "a", 501).is_err());
    assert_eq!(ledger.usage(org.id), 0);
}
