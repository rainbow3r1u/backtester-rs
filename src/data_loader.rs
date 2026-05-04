use crate::types::{Kline, KlinesBySymbol};
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
struct CacheFile {
    klines: HashMap<String, Vec<Kline>>,
}

pub fn load_from_cache(cache_dir: &Path, intervals: &[&str]) -> Result<(KlinesBySymbol, KlinesBySymbol, KlinesBySymbol)> {
    let mut k15 = HashMap::new();
    let mut k1h = HashMap::new();
    let mut k1d = HashMap::new();

    for interval in intervals {
        let path = cache_dir.join(format!("notusdt_{}.json", interval));
        if !path.exists() {
            continue;
        }
        let raw = fs::read_to_string(&path)?;
        let cache: CacheFile = serde_json::from_str(&raw)?;
        match *interval {
            "15m" => k15 = cache.klines,
            "1h" => k1h = cache.klines,
            "1d" => k1d = cache.klines,
            _ => {}
        }
    }
    Ok((k15, k1h, k1d))
}

pub fn filter_symbols(data: &KlinesBySymbol, symbols: &[String], min_klines: usize) -> KlinesBySymbol {
    let sym_set: std::collections::HashSet<&str> = symbols.iter().map(|s| s.as_str()).collect();
    let mut filtered = HashMap::new();
    for (sym, klines) in data {
        if sym_set.contains(sym.as_str()) && klines.len() >= min_klines {
            filtered.insert(sym.clone(), klines.clone());
        }
    }
    filtered
}
