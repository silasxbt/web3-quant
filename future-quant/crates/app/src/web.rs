use crate::{
    audit::{AuditManager, AuditRequest, AuditSettingsUpdate},
    config::UiConfig,
    state::SharedState,
};
use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::net::SocketAddr;

#[derive(Clone)]
struct WebState {
    market: SharedState,
    ui: UiConfig,
    audit: AuditManager,
}

#[derive(Debug, Clone, Serialize)]
struct UiSnapshot {
    language: String,
    module_explanations_enabled: bool,
    modules: Vec<ModuleExplanation>,
}

#[derive(Debug, Clone, Serialize)]
struct ModuleExplanation {
    title: &'static str,
    summary: &'static str,
    details: &'static [&'static str],
}

pub async fn serve(
    state: SharedState,
    listen_addr: String,
    ui: UiConfig,
    audit: AuditManager,
) -> Result<()> {
    let web_state = WebState {
        market: state,
        ui,
        audit,
    };
    let app = Router::new()
        .route("/", get(index))
        .route("/api/state", get(api_state))
        .route("/api/ui", get(api_ui))
        .route(
            "/api/audit/settings",
            get(api_audit_settings).post(api_audit_save_settings),
        )
        .route("/api/audit/models", post(api_audit_models))
        .route("/api/audit/run", post(api_audit_run))
        .with_state(web_state);
    let addr: SocketAddr = listen_addr
        .parse()
        .with_context(|| format!("invalid web listen addr {listen_addr}"))?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "web console listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn api_state(State(state): State<WebState>) -> Json<crate::model::AppStateSnapshot> {
    Json(state.market.snapshot().await)
}

async fn api_ui(State(state): State<WebState>) -> Json<UiSnapshot> {
    Json(UiSnapshot {
        language: state.ui.language,
        module_explanations_enabled: state.ui.module_explanations_enabled,
        modules: module_explanations(),
    })
}

async fn api_audit_settings(State(state): State<WebState>) -> Json<crate::audit::AuditSettings> {
    Json(state.audit.settings().await)
}

async fn api_audit_save_settings(
    State(state): State<WebState>,
    Json(update): Json<AuditSettingsUpdate>,
) -> impl IntoResponse {
    match state.audit.save_settings(update).await {
        Ok(settings) => (StatusCode::OK, Json(json_ok(settings))).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json_error(format!("保存失败：{err}"))),
        )
            .into_response(),
    }
}

async fn api_audit_models(
    State(state): State<WebState>,
) -> Json<crate::audit::AuditModelsResponse> {
    Json(state.audit.refresh_models().await)
}

async fn api_audit_run(
    State(state): State<WebState>,
    Json(request): Json<AuditRequest>,
) -> impl IntoResponse {
    match state.audit.audit(request).await {
        Ok(result) => (StatusCode::OK, Json(json_ok(result))).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json_error(format!("审计失败：{err}"))),
        )
            .into_response(),
    }
}

fn json_ok<T: Serialize>(data: T) -> serde_json::Value {
    serde_json::json!({ "ok": true, "data": data })
}

