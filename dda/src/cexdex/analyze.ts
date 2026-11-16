// DDA-012 deviation statistics. Descriptive only: no inventory simulation, no execution claims.
import {createHash} from 'node:crypto';
import {dec} from '../math.js';import {quantile} from '../core/research.js';import {QUOTES} from './registry.js';
import type {CexQuote} from '../adapters/cex.js';import type {CexDexCycle,CexDexSession} from './collect.js';import type {Pool,Quote} from '../types.js';
export const CEXDEX_VERSION='dda-cexdex-1.0.0';
type Dir='dex→cex'|'cex→dex';
export interface Obs { cycle:number; at:number; pool:string; size:number; dir:Dir; edge:number; edgeBps:number; notional:number; }

/** USD per quote-asset unit from the same cycle's Binance book (mid); USDT is 1. */
/** Most specific book first: taken at this pool's snapshot, then at the chain's batch, then untagged. */
const pick=(cex:CexQuote[],sym:string,chain:string,pool?:string,qty?:string)=>{const ok=(c:CexQuote)=>c.symbol===sym&&(qty===undefined||c.baseQty===qty);return cex.find(c=>ok(c)&&pool!==undefined&&c.tag===chain+':'+pool)??cex.find(c=>ok(c)&&c.tag===chain)??cex.find(c=>ok(c)&&c.tag===undefined);};
function conversion(p:Pool,cex:CexQuote[]){const sym=QUOTES[`${p.chain}:${p.quote.symbol}`]?.usdt;if(sym===undefined)return null;if(sym===null)return 1;return pick(cex,sym,p.chain,p.id)?.mid??null;}
function native(chain:'bsc'|'solana',cex:CexQuote[],pool?:string){return pick(cex,chain==='bsc'?'BNBUSDT':'SOLUSDT',chain,pool)?.mid??null;}

/** Edges per pool, size and direction for one cycle; invalid or unmatched inputs produce no observation (unknown, not zero). */
export function edges(s:CexDexSession,c:CexDexCycle):{obs:Obs[];rejects:Record<string,number>}{
 const cfg=s.config,obs:Obs[]=[],rejects:Record<string,number>={};const no=(why:string)=>{rejects[why]=(rejects[why]??0)+1;};
 for(const q of c.quotes){
  const p=s.pools.find(x=>x.id===q.poolId);if(!p){no('池不在登记表');continue;}
  const coin=s.registry.coins.find(x=>x.pools.some(y=>y.id===p.id))!;
  const book=pick(c.cex,coin.symbol,p.chain,p.id,q.baseQty);
  if(!q.valid){no('DEX 报价无效或过期');continue;}
  if(!book){no('交易所盘口缺失');continue;}
  if(book.depthLimited){no('交易所 20 档深度不足');continue;}
  if(Math.abs(book.observedAt-q.observedAt)>cfg.maxSkewMs){no('两侧时间偏差过大');continue;}
  if(q.priceImpactBps>cfg.maxImpactBps){no('DEX 冲击超过阈值');continue;}
  const conv=conversion(p,c.cex),nat=native(p.chain,c.cex,p.id);if(conv===null||nat===null){no('换算价缺失');continue;}
  // Non-USDT quote assets are converted on the exchange: taker fee on the converted leg.
  const convFee=QUOTES[`${p.chain}:${p.quote.symbol}`].usdt===null?0:cfg.takerFee,slip=cfg.extraSlippageBps/10000;
  const dexBuy=dec(q.buy).mul(conv),dexSell=dec(q.sell).mul(conv);
  const a=dec(book.sell!).mul(1-cfg.takerFee).sub(dexBuy.mul(1+convFee+slip)).sub(dec(q.gasBuy).mul(nat)).toNumber();
  const b=dexSell.mul(1-convFee-slip).sub(dec(book.buy!).mul(1+cfg.takerFee)).sub(dec(q.gasSell).mul(nat)).toNumber();
  const notional=dec(book.buy!).add(book.sell!).div(2).toNumber(),at=Math.max(q.observedAt,book.observedAt);
  obs.push({cycle:c.index,at,pool:p.id,size:q.size,dir:'dex→cex',edge:a,edgeBps:a/notional*10000,notional},{cycle:c.index,at,pool:p.id,size:q.size,dir:'cex→dex',edge:b,edgeBps:b/notional*10000,notional});
 }
 return {obs,rejects};
}

/** Withdrawal fees (public, recorded at session start) for moving the coin and USDT back per trade, in USD. */
function rebalanceUsd(s:CexDexSession,p:Pool,price:number){
 const ns:any[]=Array.isArray(s.capabilities.networks)?s.capabilities.networks as any[]:s.registry.coins.flatMap(c=>c.networks);const net=p.chain==='bsc'?'BSC':'SOL';
 const coin=s.registry.coins.find(c=>c.pools.some(x=>x.id===p.id))!.coin;
 const f=(c:string)=>ns.find(n=>n.coin===c&&n.network===net)?.withdrawFee;const cf=f(coin),uf=f('USDT');
 return cf===undefined||uf===undefined?null:Number(cf)*price+Number(uf);
}

