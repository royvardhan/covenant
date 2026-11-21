use billing_service::{Ledger, Org};

#[test]
fn retrying_a_charge_with_the_same_key_charges_once() {
    let org = Org {
        id: 1,
        limit: 1_000,
    };
    let mut ledger = Ledger::new();
    assert_eq!(ledger.charge(&org, "req-1", 400), Ok(400));
    assert_eq!(ledger.charge(&org, "req-1", 400), Ok(400));
    assert_eq!(ledger.charge(&org, "req-1", 999), Ok(400));
    assert_eq!(ledger.usage(org.id), 400);
}

#[test]
fn idempotency_keys_are_scoped_per_org() {
    let a = Org {
        id: 1,
        limit: 1_000,
    };
    let b = Org {
        id: 2,
        limit: 1_000,
    };
    let mut ledger = Ledger::new();
    ledger.charge(&a, "req-1", 100).unwrap();
    ledger.charge(&b, "req-1", 100).unwrap();
    assert_eq!(ledger.usage(a.id), 100);
    assert_eq!(ledger.usage(b.id), 100);
}

#[test]
fn a_rejected_charge_does_not_consume_its_key() {
    let org = Org { id: 1, limit: 100 };
    let mut ledger = Ledger::new();
    assert!(ledger.charge(&org, "req-1", 150).is_err());
    ledger.refund(&org, 1).ok();
    assert_eq!(ledger.charge(&org, "req-1", 50), Ok(50));
}

#[test]
fn closing_a_period_resets_keys_and_usage() {
    let org = Org { id: 1, limit: 100 };
    let mut ledger = Ledger::new();
    ledger.charge(&org, "req-1", 60).unwrap();
    assert_eq!(ledger.close_period(&org), 60);
    assert_eq!(ledger.usage(org.id), 0);
    assert_eq!(ledger.charge(&org, "req-1", 60), Ok(60));
}