fn json_error(message: String) -> serde_json::Value {
    serde_json::json!({ "ok": false, "error": message })
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

fn module_explanations() -> Vec<ModuleExplanation> {
    vec![
        ModuleExplanation {
            title: "行情扫描",
            summary: "自动读取 Binance 与 OKX 的永续合约公共行情，并只保留两边共同上架的币种。",
            details: &[
                "覆盖 U 本位永续和币本位永续。",
                "每轮扫描会刷新交易所状态、共同币种数量和最新盘口。",
                "黑名单币种会在发现阶段被排除，不参与后续价差判断。",
            ],
        },
        ModuleExplanation {
            title: "价差识别",
            summary: "比较两边买一/卖一价格，寻找一边偏贵、一边偏便宜的跨所 spread。",
            details: &[
                "方向规则是贵的一边做空，便宜的一边做多。",
                "毛价差来自价格差，预估净价差会扣除手续费和最大滑点。",
                "当前报警阈值是预估净价差至少 4%。",
            ],
        },
        ModuleExplanation {
            title: "仓位建议",
            summary: "默认按 100 USDT 名义仓位评估，但会根据两边盘口深度自动降档。",
            details: &[
                "深度不足时会降为 50U、25U 或仅观察。",
                "建议仓位不会超过最小侧盘口深度的 20%。",
                "显示为 0U 的机会代表价差存在，但当前深度不适合直接执行。",
            ],
        },
        ModuleExplanation {
            title: "风险提醒",
            summary: "在机会上附加资金费率、临近结算、资金费率差异等风险标记。",
            details: &[
                "资金费率绝对值过高会提示单边持仓成本风险。",
                "距离资金费率结算过近会提示结算窗口风险。",
                "后续交易模块会继续扩展限仓、只减仓、暂停交易和划转异常提醒。",
            ],
        },
        ModuleExplanation {
            title: "闪电审计",
            summary:
                "接入 OpenAI 兼容模型，对机会进行快速代币等价、操纵风险和资金费率结算风险审计。",
            details: &[
                "支持刷新和选择模型，支持编辑和选择提示词模板。",
                "审计过程会展示机会快照、检索入口、模型输入和模型返回。",
                "配置保存到本地运行时目录的 data/lightning-audit-config.json，API key 只从环境变量读取。",
            ],
        },
        ModuleExplanation {
            title: "Telegram 交互",
            summary: "机会触发后发送交互式 Bot 消息，支持手动刷新和 5/10/20 秒自动更新。",
            details: &[
                "立即刷新会重新推送当前机会状态。",
                "自动更新只跟踪该机会，机会消失后会提示过期。",
                "静默币种会临时停止该币种的新机会提醒。",
            ],
        },
        ModuleExplanation {
            title: "macOS 通知",
            summary: "在 Mac mini 本地弹出系统通知，不播放声音，作为 Telegram 之外的本机提醒。",
            details: &[
                "适合在本机旁边值守时使用。",
                "提醒内容保留币种、合约类型、净价差和机会等级。",
                "是否启用由配置里的 alert.macos.enabled 控制。",
            ],
        },
        ModuleExplanation {
            title: "Web 控制台",
            summary: "展示运行状态、活跃机会、最近机会、黑名单、静默币种和模块说明。",
            details: &[
                "页面每 5 秒自动刷新一次状态。",
                "当前 Web 控制台只展示和解释，不提供真实下单按钮。",
                "后续一键开仓会单独接入交易权限和风控检查。",
            ],
        },
        ModuleExplanation {
            title: "启动与托管",
            summary: "双击 .command 后会构建项目，并用 macOS LaunchAgent 手动托管本地服务。",
            details: &[
                "不会开机自动启动。",
                "关闭终端但选择保留服务时，LaunchAgent 会继续维持进程。",
                "运行时文件位于 ~/Library/Application Support/FutureQuant。",
            ],
        },
    ]
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Future Quant 本地控制台</title>
  <style>
    :root {
      color-scheme: light dark;
      --bg: #f7f7f4;
      --panel: #ffffff;
      --text: #18201d;
      --muted: #66706b;
      --line: #d9ded9;
      --accent: #0c7a65;
      --warn: #b45f06;
      --bad: #b42318;
      --soft: #eef5f1;
      --field: #fbfcfb;
    }
    @media (prefers-color-scheme: dark) {
      :root {
        --bg: #111413;
        --panel: #181d1b;
        --text: #edf2ef;
        --muted: #a8b0ab;
        --line: #2c3430;
        --accent: #4fb89f;
        --soft: #14211d;
        --field: #111614;
      }
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font: 14px/1.45 system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: var(--bg);
      color: var(--text);
    }
    header {
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 16px;
      padding: 18px 24px;
      border-bottom: 1px solid var(--line);
      background: var(--panel);
    }
    h1 { margin: 0; font-size: 20px; letter-spacing: 0; }
    .subtitle { margin-top: 4px; color: var(--muted); font-size: 13px; }
    main { padding: 18px 24px 32px; display: grid; gap: 18px; }
    section { background: var(--panel); border: 1px solid var(--line); border-radius: 8px; overflow: hidden; }
    .section-head { padding: 14px 16px; border-bottom: 1px solid var(--line); }
    .section-actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-top: 10px; }
    h2 { margin: 0; font-size: 15px; }
    .explain { margin: 6px 0 0; color: var(--muted); font-size: 13px; max-width: 980px; }
    .grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; padding: 16px; }
    .form-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; padding: 16px; }
    .metric { border: 1px solid var(--line); border-radius: 8px; padding: 12px; min-height: 72px; }
    .metric span { display: block; color: var(--muted); font-size: 12px; }
    .metric strong { display: block; margin-top: 6px; font-size: 18px; }
    label { display: grid; gap: 6px; color: var(--muted); font-size: 12px; }
    input, select, textarea {
      width: 100%;
      border: 1px solid var(--line);
      border-radius: 6px;
      background: var(--field);
      color: var(--text);
      padding: 8px 9px;
      font: inherit;
      letter-spacing: 0;
    }
    textarea { min-height: 120px; resize: vertical; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; line-height: 1.45; }
    button {
      border: 1px solid var(--line);
      border-radius: 6px;
      background: var(--panel);
      color: var(--text);
      padding: 7px 10px;
      font: inherit;
      cursor: pointer;
      white-space: nowrap;
    }
    button.primary { border-color: var(--accent); background: var(--accent); color: white; }
    button:disabled { cursor: not-allowed; opacity: .55; }
    table { width: 100%; border-collapse: collapse; }
    th, td { padding: 10px 12px; border-bottom: 1px solid var(--line); text-align: left; white-space: nowrap; }
    th { color: var(--muted); font-size: 12px; font-weight: 600; }
    td.risk { white-space: normal; min-width: 220px; }
    .ok { color: var(--accent); }
    .warn { color: var(--warn); }
    .bad { color: var(--bad); }
    .muted { color: var(--muted); }
    .empty { padding: 18px 16px; color: var(--muted); }
    .chips { display: flex; gap: 8px; flex-wrap: wrap; padding: 16px; }
    .chip { border: 1px solid var(--line); border-radius: 999px; padding: 4px 9px; color: var(--muted); }
    .module-grid, .audit-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; padding: 16px; }
    .module, .audit-card { border: 1px solid var(--line); border-radius: 8px; padding: 14px; background: var(--soft); }
    .module h3, .audit-card h3 { margin: 0 0 6px; font-size: 14px; }
    .module p, .audit-card p { margin: 0 0 8px; color: var(--muted); }
    .module ul { margin: 0; padding-left: 18px; color: var(--text); }
    .module li { margin: 4px 0; }
    .full { grid-column: 1 / -1; }
    .audit-output { padding: 16px; display: grid; gap: 12px; }
    .pre {
      white-space: pre-wrap;
      word-break: break-word;
      border: 1px solid var(--line);
      border-radius: 8px;
      background: var(--field);
      padding: 12px;
      margin: 0;
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 12px;
      line-height: 1.5;
    }
    .process { display: grid; gap: 8px; }
    .process-item { border: 1px solid var(--line); border-radius: 8px; padding: 10px; background: var(--field); }
    .process-item strong { display: block; margin-bottom: 4px; }
    .links { display: grid; gap: 6px; }
    .links a { color: var(--accent); overflow-wrap: anywhere; }
    @media (max-width: 900px) {
      .grid, .form-grid, .module-grid, .audit-grid { grid-template-columns: 1fr; }
      main, header { padding-left: 14px; padding-right: 14px; }
      .table-wrap { overflow-x: auto; }
    }
  </style>
