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
      <span class="pv-card-name"><span class="dot pv-st-${st}"></span>${esc(r.name)}</span><b>${num(s.value)}</b>
      <span class="dim">${usd(r.lo)} – ${usd(r.hi)}</span><span>${sg(s.pnl)}</span></button>`; }).join("");
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
    $("pool").value = r.pool; $("lo").value = +r.lo.toFixed(4); $("hi").value = +r.hi.toFixed(4);
    $("cap").value = Math.max(1, Math.round(r.summary.value || r.summary.deposited || 1000));
    syncTickers(); check();
  }
  drawPos(r);
}
function drawPos(r) {
  const s = r.summary, box = $("pv-pos"); box.dataset.id = r.id;
  const src = r.live?.nft ? `链上实时（NFT #${r.token_id}）` : s.snapshot_at ? `最近记录 ${ago(s.snapshot_at)}前` : "还没有价值记录";
  box.innerHTML = `<div class="pv-pos-head"><h2>${esc(r.name)}</h2>
      <span class="dim">${usd(r.lo)} – ${usd(r.hi)} · ${src}${r.status === "closed" ? " · 已平仓" : ""}</span>
      <span class="pv-actions">${r.token_id && r.status !== "closed" ? `<button type="button" class="pv-btn" data-a="sync">同步链上</button>` : ""}
        <button type="button" class="pv-btn" data-a="bt">回测这个区间</button>
        <button type="button" class="pv-btn" data-a="log" aria-expanded="false">记录与设置</button></span></div>
    <dl class="pv-kpi">
      <div><dt>投入</dt><dd>${usd(s.deposited)}</dd></div>
      <div><dt>当前价值</dt><dd>${num(s.value)}</dd></div>
      <div><dt>手续费 <span class="dim">· 记账后</span></dt><dd>${usd(s.fees)}${s.fees_before ? `<small class="dim" title="导入时仓位里已有的未领手续费，不计入收益"> 另有导入前 ${usd(s.fees_before)}</small>` : ""}</dd></div>
      <div><dt>盈亏</dt><dd>${sg(s.pnl)}</dd></div>
      <div><dt title="(当前价值 + 已提取 + 手续费 − 投入 − 成本)，含标的涨跌和无常损失">实际年化</dt><dd>${sp(s.apr)}</dd></div>
      <div><dt title="只算记账后赚到的手续费">手续费年化</dt><dd>${s.fee_apr == null ? "—" : pct(s.fee_apr, 0)}</dd></div>
      <div><dt>相对持币</dt><dd>${sg(s.vs_hodl)}</dd></div>
      <div><dt>时长</dt><dd>${s.days.toFixed(1)} 天</dd></div>
    </dl>
    <p class="note">下方的深度图、区间评级、收益口径和调仓成本，都按这个仓位的实际区间和当前价值 ${num(s.value ?? s.deposited)} 计算。${s.days < 1 ? "持有不足 1 天，暂不显示年化。" : ""}</p>
    <div id="pv-log" hidden></div>`;
}
$("pv-pos").addEventListener("click", async e => {
  const b = e.target.closest("[data-a]"); if (!b) return;
  const r = rows.find(x => x.id === $("pv-pos").dataset.id); if (!r) return;
  if (b.dataset.a === "sync") {
    busy(b, true, "同步中…");
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
      <input name="stock" type="number" step="any" placeholder="股票数量"><input name="stable" type="number" step="any" placeholder="稳定币">
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

setMode(localStorage.getItem("rwalp.mode") === "track" ? "track" : "calc");
loadPresets().catch(e => msg(e.message, true));
loadPositions();
})();
