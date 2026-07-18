use crate::{
    alert::format_open_message, config::AppConfig, model::Opportunity, state::SharedState,
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
};
use tokio::{
    sync::Mutex,
    time::{sleep, Duration},
};

#[derive(Clone)]
pub struct TelegramNotifier {
    config: AppConfig,
    http: reqwest::Client,
    token: Option<String>,
    auto_tasks: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    offset: Arc<AtomicI64>,
}

impl TelegramNotifier {
    pub fn new(config: AppConfig, http: reqwest::Client) -> Self {
        let token = std::env::var(&config.alert.telegram.bot_token_env).ok();
        Self {
            config,
            http,
            token,
            auto_tasks: Arc::new(Mutex::new(HashMap::new())),
            offset: Arc::new(AtomicI64::new(0)),
        }
    }

    pub async fn send_opportunity(&self, opportunity: &Opportunity, text: &str) {
        if !self.enabled() {
            return;
        }
        let keyboard = json!({
            "inline_keyboard": [
                [
                    {"text": "立即刷新", "callback_data": format!("refresh:{}", opportunity.id)},
                    {"text": "自动 5 秒", "callback_data": format!("auto:5:{}", opportunity.id)},
                    {"text": "自动 10 秒", "callback_data": format!("auto:10:{}", opportunity.id)}
                ],
                [
                    {"text": "自动 20 秒", "callback_data": format!("auto:20:{}", opportunity.id)},
                    {"text": "停止更新", "callback_data": format!("stop:{}", opportunity.id)},
                    {"text": "静默币种", "callback_data": format!("mute:{}", opportunity.base)}
                ],
                [
                    {"text": "打开 Web 控制台", "url": format!("http://{}", self.config.web.listen_addr)}
                ]
            ]
        });
        self.send(text, Some(keyboard)).await;
    }

    pub async fn send_plain(&self, text: &str) {
        if !self.enabled() {
            return;
        }
        self.send(text, None).await;
    }

    pub fn start_callback_polling(&self, state: SharedState) {
        if !self.enabled() || !self.config.alert.telegram.interactive {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            this.poll_callbacks(state).await;
        });
    }

    async fn poll_callbacks(self, state: SharedState) {
        loop {
            if let Err(err) = self.poll_callbacks_once(state.clone()).await {
                tracing::debug!(error = ?err, "Telegram callback polling failed");
            }
            sleep(Duration::from_secs(2)).await;
        }
    }

    async fn poll_callbacks_once(&self, state: SharedState) -> anyhow::Result<()> {
        let Some(token) = &self.token else {
            return Ok(());
        };
        let offset = self.offset.load(Ordering::Relaxed);
        let url = format!("https://api.telegram.org/bot{}/getUpdates", token);
        let resp: serde_json::Value = self
            .http
            .get(url)
            .query(&[
                ("timeout", "0".to_string()),
                ("allowed_updates", "[\"callback_query\"]".to_string()),
                ("offset", offset.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let Some(updates) = resp.get("result").and_then(|v| v.as_array()) else {
            return Ok(());
        };
        for update in updates {
            if let Some(update_id) = update.get("update_id").and_then(|v| v.as_i64()) {
                self.offset.store(update_id + 1, Ordering::Relaxed);
            }
            let Some(callback) = update.get("callback_query") else {
                continue;
            };
            let callback_id = callback
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let data = callback
                .get("data")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            self.handle_callback(state.clone(), callback_id, data).await;
        }
        Ok(())
    }

    async fn handle_callback(&self, state: SharedState, callback_id: String, data: String) {
        let parts = data.split(':').collect::<Vec<_>>();
        match parts.as_slice() {
            ["refresh", id] => {
                self.answer_callback(&callback_id, "正在刷新").await;
                self.send_current_opportunity(&state, id).await;
            }
            ["auto", seconds, id] => {
                let Ok(seconds) = seconds.parse::<u64>() else {
                    self.answer_callback(&callback_id, "更新间隔无效").await;
                    return;
                };
                self.answer_callback(&callback_id, &format!("已开启每 {} 秒自动更新", seconds))
                    .await;
                self.start_auto_update(state, (*id).to_string(), seconds)
                    .await;
            }
            ["stop", id] => {
                self.stop_auto_update(id).await;
                state.stop_auto_marker(id).await;
                self.answer_callback(&callback_id, "已停止自动更新").await;
            }
            ["mute", base] => {
                state.mute_symbol(base).await;
                self.answer_callback(&callback_id, &format!("已静默 {}", base))
                    .await;
            }
            _ => {
                self.answer_callback(&callback_id, "暂不支持该操作").await;
            }
        }
    }

    async fn send_current_opportunity(&self, state: &SharedState, id: &str) {
        if let Some(opp) = state.get_opportunity(id).await {
            self.send_opportunity(&opp, &format_open_message(&opp))
                .await;
        } else {
            self.send_plain(&format!("[机会已过期]\n\n机会 {} 当前已经不再活跃。", id))
                .await;
        }
    }

    async fn start_auto_update(&self, state: SharedState, id: String, seconds: u64) {
        self.stop_auto_update(&id).await;
        let this = self.clone();
        let task_id = id.clone();
        let handle = tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(seconds)).await;
                if state.get_opportunity(&task_id).await.is_none() {
                    this.send_plain(&format!(
                        "[机会已过期]\n\n机会 {} 当前已经不再活跃。",
                        task_id
                    ))
                    .await;
                    break;
                }
                this.send_current_opportunity(&state, &task_id).await;
            }
        });
        self.auto_tasks.lock().await.insert(id, handle);
    }

    async fn stop_auto_update(&self, id: &str) {
        if let Some(handle) = self.auto_tasks.lock().await.remove(id) {
            handle.abort();
        }
    }

    async fn answer_callback(&self, callback_id: &str, text: &str) {
        let Some(token) = &self.token else {
            return;
        };
        if callback_id.is_empty() {
            return;
        }
        let url = format!("https://api.telegram.org/bot{}/answerCallbackQuery", token);
        let payload = json!({
            "callback_query_id": callback_id,
            "text": text,
            "show_alert": false
        });
        if let Err(err) = self.http.post(url).json(&payload).send().await {
            tracing::debug!(error = ?err, "Telegram answerCallbackQuery failed");
        }
    }

    async fn send(&self, text: &str, reply_markup: Option<serde_json::Value>) {
        let Some(token) = &self.token else {
            tracing::warn!("Telegram enabled but token env is missing");
            return;
        };
        let mut payload = json!({
            "chat_id": self.config.alert.telegram.chat_id,
            "text": text,
            "disable_web_page_preview": true
        });
        if let Some(reply_markup) = reply_markup {
            payload["reply_markup"] = reply_markup;
        }
        let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
        match self.http.post(url).json(&payload).send().await {
            Ok(resp) if resp.status().is_success() => {}
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!(%status, body = %body, "Telegram send failed");
            }
            Err(err) => tracing::warn!(error = ?err, "Telegram send error"),
        }
    }

    fn enabled(&self) -> bool {
        self.config.alert.telegram.enabled
            && !self.config.alert.telegram.chat_id.trim().is_empty()
            && self.token.is_some()
    }
}
