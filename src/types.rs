use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Kline {
    pub t: i64,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub q: f64,
    #[serde(default)]
    pub v: f64,
}

#[derive(Debug, Clone)]
pub struct SharedData {
    pub symbols: Vec<String>,
    pub timestamps: Vec<i64>,
    pub ts_index: HashMap<String, HashMap<i64, usize>>,
    pub vol_24h_cache: HashMap<String, HashMap<i64, f64>>,
}

#[derive(Debug, Clone)]
pub struct Position {
    pub symbol: String,
    pub signal_type: String,
    pub entry_price: f64,
    pub quantity: f64,
    pub position_value: f64,
}

pub type KlinesBySymbol = HashMap<String, Vec<Kline>>;
