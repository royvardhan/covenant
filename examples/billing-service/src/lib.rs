use std::collections::{HashMap, HashSet};

pub type OrgId = u32;
pub type Cents = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Org {
    pub id: OrgId,
    pub limit: Cents,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BillingError {
    OverLimit { requested: Cents, available: Cents },
    RefundExceedsUsage { requested: Cents, usage: Cents },
    NonPositiveAmount(Cents),
}

#[derive(Debug, Default)]
pub struct Ledger {
    usage: HashMap<OrgId, Cents>,
    seen: HashSet<(OrgId, String)>,
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn usage(&self, org: OrgId) -> Cents {
        self.usage.get(&org).copied().unwrap_or(0)
    }

    pub fn charge(&mut self, org: &Org, key: &str, amount: Cents) -> Result<Cents, BillingError> {
        if amount <= 0 {
            return Err(BillingError::NonPositiveAmount(amount));
        }
        if self.seen.contains(&(org.id, key.to_string())) {
            return Ok(self.usage(org.id));
        }
        let current = self.usage(org.id);
        let available = org.limit - current;
        if amount > available {
            return Err(BillingError::OverLimit {
                requested: amount,
                available,
            });
        }
        self.seen.insert((org.id, key.to_string()));
        let next = current + amount;
        self.usage.insert(org.id, next);
        Ok(next)
    }

    pub fn refund(&mut self, org: &Org, amount: Cents) -> Result<Cents, BillingError> {
        if amount <= 0 {
            return Err(BillingError::NonPositiveAmount(amount));
        }
        let current = self.usage(org.id);
        if amount > current {
            return Err(BillingError::RefundExceedsUsage {
                requested: amount,
                usage: current,
            });
        }
        let next = current - amount;
        self.usage.insert(org.id, next);
        Ok(next)
    }

    pub fn close_period(&mut self, org: &Org) -> Cents {
        self.seen.retain(|(id, _)| *id != org.id);
        self.usage.remove(&org.id).unwrap_or(0)
    }
}
