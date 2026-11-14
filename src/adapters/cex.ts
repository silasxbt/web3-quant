// Read-only Binance public market data (DDA-012). No account, no API key, no order endpoints.
import {Net} from '../net.js';import {dec} from '../math.js';
export const CEX_MARKET='https://data-api.binance.vision';
export const CEX_NETWORKS='https://www.binance.com/bapi/capital/v1/public/capital/getNetworkCoinAll';
export interface CexQuote { symbol:string; size:number; baseQty:string; buy:string|null; sell:string|null; bestBid:string; bestAsk:string; mid:number; lastUpdateId:number; startedAt:number; observedAt:number; depthLimited:boolean; tag?:string; }
export interface CexNetwork { coin:string; network:string; contract:string|null; withdrawFee:string; withdrawMin:string; minConfirm:number; depositEnable:boolean; withdrawEnable:boolean; busy:boolean; estimatedArrivalTime:number|null; }
const DEPTH=100;

/** Cost of buying (walking asks) and proceeds of selling (walking bids) exactly qty base units; null when depth is insufficient. */
export function walk(levels:[string,string][],qty:string):string|null{
 let left=dec(qty),total=dec(0);
 for(const [p,q] of levels){const take=left.lt(q)?left:dec(q);total=total.add(take.mul(p));left=left.sub(take);if(left.lte(0))return total.toFixed();}
 return null;
}
export class CexAdapter {
 constructor(public net:Net){}
 async book(symbol:string,qtys:{size:number;baseQty:string}[]):Promise<CexQuote[]>{
  const startedAt=Date.now();const d=await this.net.json(`${CEX_MARKET}/api/v3/depth?symbol=${encodeURIComponent(symbol)}&limit=${DEPTH}`);const observedAt=Date.now();
  if(!Array.isArray(d?.bids)||!Array.isArray(d?.asks)||!d.bids.length||!d.asks.length)throw new Error('盘口为空 '+symbol);
  const bestBid=d.bids[0][0],bestAsk=d.asks[0][0],mid=dec(bestBid).add(bestAsk).div(2).toNumber();
  return qtys.map(({size,baseQty})=>{const buy=walk(d.asks,baseQty),sell=walk(d.bids,baseQty);
   return {symbol,size,baseQty,buy,sell,bestBid,bestAsk,mid,lastUpdateId:d.lastUpdateId,startedAt,observedAt,depthLimited:buy===null||sell===null};});
 }
 async mid(symbol:string){const d=await this.net.json(`${CEX_MARKET}/api/v3/ticker/bookTicker?symbol=${encodeURIComponent(symbol)}`);return dec(d.bidPrice).add(d.askPrice).div(2).toNumber();}
 async tradingSymbols():Promise<Set<string>>{const d=await this.net.json(`${CEX_MARKET}/api/v3/exchangeInfo?permissions=SPOT`);return new Set((d.symbols??[]).filter((s:any)=>s.status==='TRADING').map((s:any)=>s.symbol));}
 /** Public deposit/withdraw network list; only BSC and Solana entries are kept. */
 async networks():Promise<CexNetwork[]>{
  const d=await this.net.json(CEX_NETWORKS);if(d?.code!=='000000'||!Array.isArray(d.data))throw new Error('币安网络列表不可用');
  return d.data.flatMap((c:any)=>(c.networkList??[]).filter((n:any)=>['BSC','SOL'].includes(n.network)).map((n:any):CexNetwork=>({coin:c.coin,network:n.network,contract:n.contractAddress||null,withdrawFee:String(n.withdrawFee),withdrawMin:String(n.withdrawMin),minConfirm:Number(n.minConfirm),depositEnable:!!n.depositEnable,withdrawEnable:!!n.withdrawEnable,busy:!!n.busy,estimatedArrivalTime:n.estimatedArrivalTime??null})));
 }
}

