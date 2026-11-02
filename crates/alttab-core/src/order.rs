use crate::ItemKey;
use std::collections::BTreeMap;

/// Only observed attention advances this ledger; discovery and traversal do not.
/// Upstream policy: src/window-tracking/AttentionModel.swift:26–88.
#[derive(Clone, Debug, Default)]
pub struct Mru {
    ranks: BTreeMap<ItemKey, u64>,
    epoch: u64,
    seeded: bool,
}
impl Mru {
    pub fn seed(&mut self, front_to_back: &[ItemKey]) {
        if self.seeded || self.epoch != 0 {
            return;
        }
        for (index, key) in front_to_back.iter().rev().enumerate() {
            self.ranks.entry(*key).or_insert(index as u64 + 1);
        }
        self.epoch = front_to_back.len() as u64;
        self.seeded = true;
    }
    pub fn freeze_seed(&mut self) {
        self.seeded = true;
    }
    pub fn focus(&mut self, key: ItemKey) {
        if self.epoch == u64::MAX {
            let mut ordered: Vec<_> = self.ranks.iter().map(|(k, r)| (*k, *r)).collect();
            ordered.sort_by_key(|(k, r)| (*r, *k));
            for (index, (key, _)) in ordered.iter().enumerate() {
                self.ranks.insert(*key, index as u64 + 1);
            }
            self.epoch = ordered.len() as u64;
        }
        self.epoch += 1;
        self.ranks.insert(key, self.epoch);
        self.seeded = true;
    }
    pub fn rank(&self, key: ItemKey) -> u64 {
        self.ranks.get(&key).copied().unwrap_or(0)
    }
    pub fn remove(&mut self, key: ItemKey) {
        self.ranks.remove(&key);
    }
    pub fn remove_app(&mut self, app: crate::AppKey) {
        self.ranks.retain(|key, _| key.app() != app);
    }
    pub fn compare(&self, a: ItemKey, b: ItemKey) -> std::cmp::Ordering {
        self.rank(b).cmp(&self.rank(a)).then_with(|| a.cmp(&b))
    }
}
