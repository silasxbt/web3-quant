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
const tickerOf = pool => Object.entries(POOLS).find(([, a]) => a === pool)?.[0];

/* ---------- 预设 ---------- */
let presets = [];
const form = () => ({pool: $("pool").value.trim(), lo: +$("lo").value || null, hi: +$("hi").value || null,
  capital: +$("cap").value || 1000, ranges: $("ranges").value, shift: +$("shift").value || 0.5,
  days: +$("days").value || 7, market_hours: $("mh").checked, hedge: $("hedge").value});
const sticky = document.querySelector("aside .sticky");
sticky.insertAdjacentHTML("afterbegin", `<fieldset class="pv-presets"><legend>我的预设</legend>
  <div class="pv-row"><select id="pv-sel" aria-label="选择预设"><option value="">— 选择预设 —</option></select>
  <button type="button" class="pv-btn" id="pv-save" title="保存当前参数">保存</button>
  <button type="button" class="pv-btn" id="pv-del" title="删除所选预设" disabled>删除</button></div>
  <p class="hint" id="pv-msg"></p></fieldset>`);
const msg = (t, bad) => { const m = $("pv-msg"); m.textContent = t; m.className = "hint" + (bad ? " err" : ""); };
async function loadPresets(sel) {
  presets = await req("GET", "/p/presets");
  $("pv-sel").innerHTML = `<option value="">— 选择预设 —</option>` + presets.map(p =>
    `<option value="${p.id}">${esc(p.name)}</option>`).join("");
  $("pv-sel").value = sel || ""; $("pv-del").disabled = !$("pv-sel").value;
}
function applyPreset(p) {
  $("pool").value = p.pool; $("lo").value = p.lo ?? ""; $("hi").value = p.hi ?? ""; $("cap").value = p.capital;
  $("ranges").value = p.ranges; $("shift").value = p.shift; $("days").value = p.days; $("mh").checked = p.market_hours;
  $("hedge").value = p.hedge; $("hedge").dispatchEvent(new Event("change"));
  syncTickers(); check();
}
$("pv-sel").onchange = () => {
  const p = presets.find(x => x.id === $("pv-sel").value); $("pv-del").disabled = !p;
  if (p) { applyPreset(p); msg(`已切换到「${p.name}」`); }
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

/* ---------- 仓位台账 ---------- */
document.querySelector("main").insertAdjacentHTML("afterbegin", `<section class="pv-ledger" id="pv-ledger" style="margin-top:0">
  <h2>我的仓位 <small id="pv-total"></small><span class="pv-actions">
    <button type="button" class="pv-btn" id="pv-closed" aria-pressed="false">显示已平仓</button>
    <button type="button" class="pv-btn pv-primary" id="pv-new">按当前参数新建仓位</button></span></h2>
  <div id="pv-list" class="dim">读取中…</div></section>`);
let showClosed = false, open = null, rows = [];
const TYPES = {deposit: "投入", withdraw: "提取", collect: "领取手续费", cost: "成本", snapshot: "记录价值"};

function draw() {
  const vis = rows.filter(r => showClosed || r.status !== "closed");
  const live = rows.filter(r => r.status !== "closed"), s = k => live.reduce((a, r) => a + (r.summary[k] || 0), 0);
  $("pv-total").textContent = live.length ? `持仓 ${live.length} 个 · 当前价值 ${usd(s("value"), 0)} · 累计手续费 ${usd(s("fees"))} · 盈亏 ${signed(s("pnl"), usd)}` : "";
  if (!vis.length) { $("pv-list").className = "empty"; $("pv-list").innerHTML = "还没有仓位。在左侧填好池子、上下沿后点「按当前参数新建仓位」。"; return; }
  $("pv-list").className = "scroll";
  $("pv-list").innerHTML = `<table class="pv-table"><tr><th>仓位</th><th>区间</th><th>现价</th><th>投入</th><th>当前价值</th><th>手续费</th><th>盈亏</th><th>实际年化</th><th>手续费年化</th><th>相对持币</th><th>时长</th><th></th></tr>` +
    vis.map(r => { const s = r.summary, p = r.live?.price;
      const st = r.status === "closed" ? `<span class="dim">已平仓</span>` : p == null ? `<span class="neg" title="${esc(r.live?.error)}">读取失败</span>`
        : `<span class="${r.in_range ? "pos" : "neg"}">${usd(p)}</span>`;
      const src = r.live?.nft ? `<span class="pv-tag" title="NFT #${r.token_id} 链上实时">链上</span>` : s.snapshot_at ? `<span class="pv-tag dim" title="最近一次记录">${ago(s.snapshot_at)}前</span>` : "";
      return `<tr data-id="${r.id}" class="${r.status === "closed" ? "pv-closed" : ""}">
        <td><b>${esc(r.name)}</b>${r.note ? `<div class="dim pv-note">${esc(r.note)}</div>` : ""}</td>
        <td>${usd(r.lo)} – ${usd(r.hi)}</td><td>${st}</td><td>${usd(s.deposited, 0)}</td><td>${num(s.value)} ${src}</td>
        <td>${usd(s.fees)}${s.fees_unclaimed ? `<div class="dim pv-note">未领 ${usd(s.fees_unclaimed)}</div>` : ""}</td>
        <td>${sg(s.pnl)}</td><td>${sp(s.apr)}</td><td>${s.fee_apr == null ? "—" : pct(s.fee_apr, 0)}</td><td>${sg(s.vs_hodl)}</td><td>${s.days.toFixed(1)} 天</td>
        <td class="pv-ops"><button type="button" data-a="load">分析</button>${r.token_id && r.status !== "closed" ? `<button type="button" data-a="sync">同步</button>` : ""}<button type="button" data-a="log" aria-expanded="${open === r.id}">记录</button></td></tr>
        ${open === r.id ? `<tr class="pv-detail"><td colspan="12" id="pv-detail"><span class="dim">读取记录…</span></td></tr>` : ""}`; }).join("") + `</table>`;
  if (open) detail(open);
}
async function loadPositions() {
  try { rows = await req("GET", "/p/positions"); draw(); }
  catch (e) { $("pv-list").innerHTML = `<p class="err">${e.message}</p>`; }
}
async function detail(id) {
  const r = rows.find(x => x.id === id), box = $("pv-detail"); if (!r || !box) return;
  const evs = await req("GET", `/p/positions/${id}/events`).catch(() => []);
  const now = new Date(Date.now() - new Date().getTimezoneOffset() * 6e4).toISOString().slice(0, 16);
  box.innerHTML = `<div class="pv-detail-grid">
    <form id="pv-ev" class="pv-form">
      <select name="type">${Object.entries(TYPES).map(([k, v]) => `<option value="${k}">${v}</option>`).join("")}</select>
      <input name="stock" type="number" step="any" placeholder="股票数量">
      <input name="stable" type="number" step="any" placeholder="稳定币">
      <input name="value" type="number" step="any" placeholder="总价值 $">
      <input name="fees" type="number" step="any" placeholder="手续费 $">
      <input name="cost" type="number" step="any" placeholder="成本 $">
      <input name="ts" type="datetime-local" value="${now}">
      <input name="note" type="text" placeholder="备注" maxlength="300">
      <button type="submit" class="pv-btn pv-primary">记一笔</button>
    </form>
    <p class="hint pv-hint" id="pv-hint"></p>
    <div class="pv-settings">
      <label>NFT 编号 <input id="pv-nft" type="number" value="${r.token_id ?? ""}" placeholder="可选，填了自动读链上价值"></label>
      <label>备注 <input id="pv-memo" type="text" value="${esc(r.note)}" maxlength="500"></label>
      <button type="button" class="pv-btn" id="pv-upd">保存设置</button>
      ${r.status === "closed" ? `<button type="button" class="pv-btn" id="pv-reopen">重新打开</button>` : `<button type="button" class="pv-btn" id="pv-close">标记平仓</button>`}
      <button type="button" class="pv-btn pv-danger" id="pv-rm">删除仓位</button>
    </div>
    ${evs.length ? `<table class="pv-events"><tr><th>时间</th><th>类型</th><th>股票</th><th>稳定币</th><th>价值</th><th>手续费</th><th>成本</th><th>备注</th><th></th></tr>` +
      evs.slice().reverse().map(e => `<tr><td>${new Date(e.ts * 1000).toLocaleString("zh-CN", {hour12: false, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit"})}</td><td>${TYPES[e.type]}</td>
        <td>${e.stock ? e.stock.toFixed(4) : ""}</td><td>${e.stable ? usd(e.stable) : ""}</td><td>${e.value ? usd(e.value) : ""}</td>
        <td>${e.fees ? usd(e.fees) : ""}</td><td>${e.cost ? usd(e.cost) : ""}</td><td class="dim">${esc(e.note)}</td>
        <td><button type="button" class="pv-x" data-ev="${e.id}" aria-label="删除这条记录">×</button></td></tr>`).join("") + `</table>` : `<p class="dim">还没有记录。先记一笔「投入」，填股票数量和稳定币数量即可，总价值会按现价自动算。</p>`}
  </div>`;
  const hint = {deposit: "填投入的股票数量和稳定币；总价值留空会按现价计算", withdraw: "填取出的股票数量和稳定币；全部撤出后点「标记平仓」",
    collect: "填领取的手续费美元价值", cost: "填 gas、swap 手续费等美元成本", snapshot: "填仓位当前总价值和未领取手续费（有 NFT 编号时可点「同步」自动记录）"};
  const f = $("pv-ev"), show = () => { const t = f.type.value; $("pv-hint").textContent = hint[t];
    const need = {deposit: ["stock", "stable", "value"], withdraw: ["stock", "stable", "value"], collect: ["fees"], cost: ["cost"], snapshot: ["value", "fees"]}[t];
    ["stock", "stable", "value", "fees", "cost"].forEach(n => f[n].hidden = !need.includes(n)); };
  f.type.onchange = show; show();
  f.onsubmit = async ev => { ev.preventDefault();
    const d = {type: f.type.value, note: f.note.value, ts: f.ts.value ? new Date(f.ts.value).getTime() / 1000 : null};
    ["stock", "stable", "value", "fees", "cost"].forEach(n => { if (!f[n].hidden && f[n].value) d[n] = +f[n].value; });
    try { await req("POST", `/p/positions/${id}/events`, d); await loadPositions(); } catch (e) { $("pv-hint").textContent = e.message; $("pv-hint").className = "hint err"; } };
  box.querySelectorAll("[data-ev]").forEach(b => b.onclick = async () => { if (!confirm("删除这条记录？")) return; await req("DELETE", `/p/events/${b.dataset.ev}`); loadPositions(); });
  $("pv-upd").onclick = async () => { await req("PATCH", `/p/positions/${id}`, {token_id: $("pv-nft").value ? +$("pv-nft").value : null, note: $("pv-memo").value}); loadPositions(); };
  ($("pv-close") || $("pv-reopen")).onclick = async e => { await req("PATCH", `/p/positions/${id}`, {status: e.target.id === "pv-close" ? "closed" : "open"}); loadPositions(); };
  $("pv-rm").onclick = async () => { if (!confirm(`删除仓位「${r.name}」和它的全部记录？`)) return; await req("DELETE", `/p/positions/${id}`); open = null; loadPositions(); };
}
$("pv-list").addEventListener("click", async e => {
  const b = e.target.closest("[data-a]"); if (!b) return;
  const id = b.closest("tr").dataset.id, r = rows.find(x => x.id === id);
  if (b.dataset.a === "load") { $("pool").value = r.pool; $("lo").value = r.lo; $("hi").value = r.hi;
    if (r.summary.deposited) $("cap").value = Math.round(r.summary.value || r.summary.deposited); syncTickers(); check(); }
  if (b.dataset.a === "log") { open = open === id ? null : id; draw(); }
  if (b.dataset.a === "sync") { b.disabled = true; b.textContent = "同步中…";
    try { await req("POST", `/p/positions/${id}/sync`); await loadPositions(); } catch (err) { alert(err.message); b.disabled = false; b.textContent = "同步"; } }
});
$("pv-closed").onclick = () => { showClosed = !showClosed; $("pv-closed").setAttribute("aria-pressed", showClosed); draw(); };
$("pv-new").onclick = async () => {
  const f = form();
  if (!f.lo || !f.hi) { alert("先在左侧填好下沿和上沿"); return; }
  const t = tickerOf(f.pool.toLowerCase()) || f.pool.slice(0, 6);
  const name = prompt("仓位名称", `${t} ${f.lo}–${f.hi}`); if (!name) return;
  const nft = prompt("仓位 NFT 编号（可选，在 PancakeSwap / Uniswap 的仓位页面能看到，填了会自动读链上价值和未领取手续费）", "");
  try {
    const {id} = await req("POST", "/p/positions", {name: name.slice(0, 40), pool: f.pool, lo: f.lo, hi: f.hi, token_id: nft ? +nft : null});
    open = id; await loadPositions();
  } catch (e) { alert(e.message); }
};

loadPresets().catch(e => msg(e.message, true));
loadPositions();
})();
