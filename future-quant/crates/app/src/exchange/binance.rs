use crate::model::{ContractKind, Exchange, FundingInfo, Instrument, Ticker};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone)]
pub struct BinanceClient {
    http: Client,
}

impl BinanceClient {
    pub fn new(http: Client) -> Self {
        Self { http }
    }

    pub async fn instruments(&self) -> Result<Vec<Instrument>> {
        let (usdm, coinm) =
            tokio::try_join!(self.usdm_exchange_info(), self.coinm_exchange_info(),)?;
        let mut instruments = Vec::new();
        instruments.extend(parse_usdm_instruments(usdm));
        instruments.extend(parse_coinm_instruments(coinm));
        Ok(instruments)
    }

    pub async fn tickers(&self) -> Result<Vec<Ticker>> {
        let (usdm_info, coinm_info, usdm, coinm) = tokio::try_join!(
            self.usdm_exchange_info(),
            self.coinm_exchange_info(),
            self.usdm_tickers(),
            self.coinm_tickers()
        )?;
        let usdm_contract_sizes = contract_size_map(&usdm_info);
        let coinm_contract_sizes = contract_size_map(&coinm_info);
        let now = Utc::now();
        let mut tickers = Vec::new();
        tickers.extend(
            usdm.into_iter()
                .filter_map(|raw| parse_usdm_ticker(raw, now, &usdm_contract_sizes)),
        );
        tickers.extend(
            coinm
                .into_iter()
                .filter_map(|raw| parse_coinm_ticker(raw, now, &coinm_contract_sizes)),
        );
        Ok(tickers)
    }

    pub async fn funding(&self) -> Result<Vec<FundingInfo>> {
        let usdm = self.usdm_premium_index().await.unwrap_or_default();
        let coinm = self.coinm_premium_index().await.unwrap_or_default();
        let mut funding = Vec::new();
        funding.extend(usdm.into_iter().filter_map(parse_usdm_funding));
        funding.extend(coinm.into_iter().filter_map(parse_coinm_funding));
        Ok(funding)
    }

    async fn usdm_exchange_info(&self) -> Result<BinanceExchangeInfo> {
        self.http
            .get("https://fapi.binance.com/fapi/v1/exchangeInfo")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("failed to parse Binance USD-M exchangeInfo")
    }

    async fn coinm_exchange_info(&self) -> Result<BinanceExchangeInfo> {
        self.http
            .get("https://dapi.binance.com/dapi/v1/exchangeInfo")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("failed to parse Binance COIN-M exchangeInfo")
    }

    async fn usdm_tickers(&self) -> Result<Vec<BinanceTicker>> {
        self.http
            .get("https://fapi.binance.com/fapi/v1/ticker/bookTicker")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("failed to parse Binance USD-M bookTicker")
    }

    async fn coinm_tickers(&self) -> Result<Vec<BinanceTicker>> {
        self.http
            .get("https://dapi.binance.com/dapi/v1/ticker/bookTicker")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("failed to parse Binance COIN-M bookTicker")
    }

    async fn usdm_premium_index(&self) -> Result<Vec<BinancePremiumIndex>> {
        let value: serde_json::Value = self
            .http
            .get("https://fapi.binance.com/fapi/v1/premiumIndex")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        parse_binance_premium_value(value).context("failed to parse Binance USD-M premiumIndex")
    }

