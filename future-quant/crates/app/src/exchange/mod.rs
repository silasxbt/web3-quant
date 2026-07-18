pub mod binance;
pub mod okx;

use crate::model::{FundingInfo, Instrument, Ticker};
use anyhow::Result;

#[allow(dead_code)]
pub trait ExchangeMarketData {
    async fn instruments(&self) -> Result<Vec<Instrument>>;
    async fn tickers(&self) -> Result<Vec<Ticker>>;
    async fn funding(&self) -> Result<Vec<FundingInfo>>;
}
