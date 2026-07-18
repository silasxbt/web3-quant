use crate::model::{AppStateSnapshot, Opportunity, RuntimeStatus, Stats};
use chrono::Utc;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct SharedState {
    inner: Arc<RwLock<StateInner>>,
}

struct StateInner {
    started_at: chrono::DateTime<Utc>,
    active_opportunities: HashMap<String, Opportunity>,
    recent_opportunities: Vec<Opportunity>,
    blacklisted_symbols: Vec<String>,
    muted_symbols: HashSet<String>,
    status: RuntimeStatus,
    stats: Stats,
    alerted_open: HashSet<String>,
    alerted_close: HashSet<String>,
}

impl SharedState {
    pub fn new(blacklisted_symbols: Vec<String>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(StateInner {
                started_at: Utc::now(),
                active_opportunities: HashMap::new(),
                recent_opportunities: Vec::new(),
                blacklisted_symbols,
                muted_symbols: HashSet::new(),
                status: RuntimeStatus {
                    binance_ok: false,
                    okx_ok: false,
                    last_error: None,
                    last_poll_ms: None,
                    poll_count: 0,
                },
                stats: Stats {
                    discovered_common_symbols: 0,
                    active_count: 0,
                    recent_count: 0,
                    last_report_path: None,
                },
                alerted_open: HashSet::new(),
                alerted_close: HashSet::new(),
            })),
        }
    }

    pub async fn snapshot(&self) -> AppStateSnapshot {
        let state = self.inner.read().await;
        AppStateSnapshot {
            started_at: state.started_at,
            updated_at: Utc::now(),
            active_opportunities: state.active_opportunities.values().cloned().collect(),
            recent_opportunities: state.recent_opportunities.clone(),
            blacklisted_symbols: state.blacklisted_symbols.clone(),
            muted_symbols: state.muted_symbols.iter().cloned().collect(),
            status: state.status.clone(),
            stats: state.stats.clone(),
        }
    }

    pub async fn update_poll(
        &self,
        opportunities: Vec<Opportunity>,
        common_symbols: usize,
        poll_ms: u128,
        open_threshold: impl Fn(&Opportunity) -> bool,
        close_threshold: impl Fn(&Opportunity) -> bool,
    ) -> (Vec<Opportunity>, Vec<Opportunity>) {
        let mut state = self.inner.write().await;
        state.status.binance_ok = true;
        state.status.okx_ok = true;
        state.status.last_error = None;
        state.status.last_poll_ms = Some(poll_ms);
        state.status.poll_count += 1;
        state.stats.discovered_common_symbols = common_symbols;

        let mut new_alerts = Vec::new();
        let mut closed_alerts = Vec::new();
        let current_by_id: HashMap<String, Opportunity> = opportunities
            .iter()
            .map(|opp| (opp.id.clone(), opp.clone()))
            .collect();

        for opp in &opportunities {
            if !open_threshold(opp) {
                continue;
            }
            if state.muted_symbols.contains(&opp.base.to_uppercase()) {
                state.alerted_open.insert(opp.id.clone());
                continue;
            }
            if !state.alerted_open.contains(&opp.id) {
                state.alerted_open.insert(opp.id.clone());
                new_alerts.push(opp.clone());
            }
        }

        let current_ids: Vec<String> = state.active_opportunities.keys().cloned().collect();
        for id in current_ids {
            if state.alerted_close.contains(&id) {
                continue;
            }
            let should_close = current_by_id.get(&id).map(&close_threshold).unwrap_or(true);
            if should_close {
                let mut close_opp = current_by_id
                    .get(&id)
                    .cloned()
                    .or_else(|| state.active_opportunities.get(&id).cloned());
                if let Some(ref mut close_opp) = close_opp {
                    close_opp.close_alert_sent = true;
                }
                state.alerted_close.insert(id);
                if let Some(close_opp) = close_opp {
                    closed_alerts.push(close_opp);
                }
            }
        }

        let mut active_next = HashMap::new();
        for opp in opportunities
            .iter()
            .filter(|opp| state.alerted_open.contains(&opp.id))
        {
            if !state.alerted_close.contains(&opp.id) {
                active_next.insert(opp.id.clone(), opp.clone());
            }
        }
        let mut recent_updates = new_alerts.clone();
        recent_updates.extend(active_next.values().cloned());
        state.active_opportunities = active_next;
        state.recent_opportunities.extend(recent_updates);
        if state.recent_opportunities.len() > 200 {
            let keep_from = state.recent_opportunities.len() - 200;
            state.recent_opportunities.drain(0..keep_from);
        }
        state.stats.active_count = state.active_opportunities.len();
        state.stats.recent_count = state.recent_opportunities.len();

        (new_alerts, closed_alerts)
    }

    pub async fn mark_error(&self, error: String) {
        let mut state = self.inner.write().await;
        state.status.last_error = Some(error);
        state.status.binance_ok = false;
        state.status.okx_ok = false;
        state.status.poll_count += 1;
    }

    pub async fn set_report_path(&self, path: String) {
        let mut state = self.inner.write().await;
        state.stats.last_report_path = Some(path);
    }

    pub async fn get_opportunity(&self, id: &str) -> Option<Opportunity> {
        let state = self.inner.read().await;
        state.active_opportunities.get(id).cloned().or_else(|| {
            state
                .recent_opportunities
                .iter()
                .rev()
                .find(|opp| opp.id == id)
                .cloned()
        })
    }

    pub async fn mute_symbol(&self, base: &str) {
        let mut state = self.inner.write().await;
        state.muted_symbols.insert(base.to_uppercase());
    }

    pub async fn stop_auto_marker(&self, _id: &str) {}
}
