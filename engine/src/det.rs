//! Deterministic collections. std's HashMap/HashSet seed their hasher
//! randomly per instance, so two peers (or a rollback replay) can iterate
//! the same map in different orders and drift apart. These use a fixed FNV
//! hasher: the same inserts give the same order, every time, everywhere.

use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default, Clone, Copy)]
pub struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf29ce484222325 } else { self.0 };
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        self.0 = h;
    }
}

pub type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<Fnv>>;
pub type HashSet<K> = std::collections::HashSet<K, BuildHasherDefault<Fnv>>;
