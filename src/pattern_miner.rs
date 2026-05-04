use crate::types::{Kline, KlinesBySymbol};
use rand::prelude::*;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PatternFeatures {
    pub symbol: String,
    pub gain_pct: f64,
    pub is_gainer: bool,      // true=gainer, false=control
    pub pre_window_features: Vec<f64>,
    pub window_bars: usize,
}

/// Find coins with >= min_gain_pct gain. Returns (symbol, bottom_ts)
pub fn find_gainers(k1d: &KlinesBySymbol, min_gain_pct: f64) -> Vec<(String, i64)> {
    let mut events = Vec::new();
    for (sym, daily) in k1d {
        if daily.len() < 30 { continue; }
        let closes: Vec<f64> = daily.iter().map(|k| k.c).collect();
        let n = closes.len();
        for i in 0..n.saturating_sub(5) {
            let end = (i + 30).min(n);
            if end - i < 5 { continue; }
            let mut min_c = f64::MAX; let mut min_j = i;
            for j in i..end {
                if closes[j] < min_c { min_c = closes[j]; min_j = j; }
            }
            let mut max_c = min_c;
            for j in min_j..end { if closes[j] > max_c { max_c = closes[j]; } }
            let gain = (max_c - min_c) / min_c * 100.0;
            if gain >= min_gain_pct {
                let ma20: f64 = closes[i..end].iter().sum::<f64>() / (end - i) as f64;
                let ma_start: f64 = closes[i..(i+5).min(end)].iter().sum::<f64>() / 5.0;
                if ma_start > 0.0 && ma20 > ma_start * 0.95 {
                    events.push((sym.clone(), daily[min_j].t));
                }
                break;
            }
        }
    }
    events
}

/// Bar group features: statistics over a group of N bars
#[derive(Debug, Clone, Default)]
pub struct SegFeat {
    pub mean_ret: f64,
    pub ret_std: f64,
    pub vol_ratio: f64,
    pub body_ratio: f64,
    pub upper_shadow: f64,
    pub lower_shadow: f64,
    pub up_ratio: f64,
    pub max_dd: f64,
    pub close_loc: f64,
    pub vol_price_corr: f64,
    pub consec_up: f64,
    pub consec_down: f64,
    pub range_ratio: f64,
    pub range_trend: f64,
    pub ret_skew: f64,
}

impl SegFeat {
    pub fn from_bars(bars: &[Kline], prev_range: f64) -> Self {
        let n = bars.len();
        if n < 2 { return Self::default(); }

        let rets: Vec<f64> = bars.windows(2).map(|w| {
            if w[0].c > 0.0 { (w[1].c - w[0].c) / w[0].c } else { 0.0 }
        }).collect();
        let mean_ret = rets.iter().sum::<f64>() / rets.len() as f64;
        let ret_std = if rets.len() > 1 {
            (rets.iter().map(|r| (r-mean_ret).powi(2)).sum::<f64>() / rets.len() as f64).sqrt()
        } else { 0.0 };
        let ret_skew = if ret_std > 0.0 && rets.len() > 2 {
            rets.iter().map(|r| ((r-mean_ret)/ret_std).powi(3)).sum::<f64>() / rets.len() as f64
        } else { 0.0 };

        let total_q: f64 = bars.iter().map(|k| k.q).sum();
        let avg_q = total_q / n as f64;
        let last_q = bars.last().map(|k| k.q).unwrap_or(0.0);
        let vol_ratio = if avg_q > 0.0 { last_q / avg_q } else { 1.0 };

        let mut body_sum = 0.0; let mut up_shadow_sum = 0.0; let mut low_shadow_sum = 0.0;
        let mut up_count = 0; let mut range_sum = 0.0;
        for k in bars {
            let range = k.h - k.l;
            if range > 0.0 {
                body_sum += (k.c - k.o).abs() / range;
                up_shadow_sum += (k.h - k.c.max(k.o)) / range;
                low_shadow_sum += (k.c.min(k.o) - k.l) / range;
            }
            if k.c > k.o { up_count += 1; }
            range_sum += range / k.c;
        }
        let body_ratio = body_sum / n as f64;
        let upper_shadow = up_shadow_sum / n as f64;
        let lower_shadow = low_shadow_sum / n as f64;
        let up_ratio = up_count as f64 / n as f64;
        let range_ratio = range_sum / n as f64;
        let range_trend = if prev_range > 0.0 { range_ratio / prev_range } else { 1.0 };

        // Vol-price correlation: vol * sign(ret)
        let mut vol_price_sum = 0.0; let mut vol_sum = 0.0;
        for i in 1..bars.len() {
            let ret_sign = if bars[i].c > bars[i-1].c { 1.0 } else { -1.0 };
            vol_price_sum += bars[i].q * ret_sign;
            vol_sum += bars[i].q;
        }
        let vol_price_corr = if vol_sum > 0.0 { vol_price_sum / vol_sum } else { 0.0 };

        // Consecutive up/down (normalized by n)
        let mut max_consec_up = 0; let mut max_consec_down = 0;
        let mut cur_up = 0; let mut cur_down = 0;
        for k in bars {
            if k.c > k.o {
                cur_up += 1; cur_down = 0;
                max_consec_up = max_consec_up.max(cur_up);
            } else if k.c < k.o {
                cur_down += 1; cur_up = 0;
                max_consec_down = max_consec_down.max(cur_down);
            } else { cur_up = 0; cur_down = 0; }
        }
        let consec_up = max_consec_up as f64 / n as f64;
        let consec_down = max_consec_down as f64 / n as f64;

        let last = &bars[n-1];
        let hi = bars.iter().map(|k| k.h).fold(f64::MIN, f64::max);
        let lo = bars.iter().map(|k| k.l).fold(f64::MAX, f64::min);
        let close_loc = if hi > lo { (last.c - lo) / (hi - lo) } else { 0.5 };

        let mut peak = bars[0].c; let mut max_dd = 0.0;
        for k in bars {
            if k.c > peak { peak = k.c; }
            let dd = (peak - k.c) / peak;
            if dd > max_dd { max_dd = dd; }
        }

        Self { mean_ret, ret_std, vol_ratio, body_ratio, upper_shadow, lower_shadow,
               up_ratio, max_dd, close_loc, vol_price_corr, consec_up, consec_down,
               range_ratio, range_trend, ret_skew }
    }

