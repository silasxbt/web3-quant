#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::Deserialize;
use std::{collections::HashSet, fs, path::Path};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub runtime: RuntimeConfig,
    pub network: NetworkConfig,
    pub exchanges: ExchangesConfig,
    pub symbols: SymbolsConfig,
    pub strategy: StrategyConfig,
    pub risk: RiskConfig,
    pub storage: StorageConfig,
    pub alert: AlertConfig,
    pub web: WebConfig,
    pub ui: UiConfig,
    pub lightning_audit: LightningAuditConfig,
    pub system: SystemConfig,
    pub trading: TradingConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    pub mode: String,
    pub environment: String,
    pub poll_interval_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkConfig {
    pub proxy_mode: String,
    pub latency_warn_ms: u64,
    pub latency_critical_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExchangesConfig {
    pub binance: ExchangeConfig,
    pub okx: ExchangeConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExchangeConfig {
    pub enabled: bool,
    pub market_types: Vec<String>,
    pub auto_discover: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SymbolsConfig {
    pub mode: String,
    pub blacklist: HashSet<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StrategyConfig {
    pub spread: SpreadConfig,
    pub position_sizing: PositionSizingConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpreadConfig {
    pub enabled: bool,
    pub min_gross_spread_bps: i32,
    pub max_gross_spread_bps: i32,
    pub min_estimated_net_spread_bps: i32,
    pub max_slippage_bps: i32,
    pub round_trip_fee_bps: i32,
    pub default_notional_usdt: f64,
    pub close_spread_bps: i32,
    pub close_alert_once: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PositionSizingConfig {
    pub enabled: bool,
    pub depth_window_bps: i32,
    pub target_notional_usdt: f64,
    pub full_size_min_depth_usdt: f64,
    pub half_size_min_depth_usdt: f64,
    pub quarter_size_min_depth_usdt: f64,
    pub max_depth_usage_ratio: f64,
    pub watch_only_below_depth_usdt: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiskConfig {
    pub funding: FundingRiskConfig,
    pub exchange_restrictions: ExchangeRestrictionsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FundingRiskConfig {
    pub warn_only: bool,
    pub abnormal_abs_rate_bps: i32,
    pub abnormal_diff_bps: i32,
    pub near_settlement_minutes: i64,
    pub warn_on_period_change: bool,
    pub warn_on_cap_floor_change: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExchangeRestrictionsConfig {
    pub warn_on_order_limit_change: bool,
    pub warn_on_position_limit_change: bool,
    pub warn_on_reduce_only: bool,
    pub warn_on_open_disabled: bool,
    pub warn_on_trade_suspended: bool,
    pub warn_on_transfer_suspended: bool,
    pub warn_on_margin_rate_change: bool,
    pub warn_on_mark_price_abnormal: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    pub data_dir: String,
    pub reports_dir: String,
    pub log_retention_days: i64,
    pub daily_report_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AlertConfig {
    pub telegram: TelegramConfig,
    pub macos: MacosAlertConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub chat_id: String,
    pub bot_token_env: String,
    pub interactive: bool,
    pub manual_refresh_enabled: bool,
    pub auto_update_intervals_sec: Vec<u64>,
    pub close_alert_once: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MacosAlertConfig {
    pub enabled: bool,
    pub sound: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebConfig {
    pub enabled: bool,
    pub listen_addr: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UiConfig {
    pub language: String,
    pub module_explanations_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LightningAuditConfig {
    pub enabled: bool,
    pub config_path: String,
    pub openai_compatible_base_url: String,
    pub api_key_env: String,
    pub default_model: String,
    pub default_prompt_id: String,
    pub request_timeout_secs: u64,
    pub temperature: f32,
    pub max_tokens: u32,
    pub show_process_log: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemConfig {
    pub auto_start_on_boot: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TradingConfig {
    pub enabled: bool,
    pub allow_real_orders: bool,
    pub environment: String,
    pub planned_for_phase_2: bool,
    pub one_click_open_enabled: bool,
    pub require_second_confirmation: bool,
    pub safety: TradingSafetyConfig,
    pub binance: BinanceTradingCredentialConfig,
    pub okx: OkxTradingCredentialConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TradingSafetyConfig {
    pub max_notional_usdt: f64,
    pub max_unhedged_notional_usdt: f64,
    pub max_order_wait_ms: u64,
    pub kill_switch_file: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BinanceTradingCredentialConfig {
    pub api_key_env: String,
    pub api_secret_env: String,
    pub usdm_testnet_base_url: String,
    pub coinm_testnet_base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OkxTradingCredentialConfig {
    pub api_key_env: String,
    pub api_secret_env: String,
    pub passphrase_env: Option<String>,
    pub demo_trading: bool,
    pub demo_header: String,
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let config_path = if path.exists() {
            path.to_path_buf()
        } else {
            Path::new("config/local.example.toml").to_path_buf()
        };
        let raw = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read config {}", config_path.display()))?;
        let config = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config {}", config_path.display()))?;
        Ok(config)
    }

    pub fn is_blacklisted(&self, base: &str) -> bool {
        self.symbols.blacklist.contains(&base.to_uppercase())
    }
}
