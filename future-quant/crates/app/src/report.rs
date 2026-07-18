use crate::{config::AppConfig, state::SharedState};
use anyhow::Result;
use chrono::{Local, Timelike};
use std::{fs, path::PathBuf};

pub async fn maybe_write_daily_report(config: &AppConfig, state: &SharedState) -> Result<()> {
    if !config.storage.daily_report_enabled {
        return Ok(());
    }
    let now = Local::now();
    if now.hour() != 23 {
        return Ok(());
    }
    write_daily_report(config, state).await
}

pub async fn write_daily_report(config: &AppConfig, state: &SharedState) -> Result<()> {
    let snapshot = state.snapshot().await;
    fs::create_dir_all(&config.storage.reports_dir)?;
    let date = Local::now().format("%Y-%m-%d").to_string();
    let path = PathBuf::from(&config.storage.reports_dir).join(format!("opportunities-{date}.md"));
    let mut body = String::new();
    body.push_str(&format!("# 价差机会报告 {date}\n\n"));
    body.push_str(&format!(
        "- 活跃机会数量：{}\n",
        snapshot.active_opportunities.len()
    ));
    body.push_str(&format!(
        "- 最近机会记录：{}\n",
        snapshot.recent_opportunities.len()
    ));
    body.push_str(&format!(
        "- 共同合约数量：{}\n\n",
        snapshot.stats.discovered_common_symbols
    ));
    body.push_str("## 最近机会 Top 50\n\n");
    body.push_str("| 币种 | 合约类型 | 净价差 | 毛价差 | 建议仓位 |\n");
    body.push_str("|---|---:|---:|---:|---:|\n");
    for opp in snapshot.recent_opportunities.iter().rev().take(50) {
        body.push_str(&format!(
            "| {} | {} | {:.2}% | {:.2}% | {:.2}U |\n",
            opp.base,
            opp.kind.as_zh(),
            opp.estimated_net_spread_bps / 100.0,
            opp.gross_spread_bps / 100.0,
            opp.suggested_notional_usdt
        ));
    }
    fs::write(&path, body)?;
    state.set_report_path(path.display().to_string()).await;
    Ok(())
}

pub fn cleanup_old_logs(config: &AppConfig) -> Result<()> {
    fs::create_dir_all(&config.storage.data_dir)?;
    fs::create_dir_all(&config.storage.reports_dir)?;
    Ok(())
}
