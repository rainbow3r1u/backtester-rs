use crate::pattern_miner::SegFeat;
use crate::types::{Kline, KlinesBySymbol};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct ClusterData {
    k: usize,
    cluster_centers: Vec<Vec<f64>>,
    feature_names: Vec<String>,
    cluster_avg_gains: Vec<f64>,
    cluster_sizes: Vec<usize>,
}

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub cluster_id: usize,
    pub precision: f64,       // % of matches that preceded >=50% gain
    pub recall: f64,          // % of gainers that had a match before
    pub total_matches: usize,
    pub total_gainers: usize,
    pub matched_gainers: usize,
    pub matches_before_gain: usize,
    pub avg_gain_after_match: f64,
}

/// Cosine similarity between two vectors
fn cosine_sim(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x,y)| x*y).sum();
    let na: f64 = a.iter().map(|x| x*x).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| x*x).sum::<f64>().sqrt();
    if na < 1e-10 || nb < 1e-10 { return 0.0; }
    dot / (na * nb)
}

/// Map Rust SegFeat to the same feature order as Python clustering
fn extract_feature_vec(bars: &[Kline], window_bars: usize) -> Vec<f64> {
    let seg_size = 15;
    let n_seg = window_bars / seg_size;
    let mut prev_range = 0.0;
    let mut feats = Vec::new();
    for s in 0..n_seg {
        let start = s * seg_size;
        let end = (start + seg_size).min(bars.len());
        let sf = SegFeat::from_bars(&bars[start..end], prev_range);
        prev_range = sf.range_ratio;
        feats.extend(sf.to_vec());
    }
    feats
}

/// Scan all coins for pattern matches and validate against actual gains
pub fn scan_and_validate(
    k15: &KlinesBySymbol,
    cluster_file: &str,
    window_bars: usize,
    threshold: f64,
    forward_bars: usize,  // look-ahead window to check for gains
    min_gain_pct: f64,
) -> Vec<ScanResult> {
    // Load cluster centers
    let raw = std::fs::read_to_string(cluster_file)
        .expect("Failed to read cluster file");
    let data: ClusterData = serde_json::from_str(&raw)
        .expect("Failed to parse cluster JSON");

    let k = data.k;
    let centers: Vec<Vec<f64>> = data.cluster_centers.iter().map(|c| {
        // Normalize center to unit length (for cosine similarity)
        let norm = (c.iter().map(|x| x*x).sum::<f64>()).sqrt();
        if norm > 0.0 { c.iter().map(|x| x/norm).collect() } else { c.clone() }
    }).collect();

    println!("Loaded {} cluster centers, dim={}", k, centers[0].len());
    println!("Scanning with threshold={}, forward={} bars ({} minutes)",
        threshold, forward_bars, forward_bars * 15);

    // Per-cluster stats
    let mut total_matches = vec![0usize; k];
    let mut matches_before_gain = vec![0usize; k];
    let mut total_gainers = 0usize;
    let mut matched_gainers = vec![0usize; k];
    let mut gain_after_match_sum = vec![0.0f64; k];
    let mut gainer_events: Vec<(String, usize)> = Vec::new(); // (symbol, bar_idx)

    // First pass: find all gainer events (ground truth)
    for (sym, kl) in k15 {
        if kl.len() < window_bars + forward_bars { continue; }
        for i in window_bars..kl.len().saturating_sub(forward_bars) {
            let entry = kl[i].c;
            if entry <= 0.0 { continue; }
            let future_peak = kl[i..(i+forward_bars).min(kl.len())]
                .iter().map(|k| k.h).fold(0.0, f64::max);
            let gain = (future_peak - entry) / entry * 100.0;
            if gain >= min_gain_pct {
                gainer_events.push((sym.clone(), i));
                total_gainers += 1;
            }
        }
    }
    println!("Ground truth: {} gainer events (≥{}% within {} bars)",
        total_gainers, min_gain_pct, forward_bars);

    // Second pass: scan for pattern matches
    for (sym, kl) in k15 {
        if kl.len() < window_bars + forward_bars { continue; }

        for i in window_bars..kl.len().saturating_sub(forward_bars) {
            let window = &kl[i - window_bars..i];
            let feats = extract_feature_vec(window, window_bars);

            // Normalize
            let norm = (feats.iter().map(|x| x*x).sum::<f64>()).sqrt();
            if norm < 1e-10 { continue; }
            let feats_norm: Vec<f64> = feats.iter().map(|x| x/norm).collect();

            // Check against each cluster center
            for cid in 0..k {
                let sim = cosine_sim(&feats_norm, &centers[cid]);
                if sim >= threshold {
                    total_matches[cid] += 1;

                    // Check if this match preceded a gain
                    let entry = kl[i].c;
                    let future_peak = kl[i..(i+forward_bars).min(kl.len())]
                        .iter().map(|k| k.h).fold(0.0, f64::max);
                    let gain = if entry > 0.0 { (future_peak - entry) / entry * 100.0 } else { 0.0 };
                    gain_after_match_sum[cid] += gain;

                    if gain >= min_gain_pct {
                        matches_before_gain[cid] += 1;
                    }
                    break; // Only count strongest match per window
                }
            }
        }

        // Check which gainer events had a preceding match
        for cid in 0..k {
            for (gsym, gidx) in &gainer_events {
                if gsym != sym { continue; }
                // Check if there was a match in the [gidx-window_bars, gidx] range
                let search_start = if *gidx >= window_bars * 2 { gidx - window_bars * 2 } else { 0 };
                for j in search_start..*gidx {
                    if j < window_bars { continue; }
                    let pre_window = &kl[j - window_bars..j];
                    let feats = extract_feature_vec(pre_window, window_bars);
                    let norm = (feats.iter().map(|x| x*x).sum::<f64>()).sqrt();
                    if norm < 1e-10 { continue; }
                    let feats_norm: Vec<f64> = feats.iter().map(|x| x/norm).collect();
                    let sim = cosine_sim(&feats_norm, &centers[cid]);
                    if sim >= threshold {
                        matched_gainers[cid] += 1;
                        break;
                    }
                }
            }
        }
    }

    // Compute precision/recall per cluster
    (0..k).map(|cid| {
        let precision = if total_matches[cid] > 0 {
            matches_before_gain[cid] as f64 / total_matches[cid] as f64 * 100.0
        } else { 0.0 };
        let recall = if total_gainers > 0 {
            matched_gainers[cid] as f64 / total_gainers as f64 * 100.0
        } else { 0.0 };
        let avg_gain = if total_matches[cid] > 0 {
            gain_after_match_sum[cid] / total_matches[cid] as f64
        } else { 0.0 };

        ScanResult {
            cluster_id: cid,
            precision,
            recall,
            total_matches: total_matches[cid],
            total_gainers,
            matched_gainers: matched_gainers[cid],
            matches_before_gain: matches_before_gain[cid],
            avg_gain_after_match: avg_gain,
        }
    }).collect()
}
