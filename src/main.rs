mod data_loader;
mod hybrid;
mod pattern_miner;
mod pattern_scanner;
mod search;
mod spot;
mod strategies;
mod types;

use clap::{Parser, Subcommand};
use rand::prelude::*;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "bt", about = "Hybrid backtest engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run hybrid parameter search (BB spot + VS futures)
    Search {
        #[arg(long, default_value = "2000")] trials: usize,
        #[arg(long, default_value = "524")] symbols: usize,
        #[arg(long)] output: Option<PathBuf>,
        #[arg(long, default_value = "1.0")] vs_ratio_min: f64,
        #[arg(long, default_value = "10.0")] vs_ratio_max: f64,
        #[arg(long)] bb_tp: Option<f64>,
        #[arg(long)] bb_exhausted: Option<i32>,
        #[arg(long)] bb_period: Option<usize>,
        #[arg(long)] bb_std: Option<f64>,
        #[arg(long)] bb_hours: Option<usize>,
        #[arg(long)] bb_hlw: Option<usize>,
        #[arg(long)] bb_hlm: Option<usize>,
        #[arg(long)] bb_gain: Option<f64>,
        #[arg(long)] vs_fixed_ratio: Option<f64>,
        #[arg(long)] vs_fixed_gain: Option<f64>,
        #[arg(long, default_value = "0.5")] vs_gain_min: f64,
        #[arg(long, default_value = "9.9")] vs_gain_max: f64,
        #[arg(long, default_value = "2.0")] vs_sl_pct: f64,
        #[arg(long, default_value = "4")] vs_max_daily_tp: i32,
        #[arg(long)] vs_body_ratio: Option<f64>,
        #[arg(long)] split_ratio: Option<f64>,
        #[arg(long)] skip_ratio: Option<f64>,
        #[arg(long, default_value = "data_cache")] cache_dir: String,
    },
    /// Mine K-line patterns before 50%+ surges
    MinePatterns {
        #[arg(long, default_value = "524")] symbols: usize,
        #[arg(long, default_value = "50.0")] min_gain_pct: f64,
        #[arg(long, default_value = "120")] window_bars: usize,
        #[arg(long, default_value = "20")] seg_size: usize,
        #[arg(long)] output: Option<PathBuf>,
    },
    /// Validate cluster patterns: scan klines, measure precision/recall
    ScanPatterns {
        #[arg(long, default_value = "524")] symbols: usize,
        #[arg(long, default_value = "/tmp/patterns_v2_clusters.json")] cluster_file: String,
        #[arg(long, default_value = "60")] window_bars: usize,
        #[arg(long, default_value = "0.7")] threshold: f64,
        #[arg(long, default_value = "480")] forward_bars: usize,
        #[arg(long, default_value = "50.0")] min_gain_pct: f64,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cache_dir = PathBuf::from("data_cache");

    match cli.command {
        Commands::Search { trials, symbols, output, vs_ratio_min, vs_ratio_max,
                          bb_tp, bb_exhausted, split_ratio, skip_ratio, vs_body_ratio, cache_dir: cdir,
                          bb_period, bb_std, bb_hours, bb_hlw, bb_hlm, bb_gain,
                          vs_fixed_ratio, vs_fixed_gain, vs_gain_min, vs_gain_max,
                          vs_sl_pct, vs_max_daily_tp } => {
            let cache_dir = PathBuf::from(&cdir);
            let (k15_all, _, _) = data_loader::load_from_cache(&cache_dir, &["15m", "1h", "1d"])?;
            let all_syms: Vec<String> = k15_all.keys().cloned().collect();
            let mut rng = rand::thread_rng();
            let mut syms = all_syms;
            syms.shuffle(&mut rng);
            syms.truncate(symbols.min(syms.len()));

            let results = search::hybrid_search(
                &cache_dir, &syms, trials,
                vs_ratio_min, vs_ratio_max,
                bb_tp, bb_exhausted,
                split_ratio, skip_ratio, vs_body_ratio,
                bb_period, bb_std, bb_hours, bb_hlw, bb_hlm, bb_gain,
                vs_fixed_ratio, vs_fixed_gain,
                vs_gain_min, vs_gain_max,
                vs_sl_pct, vs_max_daily_tp,
            )?;
            search::print_hybrid_top(&results, 10);
            search::print_hybrid_stats(&results);
            if let Some(out) = output {
                std::fs::write(&out, serde_json::to_string_pretty(&results)?)?;
                println!("Results saved to {}", out.display());
            }
        }
        Commands::MinePatterns { symbols, min_gain_pct, window_bars, seg_size, output } => {
            let (k15_all, _, k1d_all) = data_loader::load_from_cache(&cache_dir, &["15m", "1h", "1d"])?;
            let all_syms: Vec<String> = k15_all.keys().cloned().collect();
            let mut rng = rand::thread_rng();
            let mut syms = all_syms;
            syms.shuffle(&mut rng);
            syms.truncate(symbols.min(syms.len()));

            let k15 = data_loader::filter_symbols(&k15_all, &syms, 17);
            let k1d = data_loader::filter_symbols(&k1d_all, &syms, 30);

            let (nevents, features) = pattern_miner::mine_patterns(
                &k1d, &k15, min_gain_pct, window_bars, seg_size);

            if let Some(out) = output {
                std::fs::write(&out, serde_json::to_string_pretty(&features)?)?;
                println!("Features saved to {}", out.display());
            }
            if !features.is_empty() {
                let dim = features[0].pre_window_features.len();
                println!("Feature dim={}, seg_size={}, segments={}", dim, seg_size, window_bars/seg_size);
                for f in features.iter().take(3) {
                    println!("  {} gain={:.0}% feat[0..9]={:?}",
                        f.symbol, f.gain_pct,
                        &f.pre_window_features[..9.min(f.pre_window_features.len())]);
                }
            }
            println!("Total: {} events, {} features", nevents, features.len());
        }
        Commands::ScanPatterns { symbols, cluster_file, window_bars, threshold, forward_bars, min_gain_pct } => {
            let (k15_all, _, _) = data_loader::load_from_cache(&cache_dir, &["15m", "1h", "1d"])?;
            let all_syms: Vec<String> = k15_all.keys().cloned().collect();
            let mut rng = rand::thread_rng();
            let mut syms = all_syms;
            syms.shuffle(&mut rng);
            syms.truncate(symbols.min(syms.len()));
            let k15 = data_loader::filter_symbols(&k15_all, &syms, 17);

            let results = pattern_scanner::scan_and_validate(
                &k15, &cluster_file, window_bars, threshold, forward_bars, min_gain_pct);

            println!("\n{:=<70}", "");
            println!("  Pattern Scanner Validation Results");
            println!("{:=<70}", "");
            println!("  {:>5} {:>10} {:>10} {:>10} {:>10} {:>15}",
                "Clus", "Precision", "Recall", "Matches", "HitGain", "AvgGainAfter");
            println!("  {:-<65}", "");
            for r in &results {
                println!("  {:>5} {:>9.1}% {:>9.1}% {:>10} {:>10} {:>14.1}%",
                    r.cluster_id, r.precision, r.recall, r.total_matches,
                    r.matches_before_gain, r.avg_gain_after_match);
            }
            if !results.is_empty() {
                let best = results.iter().max_by(|a,b| a.precision.partial_cmp(&b.precision).unwrap()).unwrap();
                println!("  {:-<65}", "");
                println!("  Best cluster: {} (precision={:.1}%, recall={:.1}%)",
                    best.cluster_id, best.precision, best.recall);
            }
        }
    }
    Ok(())
}
