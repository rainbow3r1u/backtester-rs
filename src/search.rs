use crate::data_loader;
use crate::hybrid::{self, BBParams, HybridResult, VSParams};
use crate::types::{KlinesBySymbol, SharedData};
use anyhow::Result;
use rand::prelude::*;
use rayon::prelude::*;
use std::path::Path;
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

pub fn precompute_shared(k15: &KlinesBySymbol) -> SharedData {
    let exclude: std::collections::HashSet<&str> = EXCLUDE.iter().copied().collect();
    let mut symbols = vec![];
    let mut all_ts = std::collections::BTreeSet::new();
    let mut ts_idx = std::collections::HashMap::new();
    let mut vc = std::collections::HashMap::new();
    for (sym, kl) in k15 {
        if exclude.contains(sym.as_str()) || kl.len() < 17 { continue; }
        symbols.push(sym.clone());
        let mut tm = std::collections::HashMap::new();
        for (i, k) in kl.iter().enumerate() { tm.insert(k.t, i); all_ts.insert(k.t); }
        ts_idx.insert(sym.clone(), tm);
        let mut vol = std::collections::HashMap::new();
        let mut s = 0.0f64; let mut t = 0usize; let cut = 24i64 * 3600 * 1000;
        for (h, k) in kl.iter().enumerate() {
            s += k.q;
            while h > t && kl[h].t - kl[t].t > cut { s -= kl[t].q; t += 1; }
            vol.insert(k.t, s);
        }
        vc.insert(sym.clone(), vol);
    }
    SharedData { symbols, timestamps: all_ts.into_iter().collect(), ts_index: ts_idx, vol_24h_cache: vc }
}

fn random_bb(rng: &mut impl Rng, bb_tp: Option<f64>, bb_exhausted: Option<i32>,
             bb_period: Option<usize>, bb_std: Option<f64>, bb_hours: Option<usize>,
             bb_hlw: Option<usize>, bb_hlm: Option<usize>, bb_gain: Option<f64>) -> BBParams {
    let search_all = bb_tp.is_some() && bb_period.is_none();
    BBParams {
        period: bb_period.unwrap_or_else(|| if search_all { *[20, 30].choose(rng).unwrap() } else { 30 }),
        std_mult: bb_std.unwrap_or_else(|| if search_all { *[2.0, 2.5].choose(rng).unwrap() } else { 2.5 }),
        min_hours: bb_hours.unwrap_or_else(|| if search_all { *[2, 4, 6, 8].choose(rng).unwrap() } else { 4 }),
        hl_window: bb_hlw.unwrap_or_else(|| if search_all { *[3, 5].choose(rng).unwrap() } else { 5 }),
        hl_min: bb_hlm.unwrap_or_else(|| if search_all { *[2, 3].choose(rng).unwrap() } else { 3 }),
        daily_gain_pct: bb_gain.unwrap_or_else(|| if search_all { *[5.0, 10.0, 15.0].choose(rng).unwrap() } else { 10.0 }),
        spot_tp_multiplier: bb_tp.unwrap_or_else(|| *[2.0, 5.0, 10.0, 99.0].choose(rng).unwrap()),
        exhausted_threshold: bb_exhausted.unwrap_or_else(|| *[5, 10, 15].choose(rng).unwrap()),
        vol_filter: 1_000_000,
    }
}

fn random_vs(rng: &mut impl Rng, ratio_min: f64, ratio_max: f64, body_override: Option<f64>,
             fixed_ratio: Option<f64>, fixed_gain: Option<f64>,
             gain_min: f64, gain_max: f64, sl_pct: f64, max_daily_tp: i32) -> VSParams {
    VSParams {
        min_ratio: fixed_ratio.unwrap_or_else(|| (rng.gen_range((ratio_min*10.0) as i32..(ratio_max*10.0) as i32) as f64) / 10.0),
        min_gain_pct: fixed_gain.unwrap_or_else(|| (rng.gen_range((gain_min*10.0) as i32..(gain_max*10.0) as i32) as f64) / 10.0),
        min_body_ratio: body_override.unwrap_or_else(|| (rng.gen_range(0..40) as f64) / 100.0),
        sl_pct,
        max_daily_tp,
        ..VSParams::default()
    }
}

