use crate::spot::SpotSimulator;
use crate::strategies::{bb_climb, vol_surge};
use crate::types::*;
use std::collections::HashMap;
use std::sync::Arc;

const EXCLUDE: &[&str] = &[
    // 大盘/股票/商品
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "TSLAUSDT", "NVDAUSDT", "AMZNUSDT",
    "GOOGLUSDT", "AAPLUSDT", "COINUSDT", "MSTRUSDT", "METAUSDT", "TSMUSDT",
    "XAUUSDT", "XAGUSDT", "XAUTUSDT", "NATGASUSDT",
    // 稳定币对
    "USDCUSDT", "RLUSDUSDT", "UUSDT", "XUSDUSDT", "USD1USDT",
    "FDUSDUSDT", "TUSDUSDT", "PAXUSDT", "BUSDUSDT", "SUSDUSDT",
    "USDEUSDT", "USDPUSDT", "USDSUSDT", "AEURUSDT", "EURIUSDT", "EURUSDT",
    "BFUSDUSDT",
    // 现货专属（期货无此交易对）
    "ACMUSDT", "ADXUSDT", "ALCXUSDT", "AMPUSDT", "ARDRUSDT",
    "ATMUSDT", "AUDIOUSDT", "BARUSDT", "BNSOLUSDT",
    "BTTCUSDT", "CITYUSDT", "DCRUSDT", "DGBUSDT", "DODOUSDT",
    "FARMUSDT", "FTTUSDT", "GLMRUSDT", "GNOUSDT", "GNSUSDT",
    "IQUSDT", "JUVUSDT", "KGSTUSDT", "LAZIOUSDT", "LUNAUSDT",
    "MBLUSDT", "NEXOUSDT", "OSMOUSDT", "PIVXUSDT", "PONDUSDT",
    "PORTOUSDT", "PSGUSDT", "PYRUSDT", "QIUSDT", "QKCUSDT",
    "QUICKUSDT", "RADUSDT", "REQUSDT", "SCUSDT", "STRAXUSDT",
    "TFUELUSDT", "TKOUSDT", "WBETHUSDT", "WBTCUSDT", "WINUSDT",
    "XNOUSDT",
];

const FUT_STOP_DAILY_GAIN_PCT: f64 = 10.0;