    pub fn to_vec(&self) -> Vec<f64> {
        vec![self.mean_ret, self.ret_std, self.vol_ratio,
             self.body_ratio, self.upper_shadow, self.lower_shadow,
             self.up_ratio, self.max_dd, self.close_loc,
             self.vol_price_corr, self.consec_up, self.consec_down,
             self.range_ratio, self.range_trend, self.ret_skew]
    }
}

/// Extract pre-move features for gainers AND random control windows
pub fn extract_features(
    k15: &KlinesBySymbol, events: &[(String, i64)],
    window_bars: usize, seg_size: usize, control_ratio: f64,
) -> Vec<PatternFeatures> {
    let mut results = Vec::new();

    // Gainer windows
    for (sym, bottom_ts) in events {
        let kl = match k15.get(sym) { Some(k) => k, None => continue };
        let bottom_idx = match kl.iter().position(|k| k.t == *bottom_ts) {
            Some(i) => i, None => continue,
        };
        if bottom_idx < window_bars { continue; }
        let pre_bars = &kl[bottom_idx - window_bars..bottom_idx];
        let entry_price = kl[bottom_idx].c;
        let peak = kl[bottom_idx..].iter().map(|k| k.h).fold(0.0f64, f64::max);
        let gain_pct = if entry_price > 0.0 { (peak - entry_price) / entry_price * 100.0 } else { 0.0 };

        let n_seg = window_bars / seg_size;
        let mut prev_range = 0.0;
        let mut feats = Vec::new();
        for s in 0..n_seg {
            let seg_start = s * seg_size;
            let seg_end = (seg_start + seg_size).min(pre_bars.len());
            let sf = SegFeat::from_bars(&pre_bars[seg_start..seg_end], prev_range);
            prev_range = sf.range_ratio;
            feats.extend(sf.to_vec());
        }
        results.push(PatternFeatures {
            symbol: sym.clone(), gain_pct, is_gainer: true,
            pre_window_features: feats, window_bars,
        });
    }

    // Control windows: random non-gainer periods (after gainers collected)
    let n_gainers = results.len();
    let mut rng = rand::thread_rng();
    let n_control = (n_gainers as f64 * control_ratio) as usize;
    let mut syms: Vec<_> = k15.keys().collect();
    syms.shuffle(&mut rng);

    for sym in syms.iter() {
        if results.len() - n_gainers >= n_control { break; }
        let kl = match k15.get(*sym) { Some(k) => k, None => continue };
        if kl.len() < window_bars + 100 { continue; }
        for _ in 0..30 {
            let idx = rng.gen_range(window_bars..kl.len().saturating_sub(100));
            let future_peak = kl[idx..(idx+100).min(kl.len())]
                .iter().map(|k| k.h).fold(0.0f64, f64::max);
            let gain = (future_peak - kl[idx].c) / kl[idx].c * 100.0;
            if gain < 30.0 {
                let pre_bars = &kl[idx-window_bars..idx];
                let n_seg = window_bars / seg_size;
                let mut prev_range = 0.0;
                let mut feats = Vec::new();
                for s in 0..n_seg {
                    let start = s * seg_size;
                    let end = (start + seg_size).min(pre_bars.len());
                    let sf = SegFeat::from_bars(&pre_bars[start..end], prev_range);
                    prev_range = sf.range_ratio;
                    feats.extend(sf.to_vec());
                }
                let entry_price = kl[idx].c;
                let peak = kl[idx..(idx+100).min(kl.len())].iter().map(|k| k.h).fold(0.0f64, f64::max);
                let ctrl_gain = if entry_price > 0.0 { (peak - entry_price) / entry_price * 100.0 } else { 0.0 };
                results.push(PatternFeatures {
                    symbol: sym.to_string(), gain_pct: ctrl_gain, is_gainer: false,
                    pre_window_features: feats, window_bars,
                });
                break;
            }
        }
    }
    results
}

pub fn mine_patterns(
    k1d: &KlinesBySymbol, k15: &KlinesBySymbol,
    min_gain_pct: f64, window_bars: usize, seg_size: usize,
) -> (usize, Vec<PatternFeatures>) {
    println!("Step 1: 扫描涨幅≥{:.0}%的币种...", min_gain_pct);
    let events = find_gainers(k1d, min_gain_pct);
    println!("  发现 {} 个暴涨事件", events.len());

    println!("Step 2: 提取K线特征 (窗口={}根, seg={}根)...", window_bars, seg_size);
    let features = extract_features(k15, &events, window_bars, seg_size, 1.0);
    let gainers = features.iter().filter(|f| f.is_gainer).count();
    let controls = features.iter().filter(|f| !f.is_gainer).count();
    let dim = if features.is_empty() { 0 } else { features[0].pre_window_features.len() };
    println!("  有效: {} gainers + {} controls, dim={}", gainers, controls, dim);

    (events.len(), features)
}
