(() => {
const req = async (method, path, body) => {
  const r = await fetch(path, {method, headers: body ? {"content-type": "application/json"} : {}, body: body ? JSON.stringify(body) : undefined});
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(typeof j.detail === "string" ? j.detail : `请求失败（${r.status}）`);
  return j;
};
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;"}[c]));
const num = v => v == null || isNaN(v) ? "—" : usd(v);
const sg = v => v == null ? "—" : `<span class="${v >= 0 ? "pos" : "neg"}">${signed(v, usd)}</span>`;
const sp = v => v == null ? "—" : `<span class="${v >= 0 ? "pos" : "neg"}">${signed(v, x => pct(x, 0))}</span>`;
const ago = t => { const d = (Date.now() / 1000 - t) / 86400; return d < 1 ? `${Math.max(1, Math.round(d * 24))} 小时` : `${d.toFixed(d < 10 ? 1 : 0)} 天`; };
/* 股票/BNB 仓位：价格按 BNB，金额同时显示 U 和 BNB */
const bnbPos = r => r.quote_sym === "BNB";
const pq = (v, r) => bnbPos(r) ? (+v).toPrecision(5) + " BNB" : usd(v);
const bn = v => v == null || isNaN(v) ? "—" : (+v).toFixed(4) + " BNB";
const sbn = v => v == null ? "" : `<span class="${v >= 0 ? "pos" : "neg"}">${v >= 0 ? "+" : "−"}${Math.abs(v).toFixed(4)} BNB</span>`;
const dual = (r, main, q) => bnbPos(r) && r.summary_q ? `${main}<small class="pv-q">${q}</small>` : main;
const tickerOf = pool => Object.entries(POOLS).find(([, a]) => a === pool)?.[0];
const form = () => ({pool: $("pool").value.trim(), lo: +$("lo").value || null, hi: +$("hi").value || null,
  capital: +$("cap").value || 1000, ranges: $("ranges").value, shift: +$("shift").value || 0.5,
  days: +$("days").value || 7, market_hours: $("mh").checked, hedge: $("hedge").value});

/* ---------- 侧栏骨架：计算 / 追踪 ---------- */
const sticky = document.querySelector("aside .sticky");
const calc = document.createElement("div"); calc.id = "pv-calc";
while (sticky.firstChild) calc.appendChild(sticky.firstChild);
sticky.insertAdjacentHTML("afterbegin", `<div class="seg pv-mode" role="tablist" aria-label="模式">
  <button type="button" role="tab" data-m="calc">计算</button><button type="button" role="tab" data-m="track">追踪</button></div>`);
sticky.appendChild(calc);
sticky.insertAdjacentHTML("beforeend", `<div id="pv-track" hidden>
  <fieldset><legend>导入仓位</legend>
    <form id="pv-import"><textarea id="pv-ids" rows="2" spellcheck="false" placeholder="NFT 编号，逗号或换行分隔，最多 10 个"></textarea>
    <button type="submit" class="run" id="pv-imp-go">导入</button></form>
    <div id="pv-imp-out"></div></fieldset>
  <fieldset><legend class="pv-legend">我的仓位 <span id="pv-total" class="dim"></span></legend>
    <div id="pv-list" class="dim">读取中…</div>
    <div class="pv-foot"><button type="button" class="pv-link" id="pv-closed" aria-pressed="false">显示已平仓</button>
      <button type="button" class="pv-link" id="pv-refresh">刷新</button></div></fieldset>
</div>`);
document.querySelector("main").insertAdjacentHTML("afterbegin", `<section id="pv-pos" class="pv-pos" hidden style="margin-top:0"></section>`);

calc.insertAdjacentHTML("afterbegin", `<fieldset class="pv-presets"><legend>我的预设</legend>
  <div class="pv-row"><select id="pv-sel" aria-label="选择预设"><option value="">— 选择预设 —</option></select>
  <button type="button" class="pv-btn" id="pv-save">保存</button>
  <button type="button" class="pv-btn" id="pv-del" disabled>删除</button></div>
  <p class="hint" id="pv-msg"></p></fieldset>`);

let mode = null, rows = [], sel = localStorage.getItem("rwalp.sel"), showClosed = false, calcState = null;
function setMode(m) {
  if (m === mode) return;
  if (m === "track" && mode === "calc") calcState = form();
  const was = mode; mode = m; localStorage.setItem("rwalp.mode", m);
  document.querySelectorAll(".pv-mode button").forEach(b => b.setAttribute("aria-selected", b.dataset.m === m));
  $("pv-calc").hidden = m !== "calc"; $("pv-track").hidden = m !== "track"; $("pv-pos").hidden = m !== "track";
  document.body.dataset.mode = m;
  if (m === "track") { const r = rows.find(x => x.id === sel); if (r) select(r.id, true); }
  else if (was && calcState) { restore(calcState); calcState = null; }
}
function restore(f) {
  $("pool").value = f.pool; $("lo").value = f.lo ?? ""; $("hi").value = f.hi ?? ""; $("cap").value = f.capital;
  $("ranges").value = f.ranges; $("shift").value = f.shift; $("days").value = f.days; $("mh").checked = f.market_hours;
  $("hedge").value = f.hedge; $("hedge").dispatchEvent(new Event("change")); syncTickers(); check();
}
document.querySelector(".pv-mode").onclick = e => { const b = e.target.closest("button"); if (b) setMode(b.dataset.m); };

/* ---------- 预设（计算模式） ---------- */
let presets = [];
const msg = (t, bad) => { const m = $("pv-msg"); m.textContent = t; m.className = "hint" + (bad ? " err" : ""); };
async function loadPresets(id) {
  presets = await req("GET", "/p/presets");
  $("pv-sel").innerHTML = `<option value="">— 选择预设 —</option>` + presets.map(p => `<option value="${p.id}">${esc(p.name)}</option>`).join("");
  $("pv-sel").value = id || ""; $("pv-del").disabled = !$("pv-sel").value;
}
$("pv-sel").onchange = () => {
  const p = presets.find(x => x.id === $("pv-sel").value); $("pv-del").disabled = !p;
  if (p) { restore(p); msg(`已切换到「${p.name}」`); }
};
$("pv-save").onclick = async () => {
  const cur = presets.find(x => x.id === $("pv-sel").value), f = form();
  const t = tickerOf(f.pool.toLowerCase()) || f.pool.slice(0, 6);
  const name = prompt(cur ? "保存为（同名则覆盖当前预设）" : "预设名称", cur ? cur.name : `${t} ${f.lo ?? ""}–${f.hi ?? ""}`.trim());
  if (!name) return;
  try {
    const body = {...f, name: name.slice(0, 40)};
    const {id} = cur && cur.name === name ? await req("PUT", `/p/presets/${cur.id}`, body) : await req("POST", "/p/presets", body);
    await loadPresets(id); msg(`已保存「${name}」`);
  } catch (e) { msg(e.message, true); }
};
$("pv-del").onclick = async () => {
  const p = presets.find(x => x.id === $("pv-sel").value);
  if (!p || !confirm(`删除预设「${p.name}」？`)) return;
  try { await req("DELETE", `/p/presets/${p.id}`); await loadPresets(); msg(`已删除「${p.name}」`); } catch (e) { msg(e.message, true); }
};

/* ---------- 追踪：仓位列表 ---------- */
function drawList() {
  const vis = rows.filter(r => showClosed || r.status !== "closed");
  const live = rows.filter(r => r.status !== "closed"), s = k => live.reduce((a, r) => a + (r.summary[k] || 0), 0);
  $("pv-total").textContent = live.length ? `${live.length}/10 · ${usd(s("value"), 0)} · ${signed(s("pnl"), usd)}` : "";
  if (!vis.length) { $("pv-list").className = "empty pv-empty"; $("pv-list").textContent = "还没有仓位。在上面输入 NFT 编号导入。"; return; }
  $("pv-list").className = "pv-cards";
  $("pv-list").innerHTML = vis.map(r => { const s = r.summary, p = r.live?.price;
    const st = r.status === "closed" ? "closed" : p == null ? "err" : r.in_range ? "in" : "out";
    return `<button type="button" class="pv-card" data-id="${r.id}" aria-pressed="${r.id === sel}">
      <span class="pv-card-name"><span class="dot pv-st-${st}"></span>${esc(r.name)}</span><b>${num(s.value)}${bnbPos(r) && r.summary_q ? ` <small class="dim">${bn(r.summary_q.value)}</small>` : ""}</b>
      <span class="dim">${pq(r.lo, r)} – ${pq(r.hi, r)}</span><span>${sg(s.pnl)}${bnbPos(r) && r.summary_q ? ` ${sbn(r.summary_q.pnl)}` : ""}</span></button>`; }).join("");
}
async function loadPositions() {
  try {
    rows = await req("GET", "/p/positions"); drawList();
    if (mode === "track") {
      const r = rows.find(x => x.id === sel) || rows.find(x => x.status !== "closed");
      if (r) select(r.id, r.id !== $("pv-pos").dataset.id); else $("pv-pos").innerHTML = "";
    }
  } catch (e) { $("pv-list").innerHTML = `<p class="err">${esc(e.message)}</p>`; }
}
$("pv-list").addEventListener("click", e => { const c = e.target.closest(".pv-card"); if (c) select(c.dataset.id, true); });
$("pv-closed").onclick = () => { showClosed = !showClosed; $("pv-closed").setAttribute("aria-pressed", showClosed); $("pv-closed").textContent = showClosed ? "隐藏已平仓" : "显示已平仓"; drawList(); };
$("pv-refresh").onclick = loadPositions;

$("pv-import").onsubmit = async e => {
  e.preventDefault();
  const ids = [...new Set(($("pv-ids").value.match(/\d+/g) || []).map(Number))];
  const out = $("pv-imp-out");
  if (!ids.length) { out.innerHTML = `<p class="hint err">请输入至少一个编号</p>`; return; }
  if (ids.length > 10) { out.innerHTML = `<p class="hint err">一次最多 10 个</p>`; return; }
  const b = $("pv-imp-go"); busy(b, true, `读取 ${ids.length} 个仓位…`);
  try {
    const res = await req("POST", "/p/import", {token_ids: ids});
    out.innerHTML = `<ul class="pv-imp-res">${res.map(r => r.ok
      ? `<li><span class="dot d-ok"></span>#${r.token_id} ${esc(r.name)}</li>`
      : `<li><span class="dot d-bad"></span>#${r.token_id}：${esc(r.error)}</li>`).join("")}</ul>`;
    const first = res.find(r => r.ok);
    if (first) { $("pv-ids").value = res.filter(r => !r.ok).map(r => r.token_id).join(", "); sel = first.id; await loadPositions(); }
  } catch (err) { out.innerHTML = `<p class="hint err">${esc(err.message)}</p>`; }
  busy(b, false, "导入");
};

/* ---------- 追踪：选中仓位 → 主区按实际仓位计算 ---------- */
const TYPES = {deposit: "投入", withdraw: "提取", collect: "领取手续费", cost: "成本", snapshot: "记录价值"};
function select(id, reanalyze) {
  const r = rows.find(x => x.id === id); if (!r) return;
  sel = id; localStorage.setItem("rwalp.sel", id); drawList();
  if (reanalyze && mode === "track") {
    $("pool").value = r.pool; $("lo").value = +(+r.lo).toPrecision(6); $("hi").value = +(+r.hi).toPrecision(6);
    $("cap").value = Math.max(1, Math.round(r.summary.value || r.summary.deposited || 1000));
    syncTickers(); check();
  }
  drawPos(r);
}
/* 盈亏归因：把盈亏拆成 股价方向 / 波动损失 / 手续费 / 成本（BNB 池再加 BNB 涨跌），并给出扣掉发行方风险后的手续费年化 */
function attrHTML(r) {
  const a = r.attribution, s = r.summary; if (!a) return "";
  const parts = [["direction", bnbPos(r) ? "股价方向（相对 BNB）" : "股价方向", "var(--ice)"], ["quote", "BNB 涨跌", "#b9a3ff"],
    ["il", "波动损失", "var(--out)"], ["fees", "手续费", "var(--in)"], ["cost", "成本", "var(--risk)"]].filter(p => a[p[0]] != null && Math.abs(a[p[0]]) >= 0.005);
  const tot = parts.reduce((x, p) => x + Math.abs(a[p[0]]), 0) || 1;
  const bar = parts.map(p => `<i style="flex:${(Math.abs(a[p[0]]) / tot).toFixed(4)};background:${p[2]};opacity:${a[p[0]] >= 0 ? 1 : .45}" title="${p[1]} ${signed(a[p[0]], usd)}"></i>`).join("");
  const leg = parts.map(p => `<span><b style="background:${p[2]}"></b>${p[1]} ${sg(a[p[0]])}</span>`).join("");
  const rp = Math.min(Math.max(+($("risk")?.value ?? 5), 0), 100) / 100;
  const adj = s.fee_apr == null ? null : (1 - rp) * (1 + s.fee_apr) - 1;
  const risk = adj == null ? "" : `<span class="dim">手续费年化 ${pct(s.fee_apr, 0)}，按发行方每年 ${pct(rp, 0)} 出事概率扣减后 <b class="${adj >= 0 ? "pos" : "neg"}">${pct(adj, 0)}</b></span>`;
  return `<div class="pv-attr"><div class="pv-attr-head"><span>盈亏归因 <b>${sg(s.pnl)}</b></span>${risk}</div>
    <div class="pv-attr-bar">${bar}</div><div class="pv-attr-leg">${leg}</div></div>`;
}
function drawPos(r) {
  const s = r.summary, Q = r.summary_q || {}, box = $("pv-pos"); box.dataset.id = r.id;
  const src = r.live?.nft ? `链上实时（NFT #${r.token_id}）` : s.snapshot_at ? `最近记录 ${ago(s.snapshot_at)}前` : "还没有价值记录";
  box.innerHTML = `<div class="pv-pos-head"><h2>${esc(r.name)}</h2>
      <span class="dim">${pq(r.lo, r)} – ${pq(r.hi, r)}${bnbPos(r) ? ` · BNB ${usd(r.qusd)}` : ""} · ${src}${r.status === "closed" ? " · 已平仓" : ""}</span>
      <span class="pv-actions">${r.token_id && r.status !== "closed" ? `<button type="button" class="pv-btn" data-a="sync">同步链上</button>` : ""}
        <button type="button" class="pv-btn" data-a="bt">回测这个区间</button>
        <button type="button" class="pv-btn" data-a="log" aria-expanded="false">记录与设置</button></span></div>
    <dl class="pv-kpi">
      <div><dt>投入</dt><dd>${dual(r, usd(s.deposited), bn(Q.deposited))}</dd></div>
      <div><dt>当前价值</dt><dd>${dual(r, num(s.value), bn(Q.value))}</dd></div>
      <div><dt>手续费 <span class="dim">· 记账后</span></dt><dd>${dual(r, usd(s.fees), bn(Q.fees))}${s.fees_before ? `<small class="dim" title="导入时仓位里已有的未领手续费，不计入收益"> 另有导入前 ${usd(s.fees_before)}</small>` : ""}</dd></div>
      <div><dt>盈亏</dt><dd>${dual(r, sg(s.pnl), sbn(Q.pnl))}</dd></div>
      <div><dt title="(当前价值 + 已提取 + 手续费 − 投入 − 成本)，含标的涨跌和无常损失">实际年化</dt><dd>${dual(r, sp(s.apr), "BNB 本位 " + sp(Q.apr))}</dd></div>
      <div><dt title="只算记账后赚到的手续费">手续费年化</dt><dd>${s.fee_apr == null ? "—" : pct(s.fee_apr, 0)}</dd></div>
      <div><dt>相对持币</dt><dd>${dual(r, sg(s.vs_hodl), sbn(Q.vs_hodl))}</dd></div>
      <div><dt>时长</dt><dd>${s.days.toFixed(1)} 天</dd></div>
    </dl>
    ${attrHTML(r)}
    <p class="note">${bnbPos(r) ? "股票/BNB 仓位：上面每格第一行是 U 本位，第二行是 BNB 本位（BNB 涨跌不算盈亏）。" : ""}下方的深度图、区间评级、收益口径和调仓成本，都按这个仓位的实际区间和当前价值 ${num(s.value ?? s.deposited)} 计算。${s.days < 1 ? "持有不足 1 天，暂不显示年化。" : ""}</p>
    <div id="pv-log" hidden></div>`;
}
$("pv-pos").addEventListener("click", async e => {
  const b = e.target.closest("[data-a]"); if (!b) return;
  const r = rows.find(x => x.id === $("pv-pos").dataset.id); if (!r) return;
  if (b.dataset.a === "sync") {
    busy(b, true, "同步中，约 30 秒…");
    try { await req("POST", `/p/positions/${r.id}/sync`); await loadPositions(); } catch (err) { alert(err.message); busy(b, false, "同步链上"); }
  }
  if (b.dataset.a === "bt") {
    const p = r.live?.price || poolData?.pool?.price; if (!p) return;
    $("ranges").value = `${((r.lo / p - 1) * 100).toFixed(2)}:${((r.hi / p - 1) * 100).toFixed(2)}`;
    backtest(); $("btsec").scrollIntoView({behavior: "smooth", block: "start"});
  }
  if (b.dataset.a === "log") {
    const L = $("pv-log"), o = L.hidden; L.hidden = !o; b.setAttribute("aria-expanded", o); if (o) drawLog(r);
  }
});
async function drawLog(r) {
  const L = $("pv-log"); L.innerHTML = `<span class="dim">读取记录…</span>`;
  const evs = await req("GET", `/p/positions/${r.id}/events`).catch(() => []);
  const now = new Date(Date.now() - new Date().getTimezoneOffset() * 6e4).toISOString().slice(0, 16);
  L.innerHTML = `<div class="pv-detail-grid">
    <form id="pv-ev" class="pv-form">
      <select name="type">${Object.entries(TYPES).map(([k, v]) => `<option value="${k}">${v}</option>`).join("")}</select>
      <input name="stock" type="number" step="any" placeholder="股票数量"><input name="stable" type="number" step="any" placeholder="${bnbPos(r) ? "BNB 数量" : "稳定币"}">
      <input name="value" type="number" step="any" placeholder="总价值 $"><input name="fees" type="number" step="any" placeholder="手续费 $">
      <input name="cost" type="number" step="any" placeholder="成本 $"><input name="ts" type="datetime-local" value="${now}">
      <input name="note" type="text" placeholder="备注" maxlength="300"><button type="submit" class="pv-btn pv-primary">记一笔</button>
    </form>
    <p class="hint pv-hint" id="pv-hint"></p>
    <div class="pv-settings">
      <label>名称 <input id="pv-name" type="text" value="${esc(r.name)}" maxlength="40"></label>
      <label>NFT 编号 <input id="pv-nft" type="number" value="${r.token_id ?? ""}"></label>
      <label>备注 <input id="pv-memo" type="text" value="${esc(r.note)}" maxlength="500"></label>
      <button type="button" class="pv-btn" id="pv-upd">保存设置</button>
      <button type="button" class="pv-btn" id="pv-st">${r.status === "closed" ? "重新打开" : "标记平仓"}</button>
      <button type="button" class="pv-btn pv-danger" id="pv-rm">删除仓位</button>
    </div>
    ${evs.length ? `<div class="scroll"><table class="pv-events"><tr><th>时间</th><th>类型</th><th>股票</th><th>稳定币</th><th>价值</th><th>手续费</th><th>成本</th><th>备注</th><th></th></tr>` +
      evs.slice().reverse().map(e => `<tr><td>${new Date(e.ts * 1000).toLocaleString("zh-CN", {hour12: false, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit"})}</td><td>${TYPES[e.type]}</td>
        <td>${e.stock ? e.stock.toFixed(4) : ""}</td><td>${e.stable ? usd(e.stable) : ""}</td><td>${e.value ? usd(e.value) : ""}</td>
        <td>${e.fees ? usd(e.fees) : ""}</td><td>${e.cost ? usd(e.cost) : ""}</td><td class="dim">${esc(e.note)}</td>
        <td><button type="button" class="pv-x" data-ev="${e.id}" aria-label="删除这条记录">×</button></td></tr>`).join("") + `</table></div>` : `<p class="dim">还没有记录。</p>`}
  </div>`;
  const hint = {deposit: "填投入的股票数量和稳定币；总价值留空按现价计算", withdraw: "填取出的股票数量和稳定币；全部撤出后点「标记平仓」",
    collect: "填领取的手续费美元价值", cost: "填 gas、swap 手续费等美元成本", snapshot: "填仓位当前总价值和未领取手续费（有 NFT 编号时直接点「同步链上」）"};
  const f = $("pv-ev"), show = () => { $("pv-hint").textContent = hint[f.type.value]; $("pv-hint").className = "hint pv-hint";
    const need = {deposit: ["stock", "stable", "value"], withdraw: ["stock", "stable", "value"], collect: ["fees"], cost: ["cost"], snapshot: ["value", "fees"]}[f.type.value];
    ["stock", "stable", "value", "fees", "cost"].forEach(n => f[n].hidden = !need.includes(n)); };
  f.type.onchange = show; show();
  const reopen = async () => {
    await loadPositions();
    const nr = rows.find(x => x.id === r.id); if (!nr) return;
    $("pv-log").hidden = false; document.querySelector('#pv-pos [data-a="log"]')?.setAttribute("aria-expanded", true); drawLog(nr);
  };
  f.onsubmit = async ev => { ev.preventDefault();
    const d = {type: f.type.value, note: f.note.value, ts: f.ts.value ? new Date(f.ts.value).getTime() / 1000 : null};
    ["stock", "stable", "value", "fees", "cost"].forEach(n => { if (!f[n].hidden && f[n].value) d[n] = +f[n].value; });
    try { await req("POST", `/p/positions/${r.id}/events`, d); await reopen(); } catch (e) { $("pv-hint").textContent = e.message; $("pv-hint").className = "hint err"; } };
  L.querySelectorAll("[data-ev]").forEach(b => b.onclick = async () => { if (!confirm("删除这条记录？")) return; await req("DELETE", `/p/events/${b.dataset.ev}`); reopen(); });
  $("pv-upd").onclick = async () => { await req("PATCH", `/p/positions/${r.id}`, {name: $("pv-name").value.trim() || r.name,
    token_id: $("pv-nft").value ? +$("pv-nft").value : null, note: $("pv-memo").value}); reopen(); };
  $("pv-st").onclick = async () => { await req("PATCH", `/p/positions/${r.id}`, {status: r.status === "closed" ? "open" : "closed"}); reopen(); };
  $("pv-rm").onclick = async () => { if (!confirm(`删除仓位「${r.name}」和它的全部记录？`)) return;
    await req("DELETE", `/p/positions/${r.id}`); sel = null; localStorage.removeItem("rwalp.sel"); $("pv-pos").innerHTML = ""; delete $("pv-pos").dataset.id; loadPositions(); };
}

/* ---------- 自选标的：替换公开版写死的快捷按钮 ---------- */
let tks = [];
const tkBox = $("tickers");
tkBox.insertAdjacentHTML("afterend", `<form id="tk-add" class="tk-add" hidden>
  <input id="tk-pool" type="text" spellcheck="false" autocomplete="off" placeholder="池子合约地址 0x…" aria-label="池子合约地址">
  <input id="tk-sym" type="text" maxlength="12" placeholder="名称（可选）" aria-label="名称">
  <button type="submit" class="pv-btn pv-primary" id="tk-go">添加</button></form><p class="hint" id="tk-msg"></p>`);
function drawTickers() {
  for (const k of Object.keys(POOLS)) delete POOLS[k];
  tks.forEach(t => POOLS[t.symbol] = t.pool);
  tkBox.innerHTML = tks.map(t => `<span class="tk"><button type="button" data-a="${t.pool}" title="${esc(t.pool)}">${esc(t.symbol)}</button>`
    + `<button type="button" class="tk-x" data-rm="${t.id}" aria-label="移除 ${esc(t.symbol)}">×</button></span>`).join("")
    + (tks.length < 10 ? `<button type="button" class="tk-plus" id="tk-plus" aria-expanded="${!$("tk-add").hidden}" title="按池子地址添加">＋</button>` : "")
    + `<span class="tk-count">${tks.length}/10</span>`;
  syncTickers();
  if (typeof board === "function") board();   // 排行榜跟着自选列表走
}
const tkMsg = (t, bad) => { $("tk-msg").textContent = t; $("tk-msg").className = "hint" + (bad ? " err" : ""); };
async function loadTickers() { try { tks = await req("GET", "/p/tickers"); drawTickers(); } catch (e) { tkMsg(e.message, true); } }
tkBox.addEventListener("click", async e => {
  const rm = e.target.closest("[data-rm]"), plus = e.target.closest("#tk-plus");
  if (plus) { const f = $("tk-add"); f.hidden = !f.hidden; plus.setAttribute("aria-expanded", !f.hidden); if (!f.hidden) { $("tk-pool").value = $("pool").value.trim(); $("tk-pool").focus(); } return; }
  if (!rm) return;
  e.stopPropagation();
  const t = tks.find(x => x.id === rm.dataset.rm);
  if (!t || !confirm(`从列表移除 ${t.symbol}？（不影响任何仓位）`)) return;
  try { await req("DELETE", `/p/tickers/${t.id}`); tks = tks.filter(x => x.id !== t.id); drawTickers(); tkMsg(`已移除 ${t.symbol}`); } catch (err) { tkMsg(err.message, true); }
}, true);
$("tk-add").onsubmit = async e => {
  e.preventDefault();
  const pool = $("tk-pool").value.trim(), sym = $("tk-sym").value.trim();
  if (!/^0x[0-9a-fA-F]{40}$/.test(pool)) { tkMsg("池子地址格式不对，应为 0x 开头的 42 位地址", true); return; }
  const b = $("tk-go"); busy(b, true, "读取中…");
  try {
    const t = await req("POST", "/p/tickers", {pool, symbol: sym || null});
    tks.push(t); drawTickers(); $("tk-add").hidden = true; $("tk-pool").value = $("tk-sym").value = "";
    tkMsg(`已添加 ${t.symbol}（${t.dex} ${(t.fee / 1e4).toFixed(2)}%${t.bstocks ? "" : "，非 bStocks 代币"}）`);
    $("pool").value = t.pool; $("lo").value = $("hi").value = ""; syncTickers(); check();
  } catch (err) { tkMsg(err.message, true); }
  busy(b, false, "添加");
};

$("risk")?.addEventListener("input", () => { const r = rows.find(x => x.id === sel); if (r && mode === "track") drawPos(r); });
setMode(localStorage.getItem("rwalp.mode") === "track" ? "track" : "calc");
loadTickers();
loadPresets().catch(e => msg(e.message, true));
loadPositions();
})();
