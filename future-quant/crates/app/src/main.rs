mod alert;
mod audit;
mod config;
mod exchange;
mod model;
mod report;
mod state;
mod strategy;
mod web;

use crate::{
    alert::AlertManager,
    audit::AuditManager,
    config::AppConfig,
    exchange::{binance::BinanceClient, okx::OkxClient},
    model::{Exchange, Instrument, MarketSnapshot},
    state::SharedState,
    strategy::StrategyEngine,
};
use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Client;
use std::{path::PathBuf, time::Instant};
use tokio::time::{sleep, Duration};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let config_path = parse_config_path();
    let config = AppConfig::load(&config_path)?;
    report::cleanup_old_logs(&config)?;

    tracing::info!(
        mode = %config.runtime.mode,
        environment = %config.runtime.environment,
        "starting Future Quant"
    );

    let http = Client::builder()
        .user_agent("future-quant/0.1")
        .timeout(Duration::from_secs(15))
        .build()
        .context("failed to build HTTP client")?;

    let binance = BinanceClient::new(http.clone());
    let okx = OkxClient::new(http.clone());
    let strategy = StrategyEngine::new(config.clone());
    let state = SharedState::new(config.symbols.blacklist.iter().cloned().collect::<Vec<_>>());
    let alerts = AlertManager::new(config.clone(), http.clone());
    let audit = AuditManager::new(config.lightning_audit.clone(), http, state.clone());
    alerts.start_telegram_callbacks(state.clone());

    if config.web.enabled {
        let web_state = state.clone();
        let listen_addr = config.web.listen_addr.clone();
        let ui = config.ui.clone();
        let audit_manager = audit.clone();
        tokio::spawn(async move {
            if let Err(err) = web::serve(web_state, listen_addr, ui, audit_manager).await {
                tracing::error!(error = ?err, "web console stopped");
            }
        });
    }

    poll_loop(config, binance, okx, strategy, state, alerts).await
}

async fn poll_loop(
    config: AppConfig,
    binance: BinanceClient,
    okx: OkxClient,
    strategy: StrategyEngine,
    state: SharedState,
    alerts: AlertManager,
) -> Result<()> {
    let interval = Duration::from_secs(config.runtime.poll_interval_secs.max(1));
    loop {
        let started = Instant::now();
        match poll_once(&binance, &okx, &strategy, &config).await {
            Ok((mut snapshot, common_count)) => {
                let mut opportunities =
                    strategy.find_current_spreads(&snapshot.instruments, &snapshot.tickers, &[]);
                if !opportunities.is_empty() {
                    let funding_targets = opportunities
                        .iter()
                        .filter(|opp| strategy.meets_open_threshold(opp))
                        .cloned()
                        .collect::<Vec<_>>();
                    snapshot.funding = fetch_funding_for_opportunities(
                        &binance,
                        &okx,
                        &snapshot.instruments,
                        &funding_targets,
                    )
                    .await;
                    strategy.apply_risk_flags(&mut opportunities, &snapshot.funding);
                }
                let poll_ms = started.elapsed().as_millis();
                let (new_alerts, close_alerts) = state
                    .update_poll(
                        opportunities,
                        common_count,
                        poll_ms,
                        |opp| strategy.meets_open_threshold(opp),
                        |opp| strategy.meets_close_threshold(opp),
                    )
                    .await;

                for opportunity in new_alerts {
                    tracing::info!(
                        symbol = %opportunity.base,
                        kind = %opportunity.kind.as_str(),
                        net = opportunity.estimated_net_spread_bps,
                        "new opportunity"
                    );
                    alerts.opportunity_opened(&opportunity).await;
                }
                for opportunity in close_alerts {
                    tracing::info!(
                        symbol = %opportunity.base,
                        kind = %opportunity.kind.as_str(),
                        "opportunity reverted"
                    );
                    alerts.opportunity_closed(&opportunity).await;
                }

                if let Err(err) = report::maybe_write_daily_report(&config, &state).await {
                    tracing::warn!(error = ?err, "daily report failed");
                }
            }
            Err(err) => {
                tracing::warn!(error = ?err, "poll failed");
                state.mark_error(err.to_string()).await;
            }
        }
        sleep(interval).await;
    }
}

