use crate::types::Kline;

pub fn detect_vol_surge(klines: &[Kline], min_ratio: f64, min_avg_vol: f64, min_gain_pct: f64, min_body_ratio: f64) -> Vec<(usize, f64, f64, f64)> {
    let mut signals = Vec::new();
    let n = klines.len();
    if n < 17 {
        return signals;
    }

    let q_values: Vec<f64> = klines.iter().map(|k| k.q).collect();
    let mut running_sum: f64 = q_values[0..16].iter().sum();

    for i in 16..n {
        let avg_q = running_sum / 16.0;

        if avg_q >= min_avg_vol && klines[i].q > 0.0 {
            let ratio = klines[i].q / avg_q;
            if ratio >= min_ratio {
                let gain = if klines[i].o > 0.0 {
                    (klines[i].c - klines[i].o) / klines[i].o * 100.0
                } else {
                    0.0
                };
                let body_ratio = if klines[i].h > klines[i].l {
                    (klines[i].c - klines[i].o) / (klines[i].h - klines[i].l)
                } else { 0.0 };
                if gain >= min_gain_pct && body_ratio >= min_body_ratio {
                    signals.push((i, ratio, gain, body_ratio));
                }
            }
        }
        running_sum = running_sum - q_values[i - 16] + q_values[i];
    }
    signals
}
