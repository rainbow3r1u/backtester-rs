#!/usr/bin/env python3
"""
Phase 2: 聚类分析暴涨前K线形态
Input: patterns_phase1.json (from Rust pattern_miner)
Output: 聚类中心 + 各聚类形态特征描述
"""
import json, sys, numpy as np

FEATURE_NAMES = [
    "mean_ret", "ret_std", "vol_ratio",
    "body_ratio", "upper_shadow", "lower_shadow",
    "up_ratio", "max_dd", "close_loc",
    "vol_pr_cor", "consec_up", "consec_dn",
    "range_rat", "range_trd", "ret_skew"
]
SEG_LABELS = ["T-15h", "T-11h", "T-7h", "T-3h"]

def load_features(path):
    with open(path) as f:
        data = json.load(f)
    X = np.array([d['pre_window_features'] for d in data])
    gains = np.array([d['gain_pct'] for d in data])
    symbols = [d['symbol'] for d in data]
    return X, gains, symbols

def standardize(X):
    mean = X.mean(axis=0, keepdims=True)
    std = X.std(axis=0, keepdims=True)
    std[std == 0] = 1.0
    return (X - mean) / std, mean, std

def kmeans(X, k, max_iter=100):
    n = X.shape[0]
    # Init with k-means++
    idx = np.random.choice(n, 1)
    centroids = X[idx].copy()
    for _ in range(1, k):
        dist = np.min(np.linalg.norm(X[:, None] - centroids[None], axis=2), axis=1)
        prob = dist / dist.sum()
        new_idx = np.random.choice(n, 1, p=prob)
        centroids = np.vstack([centroids, X[new_idx]])

    for it in range(max_iter):
        dist = np.linalg.norm(X[:, None] - centroids[None], axis=2)
        labels = np.argmin(dist, axis=1)
        new_centroids = np.array([X[labels == i].mean(axis=0) for i in range(k)])
        if np.allclose(centroids, new_centroids, rtol=1e-4):
            break
        centroids = new_centroids
    return labels, centroids, it + 1

def cluster_stats(X_raw, labels, centroids_raw, gains, k, n_segments):
    """Analyze each cluster: raw features already in interpretable space"""
    n_features = len(FEATURE_NAMES)

    for c in range(k):
        mask = labels == c
        n = mask.sum()
        avg_gain = gains[mask].mean()
        max_gain = gains[mask].max()

        print(f"\n{'='*70}")
        print(f"  聚类 {c}: {n} 个样本  |  平均涨幅 {avg_gain:.0f}%  |  最高涨幅 {max_gain:.0f}%")
        print(f"{'='*70}")

        # centroids_raw is already in original feature space (mean of raw X per cluster)
        ctr_denorm = centroids_raw[c]

        # Print segment-by-segment
        print(f"  {'':>8}", end="")
        for seg in range(n_segments):
            print(f"  {SEG_LABELS[seg]:>12}", end="")
        print()

        for fi in range(n_features):
            print(f"  {FEATURE_NAMES[fi]:>8}", end="")
            for seg in range(n_segments):
                val = ctr_denorm[seg * n_features + fi]
                print(f"  {float(val):>12.4f}", end="")
            # Trend arrow
            vals = ctr_denorm[fi::n_features]
            trend = " ↗" if vals[-1] > vals[0] else " ↘" if vals[-1] < vals[0] else " →"
            print(trend)

        # Key findings
        print(f"\n  ▸ 形态特征:")
        # Find the last 2 segments for each feature
        last2 = ctr_denorm[-2*n_features:]
        prev2 = ctr_denorm[-4*n_features:-2*n_features]
        findings = []
        # Volume change
        vol_change = last2[2] / (prev2[2] + 0.001)
        if vol_change > 1.2:
            findings.append(f"临近启动时量能放大{float(vol_change):.1f}x")
        elif vol_change < 0.8:
            findings.append(f"启动前量能收缩至{float(vol_change):.1f}x")
        # Up ratio
        if float(last2[6]) > 0.6:
            findings.append(f"最后10h阳线占比{float(last2[6])*100:.0f}%")
        # Body ratio
        if float(last2[3]) > 0.5:
            findings.append("实体占比较大(趋势明确)")
        elif float(last2[3]) < 0.35:
            findings.append("小实体占主导(蓄力形态)")
        # Lower shadow in last segment
        if float(last2[5]) > 0.3:
            findings.append("下影线较长(买盘支撑)")
        # Volatility
        if float(last2[1]) > 0.003:
            findings.append("波动率升高")
        elif float(last2[1]) < 0.001:
            findings.append("低波动率(横盘蓄力)")

        if findings:
            for f in findings:
                print(f"    • {f}")

    # Summary
    print(f"\n{'='*70}")
    print(f"  聚类质量: {k} clusters, {len(X_raw)} samples")
    sizes = [(labels == i).sum() for i in range(k)]
    print(f"  聚类大小: {sizes}")
    # Silhouette-like score (simplified)
    print(f"  各聚类平均涨幅: {[f'{gains[labels==i].mean():.0f}%' for i in range(k)]}")


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else '/tmp/patterns_phase1.json'
    k = int(sys.argv[2]) if len(sys.argv) > 2 else 6

    print(f"加载 {path}...")
    X, gains, symbols = load_features(path)
    print(f"样本: {len(X)}  维度: {X.shape[1]}")

    # Standardize
    X_std, feat_mean, feat_std = standardize(X)

    # Determine n_segments from dimension
    n_features = len(FEATURE_NAMES)
    n_segments = X.shape[1] // n_features
    seg_bars = 60 // n_segments  # approximate

    # Run K-Means multiple times, pick best
    best_labels = None
    best_centroids = None
    best_inertia = float('inf')
    best_raw_centroids = None

    for run in range(10):
        labels, centroids, iters = kmeans(X_std, k)
        inertia = sum(np.linalg.norm(X_std[i] - centroids[labels[i]])**2 for i in range(len(X)))
        if inertia < best_inertia:
            best_inertia = inertia
            best_labels = labels
            best_centroids = centroids
            best_raw_centroids = np.array([X[labels == i].mean(axis=0) for i in range(k)])

    print(f"K-Means converged. Inertia: {best_inertia:.1f}")
    n_feat = len(FEATURE_NAMES)
    n_seg = X.shape[1] // n_feat
    cluster_stats(X, best_labels, best_raw_centroids, gains, k, n_seg)

    # Save results
    output = {
        'k': k,
        'n_samples': len(X),
        'feature_names': FEATURE_NAMES,
        'segment_labels': SEG_LABELS[:n_segments],
        'cluster_sizes': [(best_labels == i).sum().item() for i in range(k)],
        'cluster_avg_gains': [float(gains[best_labels == i].mean()) for i in range(k)],
        'cluster_centers': best_raw_centroids.tolist(),
        'labels': best_labels.tolist(),
        'gain_pcts': gains.tolist(),
        'symbols': symbols,
    }
    outpath = path.replace('.json', '_clusters.json')
    with open(outpath, 'w') as f:
        json.dump(output, f, indent=2)
    print(f"\n聚类结果保存到: {outpath}")


if __name__ == '__main__':
    main()