    async fn coinm_premium_index(&self) -> Result<Vec<BinancePremiumIndex>> {
        let value: serde_json::Value = self
            .http
            .get("https://dapi.binance.com/dapi/v1/premiumIndex")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        parse_binance_premium_value(value).context("failed to parse Binance COIN-M premiumIndex")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinanceExchangeInfo {
    symbols: Vec<BinanceSymbol>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinanceSymbol {
    symbol: String,
    pair: Option<String>,
    base_asset: Option<String>,
    quote_asset: Option<String>,
    contract_type: Option<String>,
    status: Option<String>,
    contract_status: Option<String>,
    contract_size: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinanceTicker {
    symbol: String,
    bid_price: String,
    bid_qty: String,
    ask_price: String,
    ask_qty: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinancePremiumIndex {
    symbol: String,
    pair: Option<String>,
    last_funding_rate: Option<String>,
    next_funding_time: Option<i64>,
}

fn parse_usdm_instruments(raw: BinanceExchangeInfo) -> Vec<Instrument> {
    raw.symbols
        .into_iter()
        .filter(|s| s.contract_type.as_deref() == Some("PERPETUAL"))
        .filter(|s| s.status.as_deref() == Some("TRADING"))
        .filter_map(|s| {
            Some(Instrument {
                exchange: Exchange::Binance,
                exchange_symbol: s.symbol,
                base: s.base_asset?,
                quote: s.quote_asset.unwrap_or_else(|| "USDT".to_string()),
                kind: ContractKind::UsdtPerp,
                contract_size: s.contract_size.unwrap_or(1.0),
                status: "TRADING".to_string(),
            })
        })
        .collect()
}

fn parse_coinm_instruments(raw: BinanceExchangeInfo) -> Vec<Instrument> {
    raw.symbols
        .into_iter()
        .filter(|s| s.contract_type.as_deref() == Some("PERPETUAL"))
        .filter(|s| s.contract_status.as_deref() == Some("TRADING"))
        .filter_map(|s| {
            let base = s
                .pair
                .as_deref()
                .and_then(|pair| pair.strip_suffix("USD"))
                .map(str::to_string)
                .or(s.base_asset);
            Some(Instrument {
                exchange: Exchange::Binance,
                exchange_symbol: s.symbol,
                base: base?,
                quote: "USD".to_string(),
                kind: ContractKind::CoinPerp,
                contract_size: s.contract_size.unwrap_or(1.0),
                status: "TRADING".to_string(),
            })
        })
        .collect()
}

fn parse_usdm_ticker(
    raw: BinanceTicker,
    ts: DateTime<Utc>,
    contract_sizes: &HashMap<String, f64>,
) -> Option<Ticker> {
    let base = raw.symbol.strip_suffix("USDT")?.to_string();
    let bid: f64 = raw.bid_price.parse().ok()?;
    let ask: f64 = raw.ask_price.parse().ok()?;
    let bid_size: f64 = raw.bid_qty.parse().ok()?;
    let ask_size: f64 = raw.ask_qty.parse().ok()?;
    let contract_size = *contract_sizes.get(&raw.symbol).unwrap_or(&1.0);
    Some(Ticker {
        exchange: Exchange::Binance,
        exchange_symbol: raw.symbol,
        base,
        kind: ContractKind::UsdtPerp,
        bid,
        ask,
        bid_size,
        ask_size,
        bid_depth_usdt: bid * bid_size * contract_size,
        ask_depth_usdt: ask * ask_size * contract_size,
        ts,
    })
}

fn parse_coinm_ticker(
    raw: BinanceTicker,
    ts: DateTime<Utc>,
    contract_sizes: &HashMap<String, f64>,
) -> Option<Ticker> {
    let base = raw
        .symbol
        .strip_suffix("USD_PERP")
        .or_else(|| raw.symbol.strip_suffix("USD_PERPETUAL"))?
        .to_string();
    let bid: f64 = raw.bid_price.parse().ok()?;
    let ask: f64 = raw.ask_price.parse().ok()?;
    let bid_size: f64 = raw.bid_qty.parse().ok()?;
    let ask_size: f64 = raw.ask_qty.parse().ok()?;
    let contract_size = *contract_sizes.get(&raw.symbol).unwrap_or(&1.0);
    Some(Ticker {
        exchange: Exchange::Binance,
        exchange_symbol: raw.symbol,
        base,
        kind: ContractKind::CoinPerp,
        bid,
        ask,
        bid_size,
        ask_size,
        bid_depth_usdt: bid_size * contract_size,
        ask_depth_usdt: ask_size * contract_size,
        ts,
    })
}

fn parse_usdm_funding(raw: BinancePremiumIndex) -> Option<FundingInfo> {
    let base = raw.symbol.strip_suffix("USDT")?.to_string();
    Some(FundingInfo {
        exchange: Exchange::Binance,
        exchange_symbol: raw.symbol,
        base,
        kind: ContractKind::UsdtPerp,
        rate_bps: raw
            .last_funding_rate
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v * 10_000.0),
        next_funding_time: raw.next_funding_time.and_then(millis_to_utc),
    })
}

fn parse_coinm_funding(raw: BinancePremiumIndex) -> Option<FundingInfo> {
    let base = raw
        .pair
        .as_deref()
        .and_then(|pair| pair.strip_suffix("USD"))
        .map(str::to_string)
        .or_else(|| {
            raw.symbol
                .strip_suffix("USD_PERP")
                .or_else(|| raw.symbol.strip_suffix("USD_PERPETUAL"))
                .map(str::to_string)
        })?;
    Some(FundingInfo {
        exchange: Exchange::Binance,
        exchange_symbol: raw.symbol,
        base,
        kind: ContractKind::CoinPerp,
        rate_bps: raw
            .last_funding_rate
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v * 10_000.0),
        next_funding_time: raw.next_funding_time.and_then(millis_to_utc),
    })
}

fn parse_binance_premium_value(value: serde_json::Value) -> Result<Vec<BinancePremiumIndex>> {
    if value.is_array() {
        Ok(serde_json::from_value(value)?)
    } else {
        Ok(vec![serde_json::from_value(value)?])
    }
}

fn millis_to_utc(ms: i64) -> Option<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp_millis(ms)
}

fn contract_size_map(info: &BinanceExchangeInfo) -> HashMap<String, f64> {
    info.symbols
        .iter()
        .filter_map(|symbol| Some((symbol.symbol.clone(), symbol.contract_size?)))
        .collect()
}
