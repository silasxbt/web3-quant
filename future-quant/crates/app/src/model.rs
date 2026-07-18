use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exchange {
    Binance,
    Okx,
}

impl Exchange {
    pub fn as_str(self) -> &'static str {
        match self {
            Exchange::Binance => "Binance",
            Exchange::Okx => "OKX",
        }
    }

    pub fn as_zh(self) -> &'static str {
        match self {
            Exchange::Binance => "币安",
            Exchange::Okx => "OKX",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ContractKind {
    UsdtPerp,
    CoinPerp,
}

impl ContractKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ContractKind::UsdtPerp => "USDT_PERP",
            ContractKind::CoinPerp => "COIN_PERP",
        }
    }

    pub fn as_zh(self) -> &'static str {
        match self {
            ContractKind::UsdtPerp => "U 本位永续",
            ContractKind::CoinPerp => "币本位永续",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instrument {
    pub exchange: Exchange,
    pub exchange_symbol: String,
    pub base: String,
    pub quote: String,
    pub kind: ContractKind,
    pub contract_size: f64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticker {
    pub exchange: Exchange,
    pub exchange_symbol: String,
    pub base: String,
    pub kind: ContractKind,
    pub bid: f64,
    pub ask: f64,
    pub bid_size: f64,
    pub ask_size: f64,
    pub bid_depth_usdt: f64,
    pub ask_depth_usdt: f64,
    pub ts: DateTime<Utc>,
}

impl Ticker {
    pub fn bid_depth_usdt(&self) -> f64 {
        self.bid_depth_usdt
    }

    pub fn ask_depth_usdt(&self) -> f64 {
        self.ask_depth_usdt
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingInfo {
    pub exchange: Exchange,
    pub exchange_symbol: String,
    pub base: String,
    pub kind: ContractKind,
    pub rate_bps: Option<f64>,
    pub next_funding_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSnapshot {
    pub instruments: Vec<Instrument>,
    pub tickers: Vec<Ticker>,
    pub funding: Vec<FundingInfo>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opportunity {
    pub id: String,
    pub base: String,
    pub kind: ContractKind,
    pub long_exchange: Exchange,
    pub long_symbol: String,
    pub long_price: f64,
    pub short_exchange: Exchange,
    pub short_symbol: String,
    pub short_price: f64,
    pub gross_spread_bps: f64,
    pub estimated_net_spread_bps: f64,
    pub min_side_depth_usdt: f64,
    pub suggested_notional_usdt: f64,
    pub alert_level: AlertLevel,
    pub risk_flags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub close_alert_sent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertLevel {
    WatchOnly,
    Normal,
    High,
    Extreme,
}

impl AlertLevel {
    pub fn as_zh(self) -> &'static str {
        match self {
            AlertLevel::WatchOnly => "仅观察",
            AlertLevel::Normal => "正常机会",
            AlertLevel::High => "高优先级",
            AlertLevel::Extreme => "极端机会",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStateSnapshot {
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub active_opportunities: Vec<Opportunity>,
    pub recent_opportunities: Vec<Opportunity>,
    pub blacklisted_symbols: Vec<String>,
    pub muted_symbols: Vec<String>,
    pub status: RuntimeStatus,
    pub stats: Stats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub binance_ok: bool,
    pub okx_ok: bool,
    pub last_error: Option<String>,
    pub last_poll_ms: Option<u128>,
    pub poll_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub discovered_common_symbols: usize,
    pub active_count: usize,
    pub recent_count: usize,
    pub last_report_path: Option<String>,
}

pub type TickerMap = HashMap<(String, ContractKind, Exchange), Ticker>;
pub type FundingMap = HashMap<(String, ContractKind, Exchange), FundingInfo>;
