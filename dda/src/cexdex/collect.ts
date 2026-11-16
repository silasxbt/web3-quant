// DDA-012 CEX↔DEX deviation sampling. Read-only: DEX quotes through the existing adapters, Binance public order books.
import {randomUUID} from 'node:crypto';import {resolve} from 'node:path';
import {Net,redact} from '../net.js';import {BscAdapter} from '../adapters/bsc.js';import {SolanaAdapter} from '../adapters/solana.js';import {CexAdapter,CexStream,type CexQuote} from '../adapters/cex.js';
import {validateConfig} from '../config.js';import {atoms,dec,units} from '../math.js';import {Store} from '../store.js';import {QUOTES,type Registry} from './registry.js';
import type {Pool,Quote,Config} from '../types.js';
export interface CexDexConfig extends Config { takerFee:number; }
export interface CexDexSession { kind:'cexdex'; id:string; startedAt:number; endedAt:number|null; status:'running'|'stopped'|'completed'|'interrupted'; config:CexDexConfig; pools:Pool[]; registry:Registry; capabilities:Record<string,unknown>; }
export interface CexDexCycle { id:string; sessionId:string; index:number; startedAt:number; finishedAt:number; quotes:Quote[]; cex:CexQuote[]; errors:{source:string;message:string}[]; }
const CONVERSIONS=['USDCUSDT','BNBUSDT','SOLUSDT'];

export async function collect(store:Store,registry:Registry,input:Partial<CexDexConfig>,shouldStop:()=>boolean,log=(_:string)=>{}):Promise<CexDexSession>{
 const config:CexDexConfig={...validateConfig({sizes:[100,1000],maxImpactBps:1000,...input}),takerFee:input.takerFee??registry.takerFee};
 const unlock=store.lock(),id=randomUUID(),net=new Net(resolve(store.root,'raw',id+'.ndjson.gz'));
 const pools=registry.coins.filter(c=>!c.excluded).flatMap(c=>c.pools.filter(p=>p.enabled));
 const s:CexDexSession={kind:'cexdex',id,startedAt:Date.now(),endedAt:null,status:'running',config,pools,registry,capabilities:{}};
 try{
  const bsc=new BscAdapter(net,config),sol=new SolanaAdapter(net,config),cex=new CexAdapter(net);if(pools.some(p=>p.chain==='solana'))await sol.init();
  // Registry snapshot of deposit/withdraw status at session start; fees can change and are recorded as observed.
  try{s.capabilities.networks=(await cex.networks()).filter(n=>registry.coins.some(c=>c.coin===n.coin));}catch(e){s.capabilities.networks={error:redact(String(e))};}
  store.saveSession(s as any);
  const symbols=[...new Set(pools.map(p=>registry.coins.find(c=>c.pools.includes(p))!.symbol))];
  // Pools on different chains floor the same USD size to different decimals; the book is walked for every distinct quantity.
  const qty=new Map(symbols.map(sym=>{const coin=registry.coins.find(c=>c.symbol===sym)!;const rows=new Map<string,{size:number;baseQty:string}>();
   for(const p of coin.pools.filter(x=>pools.includes(x)))for(const size of config.sizes){const q=baseQty(size,p);rows.set(size+':'+q,{size,baseQty:q});}return [sym,[...rows.values()]];}));
  const bscPools=pools.filter(p=>p.chain==='bsc'),solPools=pools.filter(p=>p.chain==='solana'),deadline=s.startedAt+config.durationMinutes*60000;
  const coinOf=(p:Pool)=>registry.coins.find(c=>c.pools.includes(p))!;
  const stream=new CexStream([...new Set([...symbols,...CONVERSIONS])],x=>net.record(x)).start();
  try{
   if(!await stream.ready(net))throw new Error('币安行情流未就绪');
   // Books are snapshotted the moment each chain's DEX quotes finish, so both sides of a comparison are close in time.
   const books=(chainPools:Pool[],tag:string)=>{const syms=[...new Set(chainPools.map(p=>coinOf(p).symbol))];
    return [...syms.flatMap(sym=>stream.snapshot(sym,qty.get(sym)!,tag)),...CONVERSIONS.filter(sym=>!syms.includes(sym)).flatMap(sym=>stream.snapshot(sym,[{size:0,baseQty:'0'}],tag))];};
   for(let index=0;!shouldStop()&&Date.now()<deadline;index++){
    net.beginCycle();const startedAt=Date.now(),c:CexDexCycle={id:`${id}:${index}`,sessionId:id,index,startedAt,finishedAt:0,quotes:[],cex:[],errors:[]};
    const err=(source:string,e:unknown)=>c.errors.push({source,message:redact(String(e))});
    await Promise.allSettled([
     (async()=>{if(!bscPools.length)return;try{const all=await bsc.quotesAll(bscPools,await bsc.head());c.cex.push(...books(bscPools,'bsc'));for(const p of bscPools)c.quotes.push(...all.get(p.id)??[]);}catch(e){err('bsc',e);}})(),
     // Solana pools are snapshotted one by one; each pool's books are taken the moment its own snapshot completes.
     (async()=>{if(!solPools.length)return;try{const all=await sol.quotesAll(solPools,undefined,p=>c.cex.push(...books([p],'solana:'+p.id)));for(const p of solPools){const r=all.get(p.id);if(r instanceof Error||!r)err(p.id,r);else c.quotes.push(...r);}}catch(e){err('solana',e);}})()
    ]);
    c.finishedAt=Date.now();store.addCycle(c as any);
    if(index%12===0)log(JSON.stringify({session:id,cycles:index+1,quotes:c.quotes.length,cex:c.cex.length,errors:c.errors.length,streamMessages:stream.messages,reconnects:stream.reconnects}));
    const wait=Math.min(config.intervalMs-(Date.now()-startedAt),deadline-Date.now());if(wait>0)await new Promise(r=>setTimeout(r,wait));
   }
   s.capabilities.stream={messages:stream.messages,reconnects:stream.reconnects};
  }finally{stream.stop();}
  s.status=shouldStop()?'stopped':'completed';
 }catch(e){s.status='interrupted';s.capabilities.error=redact(String(e));throw e;}
 finally{s.endedAt=Date.now();store.saveSession(s as any);unlock();}
 return s;
}
/** Same base quantity the DEX adapters derive from initialPrice (USD per base unit), as a decimal string. */
export function baseQty(size:number,p:Pool){return units(atoms(dec(size).div(p.market==='USDC/USDT'?1:p.initialPrice),p.base.decimals),p.base.decimals);}
export {QUOTES};