pub fn hybrid_search(
    cache_dir: &Path, symbols: &[String], n_trials: usize,
    vs_ratio_min: f64, vs_ratio_max: f64,
    bb_tp: Option<f64>, bb_exhausted: Option<i32>,
    split_ratio: Option<f64>, skip_ratio: Option<f64>,
    vs_body_ratio: Option<f64>,
    bb_period: Option<usize>, bb_std: Option<f64>, bb_hours: Option<usize>,
    bb_hlw: Option<usize>, bb_hlm: Option<usize>, bb_gain: Option<f64>,
    vs_fixed_ratio: Option<f64>, vs_fixed_gain: Option<f64>,
    vs_gain_min: f64, vs_gain_max: f64,
    vs_sl_pct: f64, vs_max_daily_tp: i32,
) -> Result<Vec<HybridResult>> {
    let (k15_all, k1h_all, k1d_all) = data_loader::load_from_cache(cache_dir, &["15m", "1h", "1d"])?;
    let k15 = data_loader::filter_symbols(&k15_all, symbols, 17);
    let k1h = data_loader::filter_symbols(&k1h_all, symbols, 20);
    let k1d = data_loader::filter_symbols(&k1d_all, symbols, 1);
    let mut shared = precompute_shared(&k15);
    if let Some(ratio) = skip_ratio {
        let n = shared.timestamps.len();
        let skip = (n as f64 * ratio) as usize;
        shared.timestamps = shared.timestamps[skip..].to_vec();
        println!("Hybrid: {} symbols, {} trials, timeline {}-{} (last {:.0}%)",
            symbols.len(), n_trials, skip, n, (1.0-ratio)*100.0);
    } else if let Some(ratio) = split_ratio {
        let n = shared.timestamps.len();
        let split = (n as f64 * ratio) as usize;
        shared.timestamps.truncate(split);
        println!("Hybrid: {} symbols, {} trials, timeline 0-{} (first {:.0}%)",
            symbols.len(), n_trials, split, ratio*100.0);
    } else {
        println!("Hybrid: {} symbols, {} trials (parallel)", symbols.len(), n_trials);
    }
    let shared = Arc::new(shared);
    let k15a = Arc::new(k15);
    let k1ha = Arc::new(k1h);
    let k1da = Arc::new(k1d);
    let t0 = std::time::Instant::now();

    let results: Vec<HybridResult> = (0..n_trials).into_par_iter().map(|_| {
        let mut rng = rand::thread_rng();
        let bb = random_bb(&mut rng, bb_tp, bb_exhausted, bb_period, bb_std, bb_hours, bb_hlw, bb_hlm, bb_gain);
        let vs = random_vs(&mut rng, vs_ratio_min, vs_ratio_max, vs_body_ratio, vs_fixed_ratio, vs_fixed_gain, vs_gain_min, vs_gain_max, vs_sl_pct, vs_max_daily_tp);
        hybrid::run_hybrid(
            Arc::clone(&k15a), Arc::clone(&k1ha), Arc::clone(&k1da), Arc::clone(&shared), &bb, &vs,
            100.0, 500.0,  // BB 100 + VS 500
        )
    }).collect();

    let e = t0.elapsed();
    println!("Completed {} trials in {:.1}s ({:.1} t/s)", n_trials, e.as_secs_f64(), n_trials as f64 / e.as_secs_f64());
    Ok(results)
}

pub fn print_hybrid_top(results: &[HybridResult], n: usize) {
    let mut s: Vec<&HybridResult> = results.iter().collect();
    s.sort_by(|a,b| b.combined_return.partial_cmp(&a.combined_return).unwrap_or(std::cmp::Ordering::Equal));
    println!("\n=== TOP {} (by combined return) ===", n.min(s.len()));
    for (i, r) in s.iter().take(n).enumerate() {
        println!("#{} comb={:.1}% spot={:.1}% fut={:.1}% dd={:.1}% fut_tr={} wr={:.1}%",
            i+1, r.combined_return, r.spot_return, r.futures_return, r.combined_dd,
            r.futures_trades, r.futures_wr);
        println!("   BB: p={} std={:.1} h={} hlw={} hlm={} gain={:.0}% tp={:.0}x exh={}",
            r.bb_params.period, r.bb_params.std_mult, r.bb_params.min_hours,
            r.bb_params.hl_window, r.bb_params.hl_min, r.bb_params.daily_gain_pct,
            r.bb_params.spot_tp_multiplier, r.bb_params.exhausted_threshold);
        println!("   VS: ratio={:.1} margin={} tp={} sl={:.0}% max_dtp={} gain>={:.1}% body>={:.2}",
            r.vs_params.min_ratio, r.vs_params.margin, r.vs_params.tp_pct,
            r.vs_params.sl_pct * 100.0, r.vs_params.max_daily_tp, r.vs_params.min_gain_pct, r.vs_params.min_body_ratio);
    }
}