export function analyze(s:CexDexSession,cycles:CexDexCycle[]){
 const sorted=[...cycles].sort((a,b)=>a.index-b.index),all:Obs[]=[],rejects:Record<string,number>={};
 for(const c of sorted){const r=edges(s,c);all.push(...r.obs);for(const [k,v] of Object.entries(r.rejects))rejects[k]=(rejects[k]??0)+v;}
 const intervals=sorted.slice(1).map((c,i)=>c.startedAt-sorted[i].startedAt);
 const groups=new Map<string,Obs[]>();for(const o of all){const k=`${o.pool}|${o.size}|${o.dir}`;groups.set(k,[...(groups.get(k)??[]),o]);}
 const rows=[...groups].map(([key,xs])=>{
  const [pool,size,dir]=key.split('|'),p=s.pools.find(x=>x.id===pool)!,coin=s.registry.coins.find(c=>c.pools.includes(p)||c.pools.some(y=>y.id===pool))!;
  const bps=xs.map(x=>x.edgeBps),pos=xs.filter(x=>x.edge>0);
  // Episodes: consecutive cycles with a positive edge; a missing cycle ends the episode with an unknown end (censored).
  const eps:{start:Obs;end:Obs;n:number;censored:boolean}[]=[];let cur:typeof eps[number]|null=null;
  for(const x of xs){if(x.edge>0){if(cur&&x.cycle===cur.end.cycle+1){cur.end=x;cur.n++;}else{if(cur)eps.push(cur);const prev=xs.find(y=>y.cycle===x.cycle-1);cur={start:x,end:x,n:1,censored:!prev};}}else if(cur){eps.push(cur);cur=null;}}
  if(cur){cur.censored=true;eps.push(cur);}
  const next=(o:Obs)=>xs.find(y=>y.cycle===o.cycle+1);
  const persisted=eps.filter(e=>next(e.start)!==undefined),stillPositive=persisted.filter(e=>next(e.start)!.edge>0);
  const reb=rebalanceUsd(s,p,p.initialPrice);
  const firstEdges=eps.map(e=>e.start.edge),hours=(xs.at(-1)!.at-xs[0].at)/3600000||null;
  return {coin:coin.coin,pool,chain:p.chain,market:p.market,dex:p.dex,fee:p.fee,size:Number(size),dir,observations:xs.length,
   positiveShare:pos.length/xs.length,medianBps:quantile(bps,.5),p95Bps:quantile(bps,.95),maxBps:Math.max(...bps),
   episodes:eps.length,medianEpisodeCycles:quantile(eps.map(e=>e.n),.5),censoredEpisodes:eps.filter(e=>e.censored).length,
   persistNextCycle:persisted.length?stillPositive.length/persisted.length:null,persistDenominator:persisted.length,
   firstEdgeSum:firstEdges.reduce((a,b)=>a+b,0),firstEdgeMedian:quantile(firstEdges,.5),perHour:hours?firstEdges.reduce((a,b)=>a+b,0)/hours:null,
   rebalanceUsd:reb,afterRebalanceSum:reb===null?null:firstEdges.reduce((a,b)=>a+b-reb,0)};
 }).sort((a,b)=>b.firstEdgeSum-a.firstEdgeSum);
 // Mid deviation (size-independent): DEX spot converted to USD vs Binance mid.
 const dev=new Map<string,number[]>();
 for(const c of sorted)for(const q of c.quotes){if(!q.valid)continue;const p=s.pools.find(x=>x.id===q.poolId);if(!p)continue;const coin=s.registry.coins.find(x=>x.pools.some(y=>y.id===p.id))!;
  const book=pick(c.cex,coin.symbol,p.chain,p.id),conv=conversion(p,c.cex);if(!book||conv===null||Math.abs(book.observedAt-q.observedAt)>s.config.maxSkewMs)continue;
  const k=p.id;if(q.size!==s.config.sizes[0])continue;dev.set(k,[...(dev.get(k)??[]),(q.spot*conv/book.mid-1)*10000]);}
 const deviation=[...dev].map(([pool,xs])=>{const p=s.pools.find(x=>x.id===pool)!;const abs=xs.map(Math.abs);return {pool,market:p.market,chain:p.chain,fee:p.fee,n:xs.length,medianBps:quantile(xs,.5),medianAbsBps:quantile(abs,.5),p95AbsBps:quantile(abs,.95),feeBps:p.fee*10000};});
 const digest=createHash('sha256').update(JSON.stringify({v:CEXDEX_VERSION,config:s.config,pools:s.pools.map(p=>p.id),cycles:sorted.map(c=>[c.index,c.quotes,c.cex])})).digest('hex');
 const positive=rows.filter(r=>r.episodes>0);
 return {version:CEXDEX_VERSION,digest,session:s.id,startedAt:s.startedAt,endedAt:s.endedAt,status:s.status,config:s.config,
  quality:{cycles:sorted.length,intervalMedianMs:quantile(intervals,.5),intervalP95Ms:quantile(intervals,.95),observations:all.length,rejects,errors:sorted.reduce((n,c)=>n+c.errors.length,0)},
  rows,deviation,summary:{groups:rows.length,withPositive:positive.length,totalFirstEdge:rows.reduce((a,r)=>a+r.firstEdgeSum,0)}};
}
export type CexDexResult=ReturnType<typeof analyze>;

