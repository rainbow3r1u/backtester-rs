use crate::types::Kline;

pub fn compute_rolling_bb(closes: &[f64], period: usize, std_mult: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let mut mids = Vec::new();
    let mut uppers = Vec::new();
    let mut lowers = Vec::new();
    for i in (period - 1)..closes.len() {
        let window = &closes[i + 1 - period..=i];
        let mean = window.iter().sum::<f64>() / period as f64;
        let variance = window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / period as f64;
        let std = variance.sqrt();
        mids.push(mean);
        uppers.push(mean + std_mult * std);
        lowers.push(mean - std_mult * std);
    }
    (mids, uppers, lowers)
}

pub fn calculate_atr(klines: &[Kline], period: usize, idx: usize) -> f64 {
    let start = if idx + 1 >= period + 1 { idx + 1 - period } else { 0 };
    if start >= idx {
        return 0.0;
    }
    let mut tr_sum = 0.0;
    for i in start + 1..=idx {
        let k = &klines[i];
        let prev = &klines[i - 1];
        let tr = (k.h - k.l)
            .max((k.h - prev.c).abs())
            .max((k.l - prev.c).abs());
        tr_sum += tr;
    }
    tr_sum / (idx - start) as f64
}

fn check_hour_climb(
    k: &Kline, middle: f64, upper: f64, avg_vol: f64,
    upper_tol: f64, vol_ratio: f64, check_vol: bool,
) -> bool {
    if k.c <= middle {
        return false;
    }
    let tolerance = upper * upper_tol;
    if k.c < upper - tolerance || k.c > upper + tolerance {
        return false;
    }
    if check_vol && avg_vol > 0.0 && k.q < avg_vol * vol_ratio {
        return false;
    }
    true
}

fn check_hl_climb(klines: &[Kline], idx: usize, window: usize, min_count: usize) -> bool {
    let check_start = if idx >= window { idx + 1 - window } else { 0 };
    let mut climb_count = 0;
    for i in check_start.max(1)..=idx {
        if klines[i].h > klines[i - 1].h && klines[i].l > klines[i - 1].l {
            climb_count += 1;
        }
    }
    climb_count >= min_count
}

pub fn detect_bb_climb(
    klines: &[Kline],
    period: usize,
    std_mult: f64,
    atr_period: usize,
    atr_enabled: bool,
    upper_tolerance: f64,
    volume_ratio: f64,
    hl_window: usize,
    hl_min: usize,
) -> Vec<(usize, usize)> {
    let mut signals = Vec::new();
    let n = klines.len();
    let min_required = (period + 1).max(atr_period + 1);
    if n < min_required {
        return signals;
    }

    let closes: Vec<f64> = klines.iter().map(|k| k.c).collect();
    let (bb_mids, bb_uppers, _bb_lowers) = compute_rolling_bb(&closes, period, std_mult);
    if bb_mids.is_empty() {
        return signals;
    }

    let bb_idx = |kline_idx: usize| -> isize {
        if kline_idx + 1 >= period {
            (kline_idx + 1 - period) as isize
        } else {
            -1
        }
    };

    // Precompute 5d avg volumes (exclude current bar, matching website klines[-6:-1])
    let mut avg_vols = vec![0.0f64; n];
    for i in 0..n {
        let end = if i >= 1 { i - 1 } else { 0 };
        let start = if end >= 5 { end - 4 } else { 0 };
        let count = (end - start + 1) as f64;
        if count > 0.0 {
            let sum: f64 = klines[start..=end].iter().map(|k| k.q).sum();
            avg_vols[i] = sum / count;
        }
    }

    for i in min_required..n {
        let bi = bb_idx(i);
        if bi < 0 || bi >= bb_mids.len() as isize {
            continue;
        }
        let bi = bi as usize;
        let middle = bb_mids[bi];
        let upper = bb_uppers[bi];
        let avg_vol = avg_vols[i];

        // ④ c>mid + ⑤ ±8% upper (skip volume, checked last)
        if !check_hour_climb(&klines[i], middle, upper, avg_vol, upper_tolerance, volume_ratio, false) {
            continue;
        }
        // ⑦ HL climb
        if !check_hl_climb(klines, i, hl_window, hl_min) {
            continue;
        }
        // ⑧ ATR
        if atr_enabled {
            let atr = calculate_atr(klines, atr_period, i);
            if atr > 0.0 {
                let current_range = klines[i].h - klines[i].l;
                if current_range < atr * 0.5 {
                    continue;
                }
            } else {
                continue;
            }
        }

        // ⑨ Count consecutive (no volume check in scan)
        let mut consecutive = 1;
        for j in (0..i).rev() {
            let bj = bb_idx(j);
            if bj < 0 || bj >= bb_mids.len() as isize {
                break;
            }
            let bj = bj as usize;
            if !check_hour_climb(&klines[j], bb_mids[bj], bb_uppers[bj], avg_vols[j], upper_tolerance, volume_ratio, false) {
                break;
            }
            if !check_hl_climb(klines, j, hl_window, hl_min) {
                break;
            }
            consecutive += 1;
        }

        // ⑪ Volume: strict >1.2x or relaxed >0.3x + bullish
        let q_today = klines[i].q;
        let is_bullish = klines[i].c > klines[i].o;
        let vol_ok = if avg_vol > 0.0 {
            q_today >= avg_vol * volume_ratio  // strict: >1.2x
                || (q_today > avg_vol * 0.3 && is_bullish)  // relaxed: >0.3x + bullish
        } else {
            true
        };
        if !vol_ok {
            continue;
        }

        signals.push((i, consecutive));
    }
    signals
}
