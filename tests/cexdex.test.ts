import {test} from 'node:test';import assert from 'node:assert/strict';
import {walk,CexStream,type CexQuote} from '../src/adapters/cex.js';import {edges,analyze} from '../src/cexdex/analyze.js';import {baseQty} from '../src/cexdex/collect.js';
import {assets,validateConfig} from '../src/config.js';
import type {Pool,Quote} from '../src/types.js';
const t=1792044961471;
const base={...assets.wbnb,id:'bsc:TKN',address:'0xtoken',symbol:'TKN',decimals:18};
const pool:Pool={id:'0xpool',chain:'bsc',market:'TKN/USDT',dex:'PancakeSwap V3',base,quote:assets.bscUSDT,fee:.0025,program:'f',initialPrice:2,enabled:true};
const wpool:Pool={...pool,id:'0xwpool',market:'TKN/WBNB',quote:assets.wbnb};
const registry={version:1 as const,createdAt:'',source:'test',takerFee:.001,coins:[{coin:'TKN',symbol:'TKNUSDT',assets:{bsc:base},pools:[pool,wpool],networks:[{coin:'TKN',network:'BSC',contract:'0xtoken',withdrawFee:'1',withdrawMin:'1',minConfirm:1,depositEnable:true,withdrawEnable:true,busy:false,estimatedArrivalTime:1},{coin:'USDT',network:'BSC',contract:null,withdrawFee:'0.01',withdrawMin:'1',minConfirm:1,depositEnable:true,withdrawEnable:true,busy:false,estimatedArrivalTime:1}]}]};
const session=(sizes=[100])=>({kind:'cexdex' as const,id:'s',startedAt:t,endedAt:t+60000,status:'completed' as const,config:{...validateConfig({sizes,maxImpactBps:1000,extraSlippageBps:0}),takerFee:.001},pools:[pool,wpool],registry,capabilities:{networks:registry.coins[0].networks}});
const q=(p:Pool,buy:string,sell:string,at=t,changes:Partial<Quote>={}):Quote=>({poolId:p.id,chain:'bsc',market:p.market,size:100,baseAtoms:'50000000000000000000',baseQty:'50',buyAtoms:'0',sellAtoms:'0',buy,sell,spot:2,gasBuy:'0',gasSell:'0',nativeUsd:null,feeRate:p.fee,priceImpactBps:10,startedAt:at-100,observedAt:at,block:'0x1',blockHash:'h',blockTime:at-500,slotSpan:0,valid:true,...changes});
const book=(symbol:string,buy:string|null,sell:string|null,mid:number,at=t,baseQty='50'):CexQuote=>({symbol,size:100,baseQty,buy,sell,bestBid:String(mid),bestAsk:String(mid),mid,lastUpdateId:1,startedAt:at,observedAt:at,depthLimited:buy===null||sell===null,tag:'bsc'});

test('盘口逐档成交：精确数量、深度不足返回空',()=>{const asks:[string,string][]=[['2','10'],['2.1','10']];assert.equal(walk(asks,'15'),'30.5');assert.equal(walk(asks,'20'),'41');assert.equal(walk(asks,'20.000001'),null);});
test('边际收益：两方向各扣一次交易所 taker 费，DEX 费用不重复扣',()=>{
 const c={id:'c0',sessionId:'s',index:0,startedAt:t,finishedAt:t,errors:[],quotes:[q(pool,'99','98')],cex:[book('TKNUSDT','100','101',2),book('BNBUSDT','0','0',1000,t,'0')]};
 const r=edges(session(),c);const a=r.obs.find(o=>o.dir==='dex→cex')!,b=r.obs.find(o=>o.dir==='cex→dex')!;
 assert.ok(Math.abs(a.edge-(101*.999-99))<1e-9);assert.ok(Math.abs(b.edge-(98-100*1.001))<1e-9);});
test('非 USDT 计价池按同周期中间价换算，并对换算腿计 taker 费',()=>{
 const c={id:'c0',sessionId:'s',index:0,startedAt:t,finishedAt:t,errors:[],quotes:[q(wpool,'0.1','0.09')],cex:[book('TKNUSDT','100','101',2),book('BNBUSDT','0','0',1000,t,'0')]};
 const a=edges(session(),c).obs.find(o=>o.dir==='dex→cex')!;assert.ok(Math.abs(a.edge-(101*.999-100*1.001))<1e-9);});
test('时间偏差、深度不足、报价无效或换算缺失都不产生观测（未知而非零）',()=>{
 const mk=(qq:Quote,bb:CexQuote[])=>edges(session(),{id:'c',sessionId:'s',index:0,startedAt:t,finishedAt:t,errors:[],quotes:[qq],cex:bb});
 assert.equal(mk(q(pool,'99','98',t+3000),[book('TKNUSDT','100','101',2)]).obs.length,0);
 assert.equal(mk(q(pool,'99','98'),[book('TKNUSDT',null,'101',2)]).obs.length,0);
 assert.equal(mk(q(pool,'99','98',t,{valid:false}),[book('TKNUSDT','100','101',2)]).obs.length,0);
 assert.equal(mk(q(wpool,'0.1','0.09'),[book('TKNUSDT','100','101',2)]).obs.length,0);
 assert.equal(mk(q(pool,'99','98'),[book('TKNUSDT','100','101',2,t,'49')]).obs.length,0,'数量不同不能比较');});
test('机会段按连续周期计；缺失周期让段落截断；下一周期持续率与再平衡成本',()=>{
 const cyc=(i:number,buy:string)=>({id:'c'+i,sessionId:'s',index:i,startedAt:t+i*5000,finishedAt:t+i*5000,errors:[],quotes:[q(pool,buy,'90',t+i*5000)],cex:[book('TKNUSDT','95','101',2,t+i*5000),book('BNBUSDT','0','0',1000,t+i*5000,'0')]});
 const r=analyze(session(),[cyc(0,'99'),cyc(1,'99'),cyc(2,'101'),cyc(4,'99')]);const row=r.rows.find(x=>x.dir==='dex→cex')!;
 assert.equal(row.episodes,2);assert.equal(row.medianEpisodeCycles!>=1,true);assert.equal(row.persistNextCycle,1);assert.equal(row.persistDenominator,1);assert.equal(row.censoredEpisodes,2,"首段起点在数据开头、末段前一周期缺失，两段都是截断");
 assert.equal(row.rebalanceUsd,1*2+.01);
 assert.deepEqual(analyze(session(),[cyc(0,'99'),cyc(1,'99')]).digest,analyze(session(),[cyc(0,'99'),cyc(1,'99')]).digest);});
test('基础数量与 DEX 适配器同一取整；USDC/USDT 市场沿用 1:1 数量',()=>{assert.equal(baseQty(100,pool),'50');assert.equal(baseQty(100,{...pool,market:'USDC/USDT',initialPrice:.9989}),'100');assert.equal(baseQty(100,{...pool,initialPrice:3}),'33.333333333333333333');});
test('行情流断开后快照带最后消息时间，过期即无法通过时间门禁',()=>{const s=new CexStream(['TKNUSDT']);s.books.set('TKNUSDT',{bids:[['1.9','100']],asks:[['2.1','100']],lastUpdateId:5,receivedAt:t});
 s.lastMessage=Date.now();assert.ok(Math.abs(s.snapshot('TKNUSDT',[{size:100,baseQty:'50'}])[0].observedAt-Date.now())<50);
 s.lastMessage=Date.now()-5000;assert.equal(s.snapshot('TKNUSDT',[{size:100,baseQty:'50'}])[0].observedAt,s.lastMessage);});