const f=(x:number|null|undefined,d=2)=>x===null||x===undefined||!Number.isFinite(x)?'—':x.toFixed(d);
export function markdown(r:CexDexResult){
 const q=r.quality;
 return `# DDA-012 交易所—DEX 偏离研究\n\n会话 ${r.session}，${new Date(r.startedAt).toISOString()} → ${r.endedAt?new Date(r.endedAt).toISOString():'采集中'}，状态 ${r.status}。版本 ${r.version}，输入摘要 ${r.digest}。\n\n`+
 `周期 ${q.cycles}，间隔中位 / P95 ${f(q.intervalMedianMs,0)} / ${f(q.intervalP95Ms,0)} ms，有效观测 ${q.observations}，请求错误 ${q.errors}。剔除：${Object.entries(q.rejects).map(([k,v])=>`${k} ${v}`).join('；')||'无'}。\n\n`+
 `## 口径\n\n同一币的 DEX 池与币安盘口按相同基础币数量比较：DEX exact-output 买 / exact-input 卖（池费与冲击已在报价中），币安按公开行情流（20 档、100 ms）在该链 DEX 报价完成时的盘口逐档成交，taker 费 ${f(r.config.takerFee*100,3)}%；非 USDT 计价池按同周期币安中间价换算，并对换算腿再计一次 taker 费；另扣额外滑点 ${r.config.extraSlippageBps} bps 与 gas（按币安 BNB/SOL 中间价）。两侧时间差超过 ${r.config.maxSkewMs} ms、DEX 报价无效、盘口深度不足都不计入。\n\n`+
 `「机会段」是连续为正的周期；采样间隔约 5 秒，持续时间只能精确到周期。「下一周期仍为正」近似 5 秒延迟后是否还能吃到。逐笔再平衡按会话开始时记录的币安公开提币费，把基础币和 USDT 各提一次计入（最保守）；实际可批量再平衡，故同时给出不含再平衡的数。这是报价层面的描述统计，没有真实成交、排队、对手方或被抢概率。\n\n`+
 `## 中间价偏离（${r.config.sizes[0]} 美元档池，USD 计）\n\n| 池 | 链 | 费率 | 样本 | 偏离中位 | |偏离|中位 | |偏离| P95 |\n|---|---|---:|---:|---:|---:|---:|\n`+
 r.deviation.map(d=>`| ${d.market} ${d.pool.slice(0,8)} | ${d.chain} | ${f(d.feeBps,1)} bps | ${d.n} | ${f(d.medianBps,1)} | ${f(d.medianAbsBps,1)} | ${f(d.p95AbsBps,1)} |`).join('\n')+
 `\n\n## 扣除成本后的机会\n\n| 币 | 池 | 规模 | 方向 | 观测 | 为正占比 | 中位 bps | P95 bps | 最大 bps | 机会段 | 段长中位（周期） | 下一周期仍为正 | 首次观测合计 $ | 每小时 $ | 逐笔再平衡 $ | 扣再平衡合计 $ |\n|---|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n`+
 r.rows.map(x=>`| ${x.coin} | ${x.chain} ${x.market} ${x.pool.slice(0,6)} | ${x.size} | ${x.dir} | ${x.observations} | ${f(x.positiveShare*100,1)}% | ${f(x.medianBps,1)} | ${f(x.p95Bps,1)} | ${f(x.maxBps,1)} | ${x.episodes} | ${f(x.medianEpisodeCycles,0)} | ${x.persistNextCycle===null?'—':f(x.persistNextCycle*100,0)+'%（'+x.persistDenominator+'）'} | ${f(x.firstEdgeSum)} | ${f(x.perHour)} | ${f(x.rebalanceUsd)} | ${f(x.afterRebalanceSum)} |`).join('\n')+
 `\n\n共 ${r.summary.groups} 组，其中 ${r.summary.withPositive} 组出现过正收益段；首次观测合计 ${f(r.summary.totalFirstEdge)} 美元（每段只算一次，未扣再平衡）。\n\n不确定性：公共节点 5 秒采样会漏掉更短的机会，也会把同一机会算进相邻周期；币安公开盘口有缓存，时间对齐只到毫秒级请求时间；交易所充提有暂停、限额与对手方风险，费率会变。结论只用于决定是否继续研究，不构成执行授权。\n`;
}