export const CEX_STREAM='wss://data-stream.binance.vision/stream';
type Book={bids:[string,string][];asks:[string,string][];lastUpdateId:number;receivedAt:number};
/**
 * Public partial-depth stream (20 levels, 100 ms) for every symbol over one connection. Binance pushes a symbol only when
 * its top 20 levels change, so a quiet book stays valid while the connection is alive. Books are seeded from REST depth,
 * replaced only by a newer lastUpdateId. A snapshot is stamped "now" only while the connection delivered any message in
 * the last second; otherwise it carries the last message time and fails the two-sided time gate. Reconnects with backoff.
 */
export class CexStream {
 books=new Map<string,Book>(); messages=0; reconnects=0; lastMessage=0; private ws:WebSocket|null=null; private closed=false; private wait=1000;
 constructor(public symbols:string[],private record:(x:unknown)=>void=()=>{}){}
 start(){this.closed=false;this.open();return this;}
 private open(){
  const url=`${CEX_STREAM}?streams=${this.symbols.map(s=>s.toLowerCase()+'@depth20@100ms').join('/')}`,ws=new WebSocket(url);this.ws=ws;
  ws.onopen=()=>{this.wait=1000;this.record({type:'cex-stream-open',at:Date.now(),symbols:this.symbols});};
  ws.onmessage=e=>{try{const m=JSON.parse(String(e.data)),sym=String(m.stream).split('@')[0].toUpperCase(),d=m.data;this.lastMessage=Date.now();if(Array.isArray(d?.bids)&&Array.isArray(d?.asks)&&!((this.books.get(sym)?.lastUpdateId??-1)>d.lastUpdateId)){this.books.set(sym,{bids:d.bids,asks:d.asks,lastUpdateId:d.lastUpdateId,receivedAt:Date.now()});this.messages++;}}catch{}};
  ws.onclose=()=>{this.record({type:'cex-stream-close',at:Date.now()});if(this.closed)return;this.reconnects++;setTimeout(()=>{if(!this.closed)this.open();},this.wait);this.wait=Math.min(this.wait*2,30000);};
  ws.onerror=()=>{try{ws.close();}catch{}};
 }
 /** Waits for the connection, then seeds every symbol from REST depth so quiet books exist from the start. */
 async ready(net:Net,timeoutMs=10000){
  const end=Date.now()+timeoutMs;while(!this.lastMessage&&Date.now()<end)await new Promise(r=>setTimeout(r,100));if(!this.lastMessage)return false;
  for(const sym of this.symbols){const d=await net.json(`${CEX_MARKET}/api/v3/depth?symbol=${sym}&limit=20`);if(!((this.books.get(sym)?.lastUpdateId??-1)>=d.lastUpdateId))this.books.set(sym,{bids:d.bids,asks:d.asks,lastUpdateId:d.lastUpdateId,receivedAt:Date.now()});}
  return this.symbols.every(s=>this.books.has(s));
 }
 stop(){this.closed=true;this.ws?.close();}
 /** Walks the latest book for each requested quantity; the levels used are written to the raw log. */
 snapshot(symbol:string,qtys:{size:number;baseQty:string}[],tag?:string):CexQuote[]{
  const b=this.books.get(symbol);if(!b)return [];const now=Date.now(),at=now-this.lastMessage<=1000?now:this.lastMessage;
  this.record({type:'cex-book',symbol,tag,at:now,lastMessage:this.lastMessage,receivedAt:b.receivedAt,lastUpdateId:b.lastUpdateId,bids:b.bids,asks:b.asks});
  const bestBid=b.bids[0]?.[0],bestAsk=b.asks[0]?.[0];if(!bestBid||!bestAsk)return [];const mid=dec(bestBid).add(bestAsk).div(2).toNumber();
  return qtys.map(({size,baseQty})=>{const buy=walk(b.asks,baseQty),sell=walk(b.bids,baseQty);
   return {symbol,size,baseQty,buy,sell,bestBid,bestAsk,mid,lastUpdateId:b.lastUpdateId,startedAt:b.receivedAt,observedAt:at,depthLimited:buy===null||sell===null,...(tag?{tag}:{})};});
 }
}
