use crate::{
    config::AppConfig,
    model::{
        AlertLevel, ContractKind, Exchange, FundingInfo, FundingMap, Instrument, Opportunity,
        Ticker, TickerMap,
    },
};
use chrono::Utc;
use std::collections::{BTreeSet, HashMap};

pub struct StrategyEngine {
    config: AppConfig,
}

impl StrategyEngine {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub fn common_symbols(&self, instruments: &[Instrument]) -> BTreeSet<(String, ContractKind)> {
        let mut seen: HashMap<(String, ContractKind), BTreeSet<&'static str>> = HashMap::new();
        for instrument in instruments {
            let base = instrument.base.to_uppercase();
            if self.config.is_blacklisted(&base) {
                continue;
            }
            seen.entry((base, instrument.kind))
                .or_default()
                .insert(instrument.exchange.as_str());
        }
        seen.into_iter()
            .filter_map(|(key, exchanges)| {
                if exchanges.contains("Binance") && exchanges.contains("OKX") {
                    Some(key)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn find_current_spreads(
        &self,
        instruments: &[Instrument],
        tickers: &[Ticker],
        funding: &[FundingInfo],
    ) -> Vec<Opportunity> {
        self.find_spreads(instruments, tickers, funding, false)
    }

    pub fn meets_open_threshold(&self, opportunity: &Opportunity) -> bool {
        opportunity.gross_spread_bps >= self.config.strategy.spread.min_gross_spread_bps as f64
            && opportunity.estimated_net_spread_bps
                >= self.config.strategy.spread.min_estimated_net_spread_bps as f64
    }

    pub fn meets_close_threshold(&self, opportunity: &Opportunity) -> bool {
        opportunity.estimated_net_spread_bps <= self.config.strategy.spread.close_spread_bps as f64
    }

    pub fn apply_risk_flags(&self, opportunities: &mut [Opportunity], funding: &[FundingInfo]) {
        let funding_map: FundingMap = funding
            .iter()
            .map(|item| {
                (
                    (item.base.to_uppercase(), item.kind, item.exchange),
                    item.clone(),
                )
            })
            .collect();
        for opportunity in opportunities {
            opportunity.risk_flags =
                self.risk_flags(&opportunity.base, opportunity.kind, &funding_map);
        }
    }

    fn find_spreads(
        &self,
        instruments: &[Instrument],
        tickers: &[Ticker],
        funding: &[FundingInfo],
        enforce_threshold: bool,
    ) -> Vec<Opportunity> {
        let common = self.common_symbols(instruments);
        let ticker_map: TickerMap = tickers
            .iter()
            .map(|ticker| {
                (
                    (ticker.base.to_uppercase(), ticker.kind, ticker.exchange),
                    ticker.clone(),
                )
            })
            .collect();
        let funding_map: FundingMap = funding
            .iter()
            .map(|item| {
                (
                    (item.base.to_uppercase(), item.kind, item.exchange),
                    item.clone(),
                )
            })
            .collect();

        let mut output = Vec::new();
        for (base, kind) in common {
            let Some(binance) = ticker_map.get(&(base.clone(), kind, Exchange::Binance)) else {
                continue;
            };
            let Some(okx) = ticker_map.get(&(base.clone(), kind, Exchange::Okx)) else {
                continue;
            };
            if let Some(opp) =
                self.compare_pair(&base, kind, okx, binance, &funding_map, enforce_threshold)
            {
                output.push(opp);
            }
        }
        output.sort_by(|a, b| {
            b.estimated_net_spread_bps
                .partial_cmp(&a.estimated_net_spread_bps)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        output
    }

    fn compare_pair(
        &self,
        base: &str,
        kind: ContractKind,
        okx: &Ticker,
        binance: &Ticker,
        funding: &FundingMap,
        enforce_threshold: bool,
    ) -> Option<Opportunity> {
        let okx_expensive_bps = spread_bps(okx.bid, binance.ask)?;
        let binance_expensive_bps = spread_bps(binance.bid, okx.ask)?;

        let (short, long, gross_spread_bps, min_side_depth_usdt) =
            if okx_expensive_bps >= binance_expensive_bps {
                (
                    okx,
                    binance,
                    okx_expensive_bps,
                    okx.bid_depth_usdt().min(binance.ask_depth_usdt()),
                )
            } else {
                (
                    binance,
                    okx,
                    binance_expensive_bps,
                    binance.bid_depth_usdt().min(okx.ask_depth_usdt()),
                )
            };

        if enforce_threshold
            && gross_spread_bps < self.config.strategy.spread.min_gross_spread_bps as f64
        {
            return None;
        }
        if gross_spread_bps > self.config.strategy.spread.max_gross_spread_bps as f64 {
            tracing::debug!(
                base,
                kind = %kind.as_str(),
                gross_spread_bps,
                "filtered suspicious spread above max_gross_spread_bps"
            );
            return None;
        }

        let estimated_net_spread_bps = gross_spread_bps
            - self.config.strategy.spread.round_trip_fee_bps as f64
            - self.config.strategy.spread.max_slippage_bps as f64;

        if enforce_threshold
            && estimated_net_spread_bps
                < self.config.strategy.spread.min_estimated_net_spread_bps as f64
        {
            return None;
        }

        let suggested_notional_usdt = self.suggest_notional(min_side_depth_usdt);
        let alert_level = alert_level(estimated_net_spread_bps, suggested_notional_usdt);
        let risk_flags = self.risk_flags(base, kind, funding);
        let now = Utc::now();
        Some(Opportunity {
            id: format!("{}-{}-{}", base, kind.as_str(), short.exchange.as_str()),
            base: base.to_string(),
            kind,
            long_exchange: long.exchange,
            long_symbol: long.exchange_symbol.clone(),
            long_price: long.ask,
            short_exchange: short.exchange,
            short_symbol: short.exchange_symbol.clone(),
            short_price: short.bid,
            gross_spread_bps,
            estimated_net_spread_bps,
            min_side_depth_usdt,
            suggested_notional_usdt,
            alert_level,
            risk_flags,
            created_at: now,
            updated_at: now,
            close_alert_sent: false,
        })
    }

    fn suggest_notional(&self, min_side_depth_usdt: f64) -> f64 {
        let sizing = &self.config.strategy.position_sizing;
        if !sizing.enabled || min_side_depth_usdt < sizing.watch_only_below_depth_usdt {
            return 0.0;
        }
        let tier = if min_side_depth_usdt >= sizing.full_size_min_depth_usdt {
            sizing.target_notional_usdt
        } else if min_side_depth_usdt >= sizing.half_size_min_depth_usdt {
            sizing.target_notional_usdt / 2.0
        } else if min_side_depth_usdt >= sizing.quarter_size_min_depth_usdt {
            sizing.target_notional_usdt / 4.0
        } else {
            0.0
        };
        tier.min(min_side_depth_usdt * sizing.max_depth_usage_ratio)
    }

    fn risk_flags(&self, base: &str, kind: ContractKind, funding: &FundingMap) -> Vec<String> {
        let mut flags = Vec::new();
        let binance = funding.get(&(base.to_string(), kind, Exchange::Binance));
        let okx = funding.get(&(base.to_string(), kind, Exchange::Okx));
        let risk = &self.config.risk.funding;

        for item in [binance, okx].into_iter().flatten() {
            if let Some(rate) = item.rate_bps {
                if rate.abs() >= risk.abnormal_abs_rate_bps as f64 {
                    flags.push(format!(
                        "{} 资金费率异常：{:.2} bps",
                        item.exchange.as_zh(),
                        rate
                    ));
                }
            }
            if let Some(next) = item.next_funding_time {
                let mins = (next - Utc::now()).num_minutes();
                if mins >= 0 && mins <= risk.near_settlement_minutes {
                    flags.push(format!(
                        "{} 距离资金费率结算约 {} 分钟",
                        item.exchange.as_zh(),
                        mins
                    ));
                }
            }
        }

        if let (Some(left), Some(right)) = (binance, okx) {
            if let (Some(left_rate), Some(right_rate)) = (left.rate_bps, right.rate_bps) {
                if (left_rate - right_rate).abs() >= risk.abnormal_diff_bps as f64 {
                    flags.push(format!(
                        "两边资金费率差异异常：币安 {:.2} bps，OKX {:.2} bps",
                        left_rate, right_rate
                    ));
                }
            }
        }
        flags
    }
}

fn spread_bps(expensive_bid: f64, cheap_ask: f64) -> Option<f64> {
    if expensive_bid <= 0.0 || cheap_ask <= 0.0 {
        return None;
    }
    Some((expensive_bid / cheap_ask - 1.0) * 10_000.0)
}

fn alert_level(net_bps: f64, notional: f64) -> AlertLevel {
    if notional <= 0.0 {
        AlertLevel::WatchOnly
    } else if net_bps >= 800.0 {
        AlertLevel::Extreme
    } else if net_bps >= 500.0 {
        AlertLevel::High
    } else {
        AlertLevel::Normal
    }
}