pub fn print_hybrid_stats(results: &[HybridResult]) {
    let n = results.len();
    if n == 0 { return; }

    let mut comb: Vec<f64> = results.iter().map(|r| r.combined_return).collect();
    let mut fut: Vec<f64> = results.iter().map(|r| r.futures_return).collect();
    let mut spot: Vec<f64> = results.iter().map(|r| r.spot_return).collect();
    let mut wr: Vec<f64> = results.iter().map(|r| r.futures_wr).collect();
    let mut tr: Vec<f64> = results.iter().map(|r| r.futures_trades as f64).collect();
    let mut dd: Vec<f64> = results.iter().map(|r| r.combined_dd).collect();

    comb.sort_by(|a,b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    fut.sort_by(|a,b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    spot.sort_by(|a,b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    wr.sort_by(|a,b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    tr.sort_by(|a,b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    dd.sort_by(|a,b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let pct = |v: &[f64], p: f64| {
        let idx = ((v.len() as f64 - 1.0) * p / 100.0) as usize;
        v[idx.min(v.len()-1)]
    };

    let fut_pos = fut.iter().filter(|&&x| x > 0.0).count();
    let comb_pos = comb.iter().filter(|&&x| x > 0.0).count();

    println!("\n=== FULL STATS ({} trials) ===", n);
    println!("{:>12} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}", "", "comb", "fut", "spot", "wr", "trades", "dd");
    println!("  {:<8}  {:>8.1}% {:>8.1}% {:>8.1}% {:>8.1}% {:>8.0} {:>8.1}%",
        "mean", mean(&comb), mean(&fut), mean(&spot), mean(&wr), mean(&tr), mean(&dd));
    println!("  {:<8}  {:>8.1}% {:>8.1}% {:>8.1}% {:>8.1}% {:>8.0} {:>8.1}%",
        "median", pct(&comb, 50.0), pct(&fut, 50.0), pct(&spot, 50.0), pct(&wr, 50.0), pct(&tr, 50.0), pct(&dd, 50.0));
    println!("  {:<8}  {:>8.1}% {:>8.1}% {:>8.1}% {:>8.1}% {:>8.0} {:>8.1}%",
        "p25", pct(&comb, 25.0), pct(&fut, 25.0), pct(&spot, 25.0), pct(&wr, 25.0), pct(&tr, 25.0), pct(&dd, 25.0));
    println!("  {:<8}  {:>8.1}% {:>8.1}% {:>8.1}% {:>8.1}% {:>8.0} {:>8.1}%",
        "p75", pct(&comb, 75.0), pct(&fut, 75.0), pct(&spot, 75.0), pct(&wr, 75.0), pct(&tr, 75.0), pct(&dd, 75.0));
    println!("  {:<8}  {:>8.1}% {:>8.1}% {:>8.1}% {:>8.1}% {:>8.0} {:>8.1}%",
        "best", comb[n-1], fut[n-1], spot[n-1], wr[n-1], tr[n-1], dd[n-1]);
    println!("  {:<8}  {:>8.1}% {:>8.1}% {:>8.1}% {:>8.1}% {:>8.0} {:>8.1}%",
        "worst", comb[0], fut[0], spot[0], wr[0], tr[0], dd[0]);
    println!("  >0 count {}/{}     {}/{}",
        comb_pos, n, fut_pos, n);
    println!("  >0 pct   {:.0}%        {:.0}%",
        comb_pos as f64 / n as f64 * 100.0, fut_pos as f64 / n as f64 * 100.0);
}
