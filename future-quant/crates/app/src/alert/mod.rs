use crate::{
    alert::macos::MacosNotifier, alert::telegram::TelegramNotifier, config::AppConfig,
    model::Opportunity, state::SharedState,
};

pub mod macos;
pub mod telegram;

#[derive(Clone)]
pub struct AlertManager {
    telegram: TelegramNotifier,
    macos: MacosNotifier,
}

impl AlertManager {
    pub fn new(config: AppConfig, http: reqwest::Client) -> Self {
        Self {
            telegram: TelegramNotifier::new(config.clone(), http),
            macos: MacosNotifier::new(config),
        }
    }

    pub async fn opportunity_opened(&self, opportunity: &Opportunity) {
        let message = format_open_message(opportunity);
        self.telegram.send_opportunity(opportunity, &message).await;
        self.macos
            .notify("价差机会提醒", &notification_summary(opportunity))
            .await;
    }

    pub async fn opportunity_closed(&self, opportunity: &Opportunity) {
        let message = format_close_message(opportunity);
        self.telegram.send_plain(&message).await;
        self.macos
            .notify("价差回归提醒", &notification_summary(opportunity))
            .await;
    }

    pub fn start_telegram_callbacks(&self, state: SharedState) {
        self.telegram.start_callback_polling(state);
    }
}

pub fn format_open_message(opp: &Opportunity) -> String {
    let risks = if opp.risk_flags.is_empty() {
        "暂无额外风险标记".to_string()
    } else {
        opp.risk_flags
            .iter()
            .map(|flag| format!("- {}", flag))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "[价差机会提醒]\n\n币种：{}\n合约类型：{}\n机会等级：{}\n\n交易方向：\n做空 {} {}\n做多 {} {}\n\n价格：\n做空侧买一价：{:.8}\n做多侧卖一价：{:.8}\n\n价差：\n毛价差：{:.2}%\n预估净价差：{:.2}%\n\n盘口深度：\n两边最小可用深度：{:.2} USDT\n建议名义仓位：{:.2} USDT\n\n风险提示：\n{}",
        opp.base,
        opp.kind.as_zh(),
        opp.alert_level.as_zh(),
        opp.short_exchange.as_zh(),
        opp.short_symbol,
        opp.long_exchange.as_zh(),
        opp.long_symbol,
        opp.short_price,
        opp.long_price,
        opp.gross_spread_bps / 100.0,
        opp.estimated_net_spread_bps / 100.0,
        opp.min_side_depth_usdt,
        opp.suggested_notional_usdt,
        risks
    )
}

pub fn format_close_message(opp: &Opportunity) -> String {
    format!(
        "[价差回归提醒]\n\n币种：{}\n合约类型：{}\n原始方向：\n做空 {} {}\n做多 {} {}\n\n该机会已经不再满足报警阈值，请检查 spread 是否已经回归到适合平仓的区间。",
        opp.base,
        opp.kind.as_zh(),
        opp.short_exchange.as_zh(),
        opp.short_symbol,
        opp.long_exchange.as_zh(),
        opp.long_symbol
    )
}

fn notification_summary(opp: &Opportunity) -> String {
    format!(
        "{} {} 预估净价差 {:.2}% {}",
        opp.base,
        opp.kind.as_zh(),
        opp.estimated_net_spread_bps / 100.0,
        opp.alert_level.as_zh()
    )
}