async fn poll_once(
    binance: &BinanceClient,
    okx: &OkxClient,
    strategy: &StrategyEngine,
    config: &AppConfig,
) -> Result<(MarketSnapshot, usize)> {
    let (binance_instruments, okx_instruments, binance_tickers, okx_tickers) = tokio::try_join!(
        binance.instruments(),
        okx.instruments(),
        binance.tickers(),
        okx.tickers()
    )?;

    let mut instruments = Vec::new();
    instruments.extend(filter_enabled_instruments(
        binance_instruments,
        config,
        Exchange::Binance,
    ));
    instruments.extend(filter_enabled_instruments(
        okx_instruments,
        config,
        Exchange::Okx,
    ));

    let common = strategy.common_symbols(&instruments);
    let mut tickers = Vec::new();
    tickers.extend(binance_tickers.into_iter().filter(|ticker| {
        common.contains(&(ticker.base.to_uppercase(), ticker.kind))
            && !config.is_blacklisted(&ticker.base)
    }));
    tickers.extend(okx_tickers.into_iter().filter(|ticker| {
        common.contains(&(ticker.base.to_uppercase(), ticker.kind))
            && !config.is_blacklisted(&ticker.base)
    }));

    let common_count = common.len();
    Ok((
        MarketSnapshot {
            instruments,
            tickers,
            funding: Vec::new(),
            updated_at: Utc::now(),
        },
        common_count,
    ))
}

async fn fetch_funding_for_opportunities(
    binance: &BinanceClient,
    okx: &OkxClient,
    instruments: &[Instrument],
    opportunities: &[crate::model::Opportunity],
) -> Vec<crate::model::FundingInfo> {
    let wanted: std::collections::HashSet<(String, crate::model::ContractKind)> = opportunities
        .iter()
        .map(|opp| (opp.base.to_uppercase(), opp.kind))
        .collect();
    let okx_instruments = instruments
        .iter()
        .filter(|item| {
            item.exchange == Exchange::Okx
                && wanted.contains(&(item.base.to_uppercase(), item.kind))
        })
        .cloned()
        .collect::<Vec<_>>();

    let (binance_funding, okx_funding) =
        tokio::join!(binance.funding(), okx.funding_for(&okx_instruments));
    let mut output = Vec::new();
    output.extend(binance_funding.unwrap_or_default());
    output.extend(okx_funding);
    output
}

fn filter_enabled_instruments(
    instruments: Vec<Instrument>,
    config: &AppConfig,
    exchange: Exchange,
) -> Vec<Instrument> {
    let enabled = match exchange {
        Exchange::Binance => &config.exchanges.binance,
        Exchange::Okx => &config.exchanges.okx,
    };
    if !enabled.enabled {
        return Vec::new();
    }
    instruments
        .into_iter()
        .filter(|item| {
            let key = match (exchange, item.kind) {
                (Exchange::Binance, crate::model::ContractKind::UsdtPerp) => "usdm_perp",
                (Exchange::Binance, crate::model::ContractKind::CoinPerp) => "coinm_perp",
                (Exchange::Okx, crate::model::ContractKind::UsdtPerp) => "usdt_swap",
                (Exchange::Okx, crate::model::ContractKind::CoinPerp) => "coin_swap",
            };
            enabled.market_types.iter().any(|kind| kind == key)
                && !config.is_blacklisted(&item.base)
        })
        .collect()
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn parse_config_path() -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            if let Some(path) = args.next() {
                return PathBuf::from(path);
            }
        }
    }
    PathBuf::from("config/local.toml")
}
