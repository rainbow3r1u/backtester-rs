# 当前实盘策略参数

**更新时间：2026-04-28**
**数据基础：524个合约币种，60天历史K线，6000轮随机搜索**

---

## 策略配置

| 策略 | 状态 | 优先级 | 保证金 |
|------|------|--------|--------|
| VOL_SURGE | ✅ 启用 | 第一 | 20 USDT |
| SURGE | ❌ 禁用 | - | - |
| BB_CLIMB | ✅ 启用 | 第二 | 5 USDT |

---

## 核心参数（回测最优）

| 参数 | 默认值 | 当前值 | 说明 |
|------|--------|--------|------|
| VOL_SURGE_MIN_RATIO | 3.0 | **4.0** | 15分钟成交量突增倍数门槛 |
| VOL_SURGE_MIN_AVG_VOL | 5000 | **5000** | 前16根15m均量下限(USDT) |
| TAKE_PROFIT_PCT | 50% | **50%** | 止盈线（保证金百分比） |
| MIN_STOP_LOSS_PCT | 2% | **1%** | 止损线（开仓价百分比） |
| STOP_DAILY_GAIN_PCT | 20% | **20%** | 日线涨幅超过此值不开仓 |
| VOLUME_24H_FILTER | 300万 | **100万** | 24小时成交额最低门槛(USDT) |
| BB_CLIMB_MIN_HOURS | 2 | **3** | 布林爬坡最少连续小时数 |
| LEVERAGE | 10x | **10x** | 杠杆倍数 |
| MAX_POSITIONS | 5 | **5** | 最大同时持仓数 |
| INITIAL_CAPITAL | 100U | **100U** | 初始资金 |

---

## 新增风控规则

| 规则 | 说明 |
|------|------|
| 日止盈两次过滤 | 当日同币种止盈≥2次后，当日不再开仓 |
| 阴阳质检前置 | 开仓前检查现货/合约K线阴阳方向，不一致则不开 |
| 双阴过滤 | 最近3根1h K线中≥2根收阴，VOL_SURGE不开仓 |
| 日涨幅过滤 | 当日涨幅>20%的币种跳过 |
| 止损冷却 | 止损后同币种30分钟内不重复开仓 |
| 满仓替换 | 满仓5/5且VOL_SURGE ratio≥5.0时替换最弱非VS持仓 |
| VOL_SURGE不可替换 | VS策略持仓不能被替换 |
| 联合爆仓 | 总权益≤0时全部强平 |

---

## 排除币种

BTCUSDT, ETHUSDT, SOLUSDT, USDEUSDT 及所有稳定币对

---

## 回测验证结果（524币种，2000轮）

| 指标 | 最优值 |
|------|--------|
| Score | 81.4 |
| 60天收益率 | +336.5% |
| 最大回撤 | 88.7% |
| 交易次数 | 958笔 |
| 胜率 | 47.0% |
| 盈亏比 | 1.17 |
| 夏普比率 | 4.30 |

---

## 文件位置

- 交易脚本：`/home/myuser/websocket_new/sim_trade.py`
- 市场监控：`/home/myuser/websocket_new/market_monitor_app.py`
- 回测引擎(Python)：`/home/myuser/backtester/`
- 回测引擎(Rust)：`/home/myuser/backtester-rs/`
- COS数据管道：`/home/myuser/backtester/cos_service/`
- 本地缓存：`/home/myuser/backtester/data_cache/` (400MB)
- 搜索结果：`/home/myuser/backtester-rs/results/search_524_2k.json`
