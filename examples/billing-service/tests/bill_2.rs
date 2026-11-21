mod common;

use billing_service::{BillingError, Ledger, Org};
use common::Lcg;

#[test]
fn refund_larger_than_usage_is_rejected() {
    let org = Org {
        id: 1,
        limit: 1_000,
    };
    let mut ledger = Ledger::new();
    ledger.charge(&org, "a", 100).unwrap();
    assert_eq!(
        ledger.refund(&org, 101),
        Err(BillingError::RefundExceedsUsage {
            requested: 101,
            usage: 100
        })
    );
    assert_eq!(ledger.refund(&org, 100), Ok(0));
    assert_eq!(
        ledger.refund(&org, 1),
        Err(BillingError::RefundExceedsUsage {
            requested: 1,
            usage: 0
        })
    );
}

#[test]
fn usage_is_never_negative_under_random_charges_and_refunds() {
    for seed in 1..=200 {
        let mut rng = Lcg::new(seed);
        let org = Org {
            id: 2,
            limit: 10_000,
        };
        let mut ledger = Ledger::new();
        for i in 0..200 {
            let amount = 1 + rng.below(3_000) as i64;
            if rng.below(2) == 0 {
                let _ = ledger.charge(&org, &format!("k{i}"), amount);
            } else {
                let _ = ledger.refund(&org, amount);
            }
            assert!(
                ledger.usage(org.id) >= 0,
                "seed {seed}: usage went negative"
            );
        }
    }
}

#[test]
fn zero_and_negative_amounts_are_rejected_everywhere() {
    let org = Org { id: 3, limit: 100 };
    let mut ledger = Ledger::new();
    assert_eq!(
        ledger.charge(&org, "a", 0),
        Err(BillingError::NonPositiveAmount(0))
    );
    assert_eq!(
        ledger.charge(&org, "a", -5),
        Err(BillingError::NonPositiveAmount(-5))
    );
    assert_eq!(
        ledger.refund(&org, -5),
        Err(BillingError::NonPositiveAmount(-5))
    );
    assert_eq!(ledger.usage(org.id), 0);
}