</head>
<body>
  <header>
    <div>
      <h1>Future Quant 本地控制台</h1>
      <div class="subtitle">Binance / OKX 永续合约价差监控，当前为中文界面。</div>
    </div>
    <div id="updated" class="muted">正在加载...</div>
  </header>
  <main>
    <section>
      <div class="section-head">
        <h2>运行状态</h2>
        <p class="explain">用于判断本地服务是否正常连接两边交易所，以及每轮行情扫描是否足够快。</p>
      </div>
      <div class="grid">
        <div class="metric"><span>币安连接</span><strong id="binance">-</strong></div>
        <div class="metric"><span>OKX 连接</span><strong id="okx">-</strong></div>
        <div class="metric"><span>共同合约数量</span><strong id="common">-</strong></div>
        <div class="metric"><span>最近轮询耗时</span><strong id="poll">-</strong></div>
      </div>
    </section>
    <section>
      <div class="section-head">
        <h2>活跃机会</h2>
        <p class="explain">这里展示仍然处于跟踪中的价差机会。方向遵循“贵的一边做空，便宜的一边做多”。</p>
      </div>
      <div class="table-wrap">
        <table>
          <thead><tr><th>币种</th><th>合约类型</th><th>等级</th><th>方向</th><th>净价差</th><th>毛价差</th><th>最小深度</th><th>建议仓位</th><th>风险提示</th><th>审计</th></tr></thead>
          <tbody id="active"></tbody>
        </table>
      </div>
      <div id="active-empty" class="empty">暂无活跃机会。</div>
    </section>
    <section id="audit-section">
      <div class="section-head">
        <h2>闪电审计</h2>
        <p class="explain">接入 OpenAI 兼容 API，对出现的机会快速审计：标的是否等价、是否存在操纵/强控盘等风险、资金费率结算风险。审计过程和检索入口保持可见。</p>
        <div class="section-actions">
          <button id="refresh-models">刷新模型</button>
          <button id="save-audit" class="primary">保存配置</button>
          <span id="audit-status" class="muted">未加载</span>
        </div>
      </div>
      <div class="form-grid">
        <label>启用
          <select id="audit-enabled"><option value="true">启用</option><option value="false">关闭</option></select>
        </label>
        <label>OpenAI 兼容 Base URL
          <input id="audit-base-url" placeholder="https://api.openai.com/v1" />
        </label>
        <label>API Key 环境变量
          <input id="audit-api-key-env" placeholder="OPENAI_API_KEY" />
        </label>
        <label>模型
          <input id="audit-model" list="audit-models" placeholder="gpt-4.1-mini" />
          <datalist id="audit-models"></datalist>
        </label>
        <label>提示词模板
          <select id="audit-prompt-select"></select>
        </label>
        <label>显示过程日志
          <select id="audit-show-process"><option value="true">显示</option><option value="false">隐藏模型输入</option></select>
        </label>
        <label>请求超时秒数
          <input id="audit-timeout" type="number" min="5" max="180" />
        </label>
        <label>Temperature
          <input id="audit-temperature" type="number" min="0" max="2" step="0.1" />
        </label>
        <label>Max Tokens
          <input id="audit-max-tokens" type="number" min="256" max="8000" step="64" />
        </label>
        <label class="full">提示词名称
          <input id="audit-prompt-name" />
        </label>
        <label class="full">System Prompt
          <textarea id="audit-system-prompt"></textarea>
        </label>
        <label class="full">User Prompt 模板
          <textarea id="audit-user-template"></textarea>
        </label>
      </div>
      <div class="audit-output">
        <div class="audit-card">
          <h3>审计结论</h3>
          <p id="audit-verdict">尚未执行审计。</p>
          <pre id="audit-answer" class="pre">点击活跃机会里的“闪电审计”开始。</pre>
        </div>
        <div class="audit-grid">
          <div class="audit-card">
            <h3>可见联网入口</h3>
            <div id="audit-links" class="links muted">暂无</div>
          </div>
          <div class="audit-card">
            <h3>可见过程</h3>
            <div id="audit-process" class="process muted">暂无</div>
          </div>
        </div>
      </div>
    </section>
    <section>
      <div class="section-head">
        <h2>黑名单</h2>
        <p class="explain">黑名单币种不会参与扫描和报警，适合排除主流高深度币或你暂时不想监控的标的。</p>
      </div>
      <div id="blacklist" class="chips"></div>
    </section>
    <section>
      <div class="section-head">
        <h2>已静默币种</h2>
        <p class="explain">从 Telegram 里点“静默币种”后会出现在这里，后续不会继续推送该币种的新机会。</p>
      </div>
      <div id="muted" class="chips"></div>
    </section>
    <section>
      <div class="section-head">
        <h2>最近机会</h2>
        <p class="explain">用于回看最近触发或更新过的机会，辅助判断某个币种的 spread 是否持续存在。</p>
      </div>
      <div class="table-wrap">
        <table>
          <thead><tr><th>时间</th><th>币种</th><th>合约类型</th><th>净价差</th><th>建议仓位</th></tr></thead>
          <tbody id="recent"></tbody>
        </table>
      </div>
    </section>
    <section id="modules-section">
      <div class="section-head">
        <h2>功能模块说明</h2>
        <p class="explain">这里解释每个模块在整条监控链路里的职责，方便后续决定哪些部分可以升级到测试盘或实盘执行。</p>
      </div>
      <div id="modules" class="module-grid"></div>
    </section>
  </main>
  <script>
    let auditSettings = null;
    let currentPromptIndex = 0;
    const fmtPct = bps => `${(bps / 100).toFixed(2)}%`;
    const fmtUsd = n => `${Number(n).toFixed(2)}U`;
    const kindZh = { UsdtPerp: "U 本位永续", CoinPerp: "币本位永续", USDT_PERP: "U 本位永续", COIN_PERP: "币本位永续" };
    const levelZh = { WatchOnly: "仅观察", Normal: "正常机会", High: "高优先级", Extreme: "极端机会", WATCH_ONLY: "仅观察", NORMAL: "正常机会", HIGH: "高优先级", EXTREME: "极端机会" };
    const exchangeZh = { Binance: "币安", Okx: "OKX", OKX: "OKX" };
    const zh = (map, value) => map[value] || value;
    const el = id => document.getElementById(id);
    function text(id, value, cls) {
      const node = el(id);
      node.textContent = value;
      node.className = cls || "";
    }
    function row(cells) {
      const tr = document.createElement("tr");
      cells.forEach(cell => {
        const [value, cls] = Array.isArray(cell) ? cell : [cell, ""];
        const td = document.createElement("td");
        if (value instanceof Node) td.appendChild(value); else td.textContent = value;
        if (cls) td.className = cls;
        tr.appendChild(td);
      });
      return tr;
    }
    async function loadUi() {
      const res = await fetch("/api/ui");
      const ui = await res.json();
      document.documentElement.lang = ui.language || "zh-CN";
      el("modules-section").style.display = ui.module_explanations_enabled ? "block" : "none";
      const modules = el("modules");
      modules.innerHTML = "";
      ui.modules.forEach(module => {
        const card = document.createElement("article");
        card.className = "module";
        const title = document.createElement("h3");
        title.textContent = module.title;
        const summary = document.createElement("p");
        summary.textContent = module.summary;
        const list = document.createElement("ul");
        module.details.forEach(detail => {
          const item = document.createElement("li");
          item.textContent = detail;
          list.appendChild(item);
        });
        card.append(title, summary, list);
        modules.appendChild(card);
      });
    }
    async function loadAuditSettings() {
      const res = await fetch("/api/audit/settings");
      auditSettings = await res.json();
      renderAuditSettings();
      text("audit-status", "配置已加载", "muted");
    }
    function renderAuditSettings() {
      el("audit-enabled").value = String(auditSettings.enabled);
      el("audit-base-url").value = auditSettings.base_url;
      el("audit-api-key-env").value = auditSettings.api_key_env;
      el("audit-model").value = auditSettings.selected_model;
      el("audit-timeout").value = auditSettings.request_timeout_secs;
      el("audit-temperature").value = auditSettings.temperature;
      el("audit-max-tokens").value = auditSettings.max_tokens;
      el("audit-show-process").value = String(auditSettings.show_process_log);
      const select = el("audit-prompt-select");
      select.innerHTML = "";
      auditSettings.prompts.forEach((prompt, index) => {
        const option = document.createElement("option");
        option.value = String(index);
        option.textContent = prompt.name;
        select.appendChild(option);
      });
      currentPromptIndex = Math.max(0, auditSettings.prompts.findIndex(p => p.id === auditSettings.selected_prompt_id));
      select.value = String(currentPromptIndex);
      renderPromptEditor();
    }
    function renderPromptEditor() {
      const prompt = auditSettings.prompts[currentPromptIndex];
      if (!prompt) return;
      el("audit-prompt-name").value = prompt.name;
      el("audit-system-prompt").value = prompt.system_prompt;
      el("audit-user-template").value = prompt.user_template;
    }
    function collectAuditSettings() {
      const prompts = auditSettings.prompts.map((prompt, index) => {
        if (index !== currentPromptIndex) return prompt;
        return {
          ...prompt,
          name: el("audit-prompt-name").value,
          system_prompt: el("audit-system-prompt").value,
          user_template: el("audit-user-template").value,
        };
      });
      const selected = prompts[currentPromptIndex] || prompts[0];
      return {
        enabled: el("audit-enabled").value === "true",
        base_url: el("audit-base-url").value,
        api_key_env: el("audit-api-key-env").value,
        selected_model: el("audit-model").value,
        selected_prompt_id: selected.id,
        request_timeout_secs: Number(el("audit-timeout").value || 45),
        temperature: Number(el("audit-temperature").value || 0.1),
        max_tokens: Number(el("audit-max-tokens").value || 1200),
        show_process_log: el("audit-show-process").value === "true",
        prompts,
      };
    }
    async function saveAuditSettings() {
      text("audit-status", "正在保存...", "muted");
      const res = await fetch("/api/audit/settings", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(collectAuditSettings()),
      });
      const payload = await res.json();
      if (!payload.ok) {
        text("audit-status", payload.error || "保存失败", "bad");
        return;
      }
      auditSettings = payload.data;
      renderAuditSettings();
      text("audit-status", "已保存到本地", "ok");
    }
    async function refreshModels() {
      text("audit-status", "正在刷新模型...", "muted");
      await saveAuditSettings();
      const res = await fetch("/api/audit/models", { method: "POST" });
      const payload = await res.json();
      const list = el("audit-models");
      list.innerHTML = "";
      payload.models.forEach(model => {
        const option = document.createElement("option");
        option.value = model;
        list.appendChild(option);
      });
      renderProcess(payload.process || []);
      text("audit-status", `模型列表已刷新：${payload.models.length} 个`, "ok");
    }
    async function runAudit(id) {
      text("audit-status", "正在执行闪电审计...", "muted");
      el("audit-answer").textContent = "审计中...";
      el("audit-verdict").textContent = "等待模型返回";
      renderProcess([{ label: "提交审计", detail: `机会 ID：${id}` }]);
      const res = await fetch("/api/audit/run", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ opportunity_id: id }),
      });
      const payload = await res.json();
      if (!payload.ok) {
        text("audit-status", payload.error || "审计失败", "bad");
        el("audit-answer").textContent = payload.error || "审计失败";
        return;
      }
      const result = payload.data;
      el("audit-verdict").textContent = result.verdict;
      el("audit-answer").textContent = result.answer;
      renderProcess(result.process || []);
      renderLinks(result.evidence_links || []);
      text("audit-status", `审计完成：${result.model}`, "ok");
      el("audit-section").scrollIntoView({ behavior: "smooth", block: "start" });
    }
    function renderProcess(items) {
      const box = el("audit-process");
      box.innerHTML = "";
      if (!items.length) {
        box.textContent = "暂无";
        return;
      }
      items.forEach(item => {
        const div = document.createElement("div");
        div.className = "process-item";
        const strong = document.createElement("strong");
        strong.textContent = item.label;
        const pre = document.createElement("pre");
        pre.className = "pre";
        pre.textContent = item.detail;
        div.append(strong, pre);
        box.appendChild(div);
      });
    }
    function renderLinks(links) {
      const box = el("audit-links");
      box.innerHTML = "";
      if (!links.length) {
        box.textContent = "暂无";
        return;
      }
      links.forEach(link => {
        const a = document.createElement("a");
        a.href = link.url;
        a.target = "_blank";
        a.rel = "noreferrer";
        a.textContent = `${link.title}: ${link.url}`;
        box.appendChild(a);
      });
    }
    async function refresh() {
      const res = await fetch("/api/state");
      const state = await res.json();
      el("updated").textContent = `更新时间 ${new Date(state.updated_at).toLocaleTimeString()}`;
      text("binance", state.status.binance_ok ? "正常" : "异常", state.status.binance_ok ? "ok" : "bad");
      text("okx", state.status.okx_ok ? "正常" : "异常", state.status.okx_ok ? "ok" : "bad");
      text("common", state.stats.discovered_common_symbols);
      text("poll", state.status.last_poll_ms == null ? "-" : `${state.status.last_poll_ms} ms`);

      const active = el("active");
      active.innerHTML = "";
      el("active-empty").style.display = state.active_opportunities.length ? "none" : "block";
      state.active_opportunities.forEach(o => {
        const auditButton = document.createElement("button");
        auditButton.textContent = "闪电审计";
        auditButton.onclick = () => runAudit(o.id);
        active.appendChild(row([
          [o.base],
          [zh(kindZh, o.kind)],
          [zh(levelZh, o.alert_level)],
          [`做空 ${zh(exchangeZh, o.short_exchange)} / 做多 ${zh(exchangeZh, o.long_exchange)}`],
          [fmtPct(o.estimated_net_spread_bps), "ok"],
          [fmtPct(o.gross_spread_bps)],
          [fmtUsd(o.min_side_depth_usdt)],
          [fmtUsd(o.suggested_notional_usdt)],
          [o.risk_flags.length ? o.risk_flags.join("; ") : "暂无", "risk"],
          [auditButton],
        ]));
      });

      const blacklist = el("blacklist");
      blacklist.innerHTML = "";
      state.blacklisted_symbols.forEach(s => {
        const chip = document.createElement("span");
        chip.className = "chip";
        chip.textContent = s;
        blacklist.appendChild(chip);
      });

      const muted = el("muted");
      muted.innerHTML = "";
      state.muted_symbols.forEach(s => {
        const chip = document.createElement("span");
        chip.className = "chip";
        chip.textContent = s;
        muted.appendChild(chip);
      });

      const recent = el("recent");
      recent.innerHTML = "";
      state.recent_opportunities.slice(-30).reverse().forEach(o => {
        recent.appendChild(row([
          [new Date(o.updated_at).toLocaleTimeString()],
          [o.base],
          [zh(kindZh, o.kind)],
          [fmtPct(o.estimated_net_spread_bps)],
          [fmtUsd(o.suggested_notional_usdt)]
        ]));
      });
    }
    el("audit-prompt-select").addEventListener("change", event => {
      if (!auditSettings) return;
      auditSettings.prompts[currentPromptIndex] = collectAuditSettings().prompts[currentPromptIndex];
      currentPromptIndex = Number(event.target.value || 0);
      auditSettings.selected_prompt_id = auditSettings.prompts[currentPromptIndex]?.id || auditSettings.selected_prompt_id;
      renderPromptEditor();
    });
    el("save-audit").addEventListener("click", saveAuditSettings);
    el("refresh-models").addEventListener("click", refreshModels);
    loadUi();
    loadAuditSettings();
    refresh();
    setInterval(refresh, 5000);
  </script>
</body>
</html>"#;
