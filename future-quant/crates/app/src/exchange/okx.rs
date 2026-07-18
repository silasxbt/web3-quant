use crate::model::{ContractKind, Exchange, FundingInfo, Instrument, Ticker};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone)]
pub struct OkxClient {
    http: Client,
}

impl OkxClient {
    pub fn new(http: Client) -> Self {
        Self { http }
    }

    pub async fn instruments(&self) -> Result<Vec<Instrument>> {
        let raw: OkxEnvelope<OkxInstrument> = self
            .http
            .get("https://www.okx.com/api/v5/public/instruments")
            .query(&[("instType", "SWAP")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("failed to parse OKX instruments")?;
        Ok(raw
            .data
            .into_iter()
            .filter(|item| item.state.as_deref() == Some("live"))
            .filter_map(parse_okx_instrument)
            .collect())
    }

    pub async fn tickers(&self) -> Result<Vec<Ticker>> {
        let (instruments, raw) = tokio::try_join!(self.instruments(), async {
            let raw: OkxEnvelope<OkxTicker> = self
                .http
                .get("https://www.okx.com/api/v5/market/tickers")
                .query(&[("instType", "SWAP")])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await
                .context("failed to parse OKX tickers")?;
            Ok::<_, anyhow::Error>(raw)
        })?;
        let contract_sizes = instruments
            .into_iter()
            .map(|item| (item.exchange_symbol, item.contract_size))
            .collect::<HashMap<_, _>>();
        Ok(raw
            .data
            .into_iter()
            .filter_map(|raw| parse_okx_ticker(raw, &contract_sizes))
            .collect())
    }

    pub async fn funding_for(&self, instruments: &[Instrument]) -> Vec<FundingInfo> {
        let mut output = Vec::new();
        for instrument in instruments {
            if instrument.exchange != Exchange::Okx {
                continue;
            }
            match self.funding_one(&instrument.exchange_symbol).await {
                Ok(Some(funding)) => output.push(funding),
                Ok(None) => {}
                Err(err) => {
                    tracing::debug!(symbol = %instrument.exchange_symbol, error = ?err, "failed to fetch OKX funding");
                }
            }
        }
        output
    }

    async fn funding_one(&self, inst_id: &str) -> Result<Option<FundingInfo>> {
        let raw: OkxEnvelope<OkxFunding> = self
            .http
            .get("https://www.okx.com/api/v5/public/funding-rate")
            .query(&[("instId", inst_id)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(raw.data.into_iter().next().and_then(parse_okx_funding))
    }
}

#[derive(Debug, Deserialize)]
struct OkxEnvelope<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OkxInstrument {
    inst_id: String,
    inst_type: String,
    uly: Option<String>,
    base_ccy: Option<String>,
    quote_ccy: Option<String>,
    ct_type: Option<String>,
    ct_val: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OkxTicker {
    inst_id: String,
    bid_px: String,
    bid_sz: String,
    ask_px: String,
    ask_sz: String,
    ts: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OkxFunding {
    inst_id: String,
    funding_rate: Option<String>,
    next_funding_time: Option<String>,
}

fn parse_okx_instrument(raw: OkxInstrument) -> Option<Instrument> {
    if raw.inst_type != "SWAP" {
        return None;
    }
    let kind = match raw.ct_type.as_deref()? {
        "linear" => ContractKind::UsdtPerp,
        "inverse" => ContractKind::CoinPerp,
        _ => return None,
    };
    let base = raw
        .base_ccy
        .filter(|v| !v.is_empty())
        .or_else(|| raw.uly.as_deref()?.split('-').next().map(str::to_string))?;
    let quote = raw.quote_ccy.filter(|v| !v.is_empty()).unwrap_or_else(|| {
        if kind == ContractKind::UsdtPerp {
            "USDT".to_string()
        } else {
            "USD".to_string()
        }
    });
    Some(Instrument {
        exchange: Exchange::Okx,
        exchange_symbol: raw.inst_id,
        base,
        quote,
        kind,
        contract_size: raw.ct_val.and_then(|v| v.parse().ok()).unwrap_or(1.0),
        status: raw.state.unwrap_or_else(|| "live".to_string()),
    })
}

fn parse_okx_ticker(raw: OkxTicker, contract_sizes: &HashMap<String, f64>) -> Option<Ticker> {
    let inst_id = raw.inst_id;
    let parts: Vec<&str> = inst_id.split('-').collect();
    if parts.len() < 3 {
        return None;
    }
    let kind = match parts.get(1).copied()? {
        "USDT" => ContractKind::UsdtPerp,
        "USD" => ContractKind::CoinPerp,
        _ => return None,
    };
    let base = parts[0].to_string();
    let bid: f64 = raw.bid_px.parse().ok()?;
    let ask: f64 = raw.ask_px.parse().ok()?;
    let bid_size: f64 = raw.bid_sz.parse().ok()?;
    let ask_size: f64 = raw.ask_sz.parse().ok()?;
    let contract_size = *contract_sizes.get(&inst_id).unwrap_or(&1.0);
    let (bid_depth_usdt, ask_depth_usdt) = match kind {
        ContractKind::UsdtPerp => (
            bid * bid_size * contract_size,
            ask * ask_size * contract_size,
        ),
        ContractKind::CoinPerp => (bid_size * contract_size, ask_size * contract_size),
    };
    Some(Ticker {
        exchange: Exchange::Okx,
        exchange_symbol: inst_id,
        base,
        kind,
        bid,
        ask,
        bid_size,
        ask_size,
        bid_depth_usdt,
        ask_depth_usdt,
        ts: millis_to_utc(raw.ts.parse().ok()?)?,
    })
}

fn parse_okx_funding(raw: OkxFunding) -> Option<FundingInfo> {
    let inst_id = raw.inst_id;
    let parts: Vec<&str> = inst_id.split('-').collect();
    if parts.len() < 3 {
        return None;
    }
    let kind = match parts.get(1).copied()? {
        "USDT" => ContractKind::UsdtPerp,
        "USD" => ContractKind::CoinPerp,
        _ => return None,
    };
    let base = parts[0].to_string();
    Some(FundingInfo {
        exchange: Exchange::Okx,
        exchange_symbol: inst_id,
        base,
        kind,
        rate_bps: raw
            .funding_rate
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v * 10_000.0),
        next_funding_time: raw
            .next_funding_time
            .and_then(|v| v.parse::<i64>().ok())
            .and_then(millis_to_utc),
    })
}

fn millis_to_utc(ms: i64) -> Option<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp_millis(ms)
}
