use crate::{
    config::LightningAuditConfig,
    model::{ContractKind, Opportunity},
    state::SharedState,
};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{fs, path::PathBuf};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AuditManager {
    http: Client,
    state: SharedState,
    config_path: PathBuf,
    settings: std::sync::Arc<RwLock<AuditSettings>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSettings {
    pub enabled: bool,
    pub base_url: String,
    pub api_key_env: String,
    pub selected_model: String,
    pub selected_prompt_id: String,
    pub request_timeout_secs: u64,
    pub temperature: f32,
    pub max_tokens: u32,
    pub show_process_log: bool,
    pub prompts: Vec<AuditPrompt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditPrompt {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub user_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSettingsUpdate {
    pub enabled: bool,
    pub base_url: String,
    pub api_key_env: String,
    pub selected_model: String,
    pub selected_prompt_id: String,
    pub request_timeout_secs: u64,
    pub temperature: f32,
    pub max_tokens: u32,
    pub show_process_log: bool,
    pub prompts: Vec<AuditPrompt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditModelsResponse {
    pub configured_model: String,
    pub models: Vec<String>,
    pub process: Vec<AuditProcessEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRequest {
    pub opportunity_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditResponse {
    pub opportunity_id: String,
    pub audited_at: String,
    pub model: String,
    pub prompt_id: String,
    pub verdict: String,
    pub answer: String,
    pub process: Vec<AuditProcessEvent>,
    pub evidence_links: Vec<EvidenceLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditProcessEvent {
    pub label: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceLink {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct ModelsEnvelope {
    data: Vec<ModelItem>,
}

#[derive(Debug, Deserialize)]
struct ModelItem {
    id: String,
}

impl AuditManager {
    pub fn new(config: LightningAuditConfig, http: Client, state: SharedState) -> Self {
        let config_path = PathBuf::from(&config.config_path);
        let defaults = AuditSettings::from_config(config);
        let settings = load_or_init_settings(&config_path, defaults);
        Self {
            http,
            state,
            config_path,
            settings: std::sync::Arc::new(RwLock::new(settings)),
        }
    }

    pub async fn settings(&self) -> AuditSettings {
        self.settings.read().await.clone()
    }

    pub async fn save_settings(&self, update: AuditSettingsUpdate) -> Result<AuditSettings> {
        let settings = AuditSettings {
            enabled: update.enabled,
            base_url: trim_base_url(&update.base_url),
            api_key_env: update.api_key_env.trim().to_string(),
            selected_model: update.selected_model.trim().to_string(),
            selected_prompt_id: update.selected_prompt_id.trim().to_string(),
            request_timeout_secs: update.request_timeout_secs.clamp(5, 180),
            temperature: update.temperature.clamp(0.0, 2.0),
            max_tokens: update.max_tokens.clamp(256, 8000),
            show_process_log: update.show_process_log,
            prompts: sanitize_prompts(update.prompts),
        };
        if settings.prompts.is_empty() {
            return Err(anyhow!("至少需要保留一个审计提示词"));
        }
        fs::create_dir_all(
            self.config_path
                .parent()
                .ok_or_else(|| anyhow!("invalid audit config path"))?,
        )?;
        fs::write(&self.config_path, serde_json::to_string_pretty(&settings)?)?;
        *self.settings.write().await = settings.clone();
        Ok(settings)
    }

    pub async fn refresh_models(&self) -> AuditModelsResponse {
        let settings = self.settings().await;
        let mut process = vec![AuditProcessEvent {
            label: "读取配置".to_string(),
            detail: format!(
                "模型接口：{}，密钥环境变量：{}",
                settings.base_url, settings.api_key_env
            ),
        }];
        let Some(api_key) = read_api_key(&settings) else {
            process.push(AuditProcessEvent {
                label: "跳过刷新".to_string(),
                detail: "未找到 API key 环境变量，返回当前配置模型。".to_string(),
            });
            return AuditModelsResponse {
                configured_model: settings.selected_model.clone(),
                models: vec![settings.selected_model],
                process,
            };
        };
        let url = format!("{}/models", trim_base_url(&settings.base_url));
        process.push(AuditProcessEvent {
            label: "请求模型列表".to_string(),
            detail: format!("GET {}", url),
        });
        let result = self
            .http
            .get(&url)
            .bearer_auth(api_key)
            .send()
            .await
            .and_then(|resp| resp.error_for_status());
        let Ok(resp) = result else {
            process.push(AuditProcessEvent {
                label: "刷新失败".to_string(),
                detail: "模型接口不可用或鉴权失败，保留当前配置模型。".to_string(),
            });
            return AuditModelsResponse {
                configured_model: settings.selected_model.clone(),
                models: vec![settings.selected_model],
                process,
            };
        };
        match resp.json::<ModelsEnvelope>().await {
            Ok(envelope) => {
                let mut models = envelope
                    .data
                    .into_iter()
                    .map(|item| item.id)
                    .collect::<Vec<_>>();
                models.sort();
                models.dedup();
                if models.is_empty() {
                    models.push(settings.selected_model.clone());
                }
                process.push(AuditProcessEvent {
                    label: "刷新完成".to_string(),
                    detail: format!("获取到 {} 个模型。", models.len()),
                });
                AuditModelsResponse {
                    configured_model: settings.selected_model,
                    models,
                    process,
                }
            }
            Err(err) => {
                process.push(AuditProcessEvent {
                    label: "解析失败".to_string(),
                    detail: format!("模型列表响应无法解析：{err}"),
                });
                AuditModelsResponse {
                    configured_model: settings.selected_model.clone(),
                    models: vec![settings.selected_model],
                    process,
                }
            }
        }
    }

    pub async fn audit(&self, request: AuditRequest) -> Result<AuditResponse> {
        let settings = self.settings().await;
        if !settings.enabled {
            return Err(anyhow!("闪电审计未启用"));
        }
        let Some(api_key) = read_api_key(&settings) else {
            return Err(anyhow!("未找到 API key 环境变量：{}", settings.api_key_env));
        };
        let opportunity = self
            .state
            .get_opportunity(&request.opportunity_id)
            .await
            .ok_or_else(|| anyhow!("机会不存在或已过期：{}", request.opportunity_id))?;
        let prompt = settings
            .prompts
            .iter()
            .find(|item| item.id == settings.selected_prompt_id)
            .or_else(|| settings.prompts.first())
            .cloned()
            .ok_or_else(|| anyhow!("没有可用审计提示词"))?;
        let evidence_links = evidence_links(&opportunity);
        let mut process = vec![
            AuditProcessEvent {
                label: "机会快照".to_string(),
                detail: opportunity_summary(&opportunity),
            },
            AuditProcessEvent {
                label: "生成联网检索入口".to_string(),
                detail: evidence_links
                    .iter()
                    .map(|item| format!("{} -> {}", item.title, item.url))
                    .collect::<Vec<_>>()
                    .join("\n"),
            },
            AuditProcessEvent {
                label: "选择模型与提示词".to_string(),
                detail: format!("模型：{}，提示词：{}", settings.selected_model, prompt.name),
            },
        ];
        let user_prompt = render_user_prompt(&prompt.user_template, &opportunity, &evidence_links);
        if settings.show_process_log {
            process.push(AuditProcessEvent {
                label: "模型输入".to_string(),
                detail: format!(
                    "System Prompt:\n{}\n\nUser Prompt:\n{}",
                    prompt.system_prompt, user_prompt
                ),
            });
        }
        let url = format!("{}/chat/completions", trim_base_url(&settings.base_url));
        process.push(AuditProcessEvent {
            label: "调用审计模型".to_string(),
            detail: format!("POST {}", url),
        });
        let payload = json!({
            "model": settings.selected_model,
            "temperature": settings.temperature,
            "max_tokens": settings.max_tokens,
            "messages": [
                {"role": "system", "content": prompt.system_prompt},
                {"role": "user", "content": user_prompt}
            ]
        });
        let resp = self
            .http
            .post(&url)
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await
            .with_context(|| "审计模型请求失败")?
            .error_for_status()
            .with_context(|| "审计模型返回错误状态")?;
        let value = resp
            .json::<serde_json::Value>()
            .await
            .with_context(|| "审计模型响应无法解析")?;
        let answer = value
            .get("choices")
            .and_then(|choices| choices.as_array())
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_str())
            .unwrap_or("模型未返回可读内容。")
            .trim()
            .to_string();
        process.push(AuditProcessEvent {
            label: "审计完成".to_string(),
            detail: "模型已返回审计结论。".to_string(),
        });
        Ok(AuditResponse {
            opportunity_id: request.opportunity_id,
            audited_at: Utc::now().to_rfc3339(),
            model: settings.selected_model,
            prompt_id: prompt.id,
            verdict: extract_verdict(&answer),
            answer,
            process,
            evidence_links,
        })
    }
}

impl AuditSettings {
    fn from_config(config: LightningAuditConfig) -> Self {
        Self {
            enabled: config.enabled,
            base_url: trim_base_url(&config.openai_compatible_base_url),
            api_key_env: config.api_key_env,
            selected_model: config.default_model,
            selected_prompt_id: config.default_prompt_id,
            request_timeout_secs: config.request_timeout_secs,
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            show_process_log: config.show_process_log,
            prompts: default_prompts(),
        }
    }
}

fn load_or_init_settings(path: &PathBuf, defaults: AuditSettings) -> AuditSettings {
    if let Ok(raw) = fs::read_to_string(path) {
        if let Ok(settings) = serde_json::from_str::<AuditSettings>(&raw) {
            return settings;
        }
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(
        path,
        serde_json::to_string_pretty(&defaults).unwrap_or_default(),
    );
    defaults
}

fn sanitize_prompts(prompts: Vec<AuditPrompt>) -> Vec<AuditPrompt> {
    prompts
        .into_iter()
        .filter_map(|prompt| {
            let id = prompt.id.trim();
            let name = prompt.name.trim();
            let system_prompt = prompt.system_prompt.trim();
            let user_template = prompt.user_template.trim();
            if id.is_empty()
                || name.is_empty()
                || system_prompt.is_empty()
                || user_template.is_empty()
            {
                return None;
            }
            Some(AuditPrompt {
                id: id.to_string(),
                name: name.to_string(),
                system_prompt: system_prompt.to_string(),
                user_template: user_template.to_string(),
            })
        })
        .collect()
}

fn read_api_key(settings: &AuditSettings) -> Option<String> {
    std::env::var(&settings.api_key_env)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn trim_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn default_prompts() -> Vec<AuditPrompt> {
    vec![
        AuditPrompt {
            id: "strict_token_equivalence".to_string(),
            name: "严格标的等价审计".to_string(),
            system_prompt: STRICT_SYSTEM_PROMPT.to_string(),
            user_template: DEFAULT_USER_TEMPLATE.to_string(),
        },
        AuditPrompt {
            id: "fast_risk_triage".to_string(),
            name: "快速风险分诊".to_string(),
            system_prompt: FAST_SYSTEM_PROMPT.to_string(),
            user_template: DEFAULT_USER_TEMPLATE.to_string(),
        },
    ]
}

fn opportunity_summary(opp: &Opportunity) -> String {
    format!(
        "{} {}，做空 {} {} @ {:.8}，做多 {} {} @ {:.8}，毛价差 {:.2}%，预估净价差 {:.2}%，最小侧深度 {:.2}U，建议仓位 {:.2}U。",
        opp.base,
        opp.kind.as_zh(),
        opp.short_exchange.as_zh(),
        opp.short_symbol,
        opp.short_price,
        opp.long_exchange.as_zh(),
        opp.long_symbol,
        opp.long_price,
        opp.gross_spread_bps / 100.0,
        opp.estimated_net_spread_bps / 100.0,
        opp.min_side_depth_usdt,
        opp.suggested_notional_usdt,
    )
}

fn render_user_prompt(template: &str, opp: &Opportunity, links: &[EvidenceLink]) -> String {
    template
        .replace("{{base}}", &opp.base)
        .replace("{{kind}}", opp.kind.as_zh())
        .replace("{{short_exchange}}", opp.short_exchange.as_zh())
        .replace("{{short_symbol}}", &opp.short_symbol)
        .replace("{{short_price}}", &format!("{:.8}", opp.short_price))
        .replace("{{long_exchange}}", opp.long_exchange.as_zh())
        .replace("{{long_symbol}}", &opp.long_symbol)
        .replace("{{long_price}}", &format!("{:.8}", opp.long_price))
        .replace(
            "{{gross_spread}}",
            &format!("{:.2}%", opp.gross_spread_bps / 100.0),
        )
        .replace(
            "{{estimated_net_spread}}",
            &format!("{:.2}%", opp.estimated_net_spread_bps / 100.0),
        )
        .replace(
            "{{min_side_depth}}",
            &format!("{:.2} USDT", opp.min_side_depth_usdt),
        )
        .replace(
            "{{suggested_notional}}",
            &format!("{:.2} USDT", opp.suggested_notional_usdt),
        )
        .replace(
            "{{risk_flags}}",
            &if opp.risk_flags.is_empty() {
                "暂无系统风险标记".to_string()
            } else {
                opp.risk_flags.join("\n")
            },
        )
        .replace(
            "{{evidence_links}}",
            &links
                .iter()
                .map(|item| format!("- {}: {}", item.title, item.url))
                .collect::<Vec<_>>()
                .join("\n"),
        )
}

fn evidence_links(opp: &Opportunity) -> Vec<EvidenceLink> {
    let base = &opp.base;
    let kind = match opp.kind {
        ContractKind::UsdtPerp => "USDT perpetual",
        ContractKind::CoinPerp => "coin margined perpetual",
    };
    let queries = [
        (
            "标的等价检索",
            format!(
                "{} {} Binance {} OKX {} contract underlying same token",
                base, kind, opp.short_symbol, opp.long_symbol
            ),
        ),
        (
            "操纵与强控盘检索",
            format!(
                "{} token manipulation scam exploit delisting suspension risk",
                base
            ),
        ),
        (
            "资金费率与结算检索",
            format!(
                "{} funding rate Binance OKX perpetual settlement risk",
                base
            ),
        ),
    ];
    queries
        .into_iter()
        .map(|(title, query)| EvidenceLink {
            title: title.to_string(),
            url: format!(
                "https://www.google.com/search?q={}",
                urlencoding::encode(&query)
            ),
        })
        .collect()
}

fn extract_verdict(answer: &str) -> String {
    for line in answer.lines().take(12) {
        let trimmed = line.trim();
        if trimmed.contains("结论") || trimmed.contains("等级") || trimmed.contains("建议") {
            return trimmed.trim_matches(['#', '*', '-', ' ']).to_string();
        }
    }
    "需人工复核".to_string()
}

const STRICT_SYSTEM_PROMPT: &str = r#"你是加密永续合约跨交易所价差机会的闪电审计 Agent。你必须用中文回复。

目标是在 30-60 秒内给出可执行的审计结论，而不是写长报告。请保持谨慎：如果信息不足，结论必须偏向“暂缓/人工复核”。

你要审计三件事：
1. 两个交易所合约标的是否实际等价，包括 token、合约单位、计价币、交割合约/永续类型、是否可能同名不同币。
2. 该代币近期是否存在恶意操纵、强控盘、异常拉盘砸盘、暂停交易、充提异常、合约风控限制、交易所公告风险。
3. 资金费率是否即将结算，或是否存在资金费率异常导致持仓成本吞噬价差的风险。

回复格式必须固定：

结论等级：可继续观察 / 暂缓 / 禁止执行
一句话结论：...
标的等价审计：...
操纵与控盘风险：...
资金费率结算风险：...
需要人工确认：列出最多 5 项
可见依据：引用用户提供的搜索入口、合约信息或你实际使用的网页标题/URL

不要输出隐藏推理链。可以输出简洁的审计步骤、检查项、依据和不确定性。"#;

const FAST_SYSTEM_PROMPT: &str = r#"你是一个偏风控的交易机会快速分诊 Agent。你必须用中文回复，重点识别“不要做”的理由。

如果两个交易所标的不等价、合约单位可疑、盘口深度过低、资金费率临近结算、或搜索证据不足，请直接给出暂缓或禁止执行。回复要短、清楚、可操作。"#;

const DEFAULT_USER_TEMPLATE: &str = r#"请对以下跨交易所永续合约价差机会做闪电审计。

机会信息：
- 币种：{{base}}
- 合约类型：{{kind}}
- 做空侧：{{short_exchange}} {{short_symbol}} @ {{short_price}}
- 做多侧：{{long_exchange}} {{long_symbol}} @ {{long_price}}
- 毛价差：{{gross_spread}}
- 预估净价差：{{estimated_net_spread}}
- 两边最小深度：{{min_side_depth}}
- 建议仓位：{{suggested_notional}}
- 系统风险标记：
{{risk_flags}}

可见联网检索入口：
{{evidence_links}}

请优先审计：
1. 两边标的是否实际等价。
2. 是否有恶意操纵、强控盘、交易所公告、暂停交易、充提异常等风险。
3. 资金费率是否即将结算或异常。

请按系统要求的固定格式输出。"#;