#[derive(Debug, Clone, serde::Serialize)]
pub struct HybridResult {
    pub spot_final: f64,
    pub spot_return: f64,
    pub spot_trades: usize,
    pub spot_buy_count: usize,
    pub spot_symbols_count: usize,
    pub futures_final: f64,
    pub futures_return: f64,
    pub futures_trades: usize,
    pub futures_wr: f64,
    pub combined_return: f64,
    pub combined_dd: f64,
    pub bb_exhausted_count: usize,
    pub bb_params: BBParams,
    pub vs_params: VSParams,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BBParams {
    pub period: usize,
    pub std_mult: f64,
    pub min_hours: usize,
    pub hl_window: usize,
    pub hl_min: usize,
    pub vol_filter: i64,
    pub daily_gain_pct: f64,
    #[serde(default)]
    pub spot_tp_multiplier: f64,
    #[serde(default)]
    pub exhausted_threshold: i32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VSParams {
    pub min_ratio: f64,
    pub min_avg_vol: f64,
    pub margin: i32,
    pub tp_pct: i32,
    pub sl_pct: f64,
    pub vol_24h_filter: i64,
    pub max_daily_tp: i32,
    #[serde(default)]
    pub min_gain_pct: f64,
    #[serde(default)]
    pub min_body_ratio: f64,
}

impl Default for VSParams {
    fn default() -> Self {
        Self { min_ratio: 1.0, min_avg_vol: 5000.0, margin: 20, tp_pct: 50, sl_pct: 0.02, vol_24h_filter: 1_000_000, max_daily_tp: 4, min_gain_pct: 0.0, min_body_ratio: 0.0 }
    }
}

impl Default for BBParams {
    fn default() -> Self {
        Self { period: 20, std_mult: 2.5, min_hours: 4, hl_window: 5, hl_min: 3,
            vol_filter: 1_000_000, daily_gain_pct: 15.0, spot_tp_multiplier: 5.0, exhausted_threshold: 20 }
    }
}

pub fn run_hybrid(
    k15: Arc<KlinesBySymbol>, k1h: Arc<KlinesBySymbol>, k1d: Arc<KlinesBySymbol>,
    shared: Arc<SharedData>,
    bb: &BBParams, vs: &VSParams,
    spot_capital: f64, futures_capital: f64,
) -> HybridResult {
    let exclude: std::collections::HashSet<&str> = EXCLUDE.iter().copied().collect();
    let mut spot = SpotSimulator::new(spot_capital, 20, spot_capital / 20.0, 0.001, bb.spot_tp_multiplier);
    let mut fut_bal = futures_capital;
    let mut fut_pos: Vec<Position> = vec![];
    let mut fut_closed: Vec<(String, f64, String)> = vec![]; // (sym, pnl, sig_type)
    let mut tp_per_bb: HashMap<String, usize> = HashMap::new();
    let mut fut_cd: HashMap<String, usize> = HashMap::new(); // cooldown
    let mut fut_dtp: HashMap<(String, String), i32> = HashMap::new();
    let mut fut_eq: Vec<(usize, f64)> = vec![];
    let mut spot_entry_ts: HashMap<String, i64> = HashMap::new();

    // Pre-detect signals
    let mut bb_sigs: HashMap<String, Vec<usize>> = HashMap::new(); // symbol -> list of kline_indices
    let mut vs_sigs: HashMap<String, Vec<(usize, f64, f64, f64)>> = HashMap::new(); // (idx, ratio, gain, body_ratio)

    for sym in &shared.symbols {
        if exclude.contains(sym.as_str()) { continue; }
        if let Some(kl) = k1d.get(sym) {
            let det = bb_climb::detect_bb_climb(kl, bb.period, bb.std_mult, 14, true, 0.08, 1.2, bb.hl_window, bb.hl_min);
            let valid: Vec<usize> = det.into_iter().filter(|s| s.1 >= bb.min_hours).map(|s| s.0).collect();
            if !valid.is_empty() { bb_sigs.insert(sym.clone(), valid); }
        }
        if let Some(kl) = k15.get(sym) {
            let vs = vol_surge::detect_vol_surge(kl, vs.min_ratio, vs.min_avg_vol, vs.min_gain_pct, vs.min_body_ratio);
            if !vs.is_empty() {
                vs_sigs.insert(sym.clone(), vs);
            }
        }
    }

    let vol_24h = &shared.vol_24h_cache;

    for (ki, &ts) in shared.timestamps.iter().enumerate() {
        // --- SPOT: check positions, then buy signals ---
        // Find previous 1d timestamp (today's daily close is not known during intraday)
        let ts_1d_prev = k1d.values().next().and_then(|kl| {
            let mut iter = kl.iter().filter(|k| k.t <= ts).rev();
            iter.next()?; // skip today
            iter.next().map(|k| k.t)
        }).unwrap_or(0);

        spot.check_positions(&k1d, bb.period, bb.std_mult, ts_1d_prev, ki);

        // BB sell → VS sync close
        for _pi in (0..spot.positions.len()).rev() {
            // Check if any position was closed (we need to detect sells)
            // We use trade_log as signal: if BUY count > SELL count, position exists
            // Simpler: iterate BB sell reasons from trade_log at this ki
        }
        // Actually, check_positions already executed sells. Now sync VS:
        let spot_holds: std::collections::HashSet<String> = spot.positions.iter().map(|p| p.symbol.clone()).collect();
        let mut fut_to_close: Vec<usize> = vec![];
        for (pi, fp) in fut_pos.iter().enumerate() {
            if !spot_holds.contains(&fp.symbol) {
                fut_to_close.push(pi);
            }
        }
        fut_to_close.sort_by(|a,b| b.cmp(a));
        for pi in fut_to_close {
            let fp = &fut_pos[pi];
            let price = get_price(&shared, &k15, &fp.symbol, ts);
            let pnl = (price - fp.entry_price) * fp.quantity;
            let fee = fp.position_value * 0.0004 * 2.0;
            fut_bal += pnl - fee;
            fut_closed.push((fp.symbol.clone(), pnl - fee, fp.signal_type.clone()));
            fut_pos.remove(pi);
        }

        // BB buy signals (1d klines)
        let price_map = build_prices(&shared, &k15, ts);
        for sym in &shared.symbols {
            if let Some(indices) = bb_sigs.get(sym) {
                for &idx in indices {
                    if let Some(kl) = k1d.get(sym) {
                        if idx < kl.len() {
                            let sig_ts = kl[idx].t;
                            // Only open at next day's UTC 00:00 (first 15m candle after signal confirms)
                            if ts >= sig_ts + 86_400_000 && ts < sig_ts + 86_400_000 + 900_000 {
                                // Volume filter
                                let v24 = vol_24h.get(sym).and_then(|vc| vc.get(&ts)).copied().unwrap_or(0.0);
                                if v24 < bb.vol_filter as f64 { continue; }
                                // Two-day cumulative gain filter (signal day + prev day)
                                if !chk_two_day_gain(&k1d, sym, sig_ts, bb.daily_gain_pct) { continue; }
                                if let Some(&price) = price_map.get(sym) {
                                    spot.buy(sym, price, ki);
                                    spot_entry_ts.insert(sym.clone(), ts);
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        // --- FUTURES: check positions, liquidation, replace ---
        let mut ftc: Vec<(usize, String, f64, f64)> = vec![];
        let mut total_unrealized = 0.0;
        for (pi, fp) in fut_pos.iter().enumerate() {
            if let Some(&price) = price_map.get(&fp.symbol) {
                let pnl = (price - fp.entry_price) * fp.quantity;
                total_unrealized += pnl;
                let tp = fp.entry_price * (1.0 + vs.tp_pct as f64 / 100.0 / 10.0);
                let sl = fp.entry_price * (1.0 - vs.sl_pct);
                if price >= tp { ftc.push((pi, "TAKE_PROFIT".into(), price, pnl)); }
                else if price <= sl { ftc.push((pi, "STOP_LOSS".into(), price, pnl)); }
            }
        }

        // 联合爆仓
        if fut_bal + total_unrealized <= 0.0 && !fut_pos.is_empty() {
            for (pi, fp) in fut_pos.iter().enumerate() {
                let price = price_map.get(&fp.symbol).copied().unwrap_or(fp.entry_price * 0.5);
                let pnl = (price - fp.entry_price) * fp.quantity;
                if !ftc.iter().any(|(i,_,_,_)| *i == pi) {
                    ftc.push((pi, "LIQUIDATED_CROSS".into(), price, pnl));
                }
            }
        }

        ftc.sort_by(|a,b| b.0.cmp(&a.0));
        for (pi, reason, _price, pnl) in ftc {
            let fp = &fut_pos[pi];
            let fee = fp.position_value * 0.0004 * 2.0;
            let ap = pnl - fee;
            fut_bal += ap;
            let sym = fp.symbol.clone();
            if reason == "TAKE_PROFIT" {
                *tp_per_bb.entry(sym.clone()).or_insert(0) += 1;
                // 日止盈计数
                let date = bjd(ts);
                *fut_dtp.entry((sym.clone(), date)).or_insert(0) += 1;
                if tp_per_bb.get(&sym).copied().unwrap_or(0) >= bb.exhausted_threshold as usize {
                    spot.mark_exhausted(&sym);
                }
            }
            if reason == "STOP_LOSS" { fut_cd.insert(sym.clone(), ki + 2); }
            fut_closed.push((sym, ap, fp.signal_type.clone()));
            fut_pos.remove(pi);
        }

        // VS signals (with all filters)
        let bjd_now = bjd(ts);
        for sym in &shared.symbols {
            if fut_pos.len() >= 5 { break; }
            if fut_cd.get(sym).map_or(false, |&c| ki < c) { continue; }
            if !spot.holds(sym) { continue; }
            if spot.is_exhausted(sym) { continue; }
            if let Some(&price) = price_map.get(sym) {
                if price <= 0.0 { continue; }

                // 日涨幅过滤
                if !chk_gain(&k1d, sym, ts, FUT_STOP_DAILY_GAIN_PCT) { continue; }

                // 24h成交量过滤
                let v24 = vol_24h.get(sym).and_then(|vc| vc.get(&ts)).copied().unwrap_or(0.0);
                if v24 < vs.vol_24h_filter as f64 { continue; }

                // 日止盈次数过滤
                if fut_dtp.get(&(sym.clone(), bjd_now.clone())).copied().unwrap_or(0) >= vs.max_daily_tp { continue; }

                // 双阴过滤 (use previous fully closed 1h candle)
                let ts_prev_1h = (ts / 3_600_000) * 3_600_000 - 3_600_000;
                if chk_double_yin(&k1h, sym, ts_prev_1h) { continue; }

                if let Some(vl) = vs_sigs.get(sym) {
                    if let Some(kl) = k15.get(sym) {
                        for &(vi, ratio, _gain, _body) in vl {
                            if vi < kl.len() {
                                let sig_ts = kl[vi].t;
                                if ts - sig_ts > 0 && ts - sig_ts <= 900_000 {
                                    // VS signal must be AFTER spot entry
                                    if let Some(&entry_ts) = spot_entry_ts.get(sym) {
                                        if sig_ts < entry_ts { continue; }
                                    } else {
                                        continue;
                                    }
                                    let margin = vs.margin as f64;
                                    let pv = margin * 10.0;
                                    let qty = pv / price;
                                    fut_pos.push(Position {
                                        symbol: sym.clone(),
                                        signal_type: format!("VOL_SURGE_{:.1}x", ratio),
                                        entry_price: price, quantity: qty,
                                        position_value: pv,
                                    });
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Equity tracking
        let spot_eq = spot.bal + spot.positions.iter().map(|p| {
            price_map.get(&p.symbol).copied().unwrap_or(p.entry_price) * p.quantity
        }).sum::<f64>();
        let fut_eq_val = fut_bal + fut_pos.iter().map(|p| {
            (price_map.get(&p.symbol).copied().unwrap_or(p.entry_price) - p.entry_price) * p.quantity
        }).sum::<f64>();
        let total_eq = spot_eq + fut_eq_val;
        fut_eq.push((ki, total_eq));

        // All BB exhausted → sell all
        if spot.all_exhausted() {
            spot.sell_all(&price_map, ki);
        }
    }

    // Results
    let spot_final = spot.bal + spot.positions.iter().map(|p| {
        build_prices(&shared, &k15, *shared.timestamps.last().unwrap_or(&0)).get(&p.symbol).copied().unwrap_or(p.entry_price) * p.quantity
    }).sum::<f64>();
    let fut_final = fut_bal + fut_pos.iter().map(|p| {
        (build_prices(&shared, &k15, *shared.timestamps.last().unwrap_or(&0)).get(&p.symbol).copied().unwrap_or(p.entry_price) - p.entry_price) * p.quantity
    }).sum::<f64>();

    let spot_ret = (spot_final - spot_capital) / spot_capital * 100.0;
    let fut_ret = (fut_final - futures_capital) / futures_capital * 100.0;
    let combined_ret = (spot_final + fut_final - spot_capital - futures_capital) / (spot_capital + futures_capital) * 100.0;

    let fut_n = fut_closed.len();
    let fut_wins = fut_closed.iter().filter(|(_, pnl, _)| *pnl > 0.0).count();
    let fut_wr = if fut_n > 0 { fut_wins as f64 / fut_n as f64 * 100.0 } else { 0.0 };

    let mut peak = spot_capital + futures_capital;
    let mut max_dd = 0.0;
    for &(_, eq) in &fut_eq {
        if eq > peak { peak = eq; }
        let d = (peak - eq) / peak * 100.0;
        if d > max_dd { max_dd = d; }
    }

    let spot_buy_count = spot.trade_log.iter().filter(|t| t.action == "BUY").count();
    let spot_symbols_count: std::collections::HashSet<String> = spot.trade_log.iter().map(|t| t.symbol.clone()).collect();
    HybridResult {
        spot_final, spot_return: spot_ret,
        spot_trades: spot.trade_log.iter().filter(|t| t.action.starts_with("SELL")).count(),
        spot_buy_count,
        spot_symbols_count: spot_symbols_count.len(),
        futures_final: fut_final, futures_return: fut_ret,
        futures_trades: fut_n, futures_wr: fut_wr,
        combined_return: combined_ret, combined_dd: max_dd,
        bb_exhausted_count: spot.exhausted_count(),
        bb_params: bb.clone(), vs_params: vs.clone(),
    }
}

fn get_price(shared: &SharedData, k15: &KlinesBySymbol, sym: &str, ts: i64) -> f64 {
    shared.ts_index.get(sym).and_then(|tm| tm.get(&ts)).and_then(|&idx| {
        k15.get(sym).and_then(|kl| if idx < kl.len() { Some(kl[idx].c) } else { None })
    }).unwrap_or(0.0)
}

fn build_prices(shared: &SharedData, k15: &KlinesBySymbol, ts: i64) -> HashMap<String, f64> {
    let mut map = HashMap::new();
    for sym in &shared.symbols {
        if let Some(p) = (|| {
            let tm = shared.ts_index.get(sym)?;
            let &idx = tm.get(&ts)?;
            let kl = k15.get(sym)?;
            if idx < kl.len() { Some(kl[idx].c) } else { None }
        })() { map.insert(sym.clone(), p); }
    }
    map
}

fn chk_gain(k1d: &KlinesBySymbol, sym: &str, ts: i64, max_pct: f64) -> bool {
    if let Some(kd) = k1d.get(sym) {
        let mut iter = kd.iter().filter(|k| k.t <= ts).rev();
        iter.next(); // skip today (not known during intraday)
        if let Some(k) = iter.next() {
            if k.o > 0.0 { return (k.c - k.o) / k.o * 100.0 <= max_pct; }
        }
    }
    true
}

fn chk_two_day_gain(k1d: &KlinesBySymbol, sym: &str, ts: i64, max_pct: f64) -> bool {
    if let Some(kd) = k1d.get(sym) {
        let mut iter = kd.iter().filter(|k| k.t <= ts).rev();
        let today = iter.next();
        let yesterday = iter.next();
        if let (Some(t), Some(y)) = (today, yesterday) {
            if y.o > 0.0 {
                let total_gain = (t.c - y.o) / y.o * 100.0;
                return total_gain <= max_pct;
            }
        }
    }
    true
}

fn chk_double_yin(k1h: &KlinesBySymbol, sym: &str, ts: i64) -> bool {
    if let Some(kh) = k1h.get(sym) {
        let r: Vec<&Kline> = kh.iter().filter(|k| k.t <= ts).collect();
        if r.len() < 2 { return false; }
        let s = if r.len() >= 3 { r.len() - 3 } else { 0 };
        r[s..].iter().filter(|k| k.c < k.o).count() >= 2
    } else { false }
}

fn bjd(ts: i64) -> String {
    let d = ts / 1000 / 86400;
    let a = d + 719468i64; let era = if a>=0{a}else{a-146096}/146097;
    let doe = a-era*146097; let yoe = (doe-doe/1460+doe/36524-doe/146096)/365;
    let y = yoe+era*400; let doy = doe-(365*yoe+yoe/4-yoe/100);
    let mp = (5*doy+2)/153; let day = doy-(153*mp+2)/5+1;
    let m = if mp<10{mp+3}else{mp-9}; let y = if m<=2{y+1}else{y};
    format!("{:04}-{:02}-{:02}",y,m,day)
}
