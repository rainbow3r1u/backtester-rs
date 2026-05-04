use crate::strategies::bb_climb;
use crate::types::Kline;
use std::collections::HashMap;

pub struct SpotSimulator {
    pub bal: f64,
    pub positions: Vec<SpotPos>,
    pub trade_log: Vec<SpotTrade>,
    max_pos: usize,
    per_trade: f64,
    fee: f64,
    tp_multiplier: f64,
}

#[derive(Debug, Clone)]
pub struct SpotPos {
    pub symbol: String,
    pub entry_price: f64,
    pub quantity: f64,
    pub exhausted: bool, // VS has TP'd 5+ times on this
}

#[derive(Debug, Clone)]
pub struct SpotTrade {
    pub action: String,
    pub symbol: String,
}

impl SpotSimulator {
    pub fn new(bal: f64, max_pos: usize, per_trade: f64, fee: f64, tp_multiplier: f64) -> Self {
        Self { bal, positions: vec![], trade_log: vec![], max_pos, per_trade, fee, tp_multiplier }
    }

    pub fn holds(&self, sym: &str) -> bool {
        self.positions.iter().any(|p| p.symbol == sym)
    }

    pub fn is_exhausted(&self, sym: &str) -> bool {
        self.positions.iter().any(|p| p.symbol == sym && p.exhausted)
    }

    pub fn mark_exhausted(&mut self, sym: &str) {
        for p in &mut self.positions {
            if p.symbol == sym { p.exhausted = true; return; }
        }
    }

    pub fn all_exhausted(&self) -> bool {
        !self.positions.is_empty() && self.positions.iter().all(|p| p.exhausted)
    }

    pub fn exhausted_count(&self) -> usize {
        self.positions.iter().filter(|p| p.exhausted).count()
    }

    pub fn buy(&mut self, sym: &str, price: f64, _k_idx: usize) {
        if self.positions.len() >= self.max_pos { return; }
        if self.holds(sym) { return; }
        if self.bal < self.per_trade { return; }

        let qty = self.per_trade / price * (1.0 - self.fee);
        self.bal -= self.per_trade;
        self.positions.push(SpotPos {
            symbol: sym.to_string(), entry_price: price,
            quantity: qty, exhausted: false,
        });
        self.trade_log.push(SpotTrade {
            action: "BUY".into(), symbol: sym.to_string(),
        });
    }

    pub fn sell(&mut self, pi: usize, price: f64, _k_idx: usize, reason: &str) {
        let pos = &self.positions[pi];
        let amount = pos.quantity * price * (1.0 - self.fee);
        self.bal += amount;
        self.trade_log.push(SpotTrade {
            action: format!("SELL_{}", reason), symbol: pos.symbol.clone(),
        });
        self.positions.remove(pi);
    }

    pub fn sell_all(&mut self, price_map: &HashMap<String, f64>, k_idx: usize) {
        let mut i = 0;
        while i < self.positions.len() {
            let sym = self.positions[i].symbol.clone();
            if let Some(&price) = price_map.get(&sym) {
                self.sell(i, price, k_idx, "ALL_EXHAUSTED");
            } else {
                i += 1;
            }
        }
    }

    /// Check BB positions for TP (100%) and SL (below lower band)
    pub fn check_positions(&mut self, klines: &HashMap<String, Vec<Kline>>,
                           bb_period: usize, bb_std: f64,
                           ts: i64, k_idx: usize) {
        let mut to_close: Vec<(usize, String, f64)> = vec![];
        for (pi, pos) in self.positions.iter().enumerate() {
            // Find the current price from klines
            let price = if let Some(kl) = klines.get(&pos.symbol) {
                kl.iter().filter(|k| k.t <= ts).last().map(|k| k.c)
            } else { None };
            let price = match price { Some(p) => p, None => continue };

            // TP: price * tp_multiplier
            if price >= pos.entry_price * self.tp_multiplier {
                to_close.push((pi, "TP".into(), price));
                continue;
            }

            // SL: below lower BB
            if let Some(kl) = klines.get(&pos.symbol) {
                let closes: Vec<f64> = kl.iter().map(|k| k.c).collect();
                let (_, _, lowers) = bb_climb::compute_rolling_bb(&closes, bb_period, bb_std);
                if !lowers.is_empty() && price < *lowers.last().unwrap() {
                    to_close.push((pi, "SL_BB".into(), price));
                }
            }
        }
        to_close.sort_by(|a,b| b.0.cmp(&a.0));
        for (pi, reason, price) in to_close {
            self.sell(pi, price, k_idx, &reason);
        }
    }
}
