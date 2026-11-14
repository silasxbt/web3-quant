// DDA-012 asset registry: coins Binance accepts on both BSC and Solana, contracts as Binance lists them,
// decimals read on-chain, and DEX pools that returned a valid quote at discovery time.
import {encodeFunctionData,decodeFunctionResult,parseAbi,type Address} from 'viem';
import {assets} from '../config.js';import {Net} from '../net.js';import {BscAdapter} from '../adapters/bsc.js';import {SolanaAdapter} from '../adapters/solana.js';import {CexAdapter,type CexNetwork} from '../adapters/cex.js';
import type {Asset,Pool,Config} from '../types.js';
const FACTORY='0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865',RAYDIUM_CLMM='CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK',WSOL='So11111111111111111111111111111111111111112';
const erc20=parseAbi(['function decimals() view returns(uint8)']),factory=parseAbi(['function getPool(address,address,uint24) view returns(address)']);
/** DEX quote assets and the Binance symbol that converts them to USDT (null = USDT itself). */
export const QUOTES:Record<string,{asset:Asset;usdt:string|null}>={
 'bsc:USDT':{asset:assets.bscUSDT,usdt:null},'bsc:USDC':{asset:assets.bscUSDC,usdt:'USDCUSDT'},'bsc:WBNB':{asset:assets.wbnb,usdt:'BNBUSDT'},
 'solana:USDT':{asset:assets.solUSDT,usdt:null},'solana:USDC':{asset:assets.solUSDC,usdt:'USDCUSDT'},'solana:SOL':{asset:assets.sol,usdt:'SOLUSDT'},
};
export interface RegistryCoin { coin:string; symbol:string; networks:CexNetwork[]; assets:Partial<Record<'bsc'|'solana',Asset>>; pools:Pool[]; excluded?:string; }
export interface Registry { version:1; createdAt:string; source:string; takerFee:number; coins:RegistryCoin[]; }

export async function discover(net:Net,cfg:Config,log=(_:string)=>{}):Promise<Registry>{
 const cex=new CexAdapter(net),bsc=new BscAdapter(net,cfg),sol=new SolanaAdapter(net,cfg);await sol.init();
 const [networks,symbols]=await Promise.all([cex.networks(),cex.tradingSymbols()]);
 const by=new Map<string,CexNetwork[]>();for(const n of networks)by.set(n.coin,[...(by.get(n.coin)??[]),n]);
 const coins:RegistryCoin[]=[];
 for(const [coin,ns] of [...by].sort((a,b)=>a[0].localeCompare(b[0]))){
  if(ns.length<2)continue;const symbol=coin+'USDT',entry:RegistryCoin={coin,symbol,networks:ns,assets:{},pools:[]};coins.push(entry);
  const skip=(why:string)=>{entry.excluded=why;log(`${coin}: ${why}`);};
  if(coin==='USDT'){skip('计价币本身，不作为基础资产');continue;}
  if(/^\d/.test(coin)){skip('交易所计价单位是链上代币的倍数，单位换算未核验');continue;}
  if(!ns.every(n=>n.depositEnable&&n.withdrawEnable)){skip('币安 BSC 或 Solana 充提未同时开启');continue;}
  if(!symbols.has(symbol)){skip('币安没有正在交易的 '+symbol);continue;}
  const mid=await cex.mid(symbol);
  for(const n of ns){
   const chain=n.network==='BSC'?'bsc':'solana';const address=n.contract??(chain==='solana'&&coin==='SOL'?WSOL:null);
   if(!address){log(`${coin} ${chain}: 无合约地址`);continue;}
   try{
    const decimals=chain==='bsc'?Number(decodeFunctionResult({abi:erc20,functionName:'decimals',data:await bsc.call(address,encodeFunctionData({abi:erc20,functionName:'decimals'}))})):await splDecimals(net,cfg,address);
    entry.assets[chain]={id:`${chain}:${coin}@${address}`,chain,address,symbol:coin,decimals,kind:'wrapped',provenance:`币安 ${n.network} 网络登记合约；经币安余额兑付（DDA-012），含交易所对手方与充提暂停风险`};
   }catch(e){log(`${coin} ${chain}: 精度读取失败 ${String(e).slice(0,80)}`);}
  }
  for(const [chain,base] of Object.entries(entry.assets) as ['bsc'|'solana',Asset][]){
   const candidates:Pool[]=[];
   for(const [key,q] of Object.entries(QUOTES)){
    if(!key.startsWith(chain+':')||q.asset.address===base.address)continue;
    if(chain==='bsc'){
     for(const fee of [100,500,2500,10000]){
      try{const id=decodeFunctionResult({abi:factory,functionName:'getPool',data:await bsc.call(FACTORY,encodeFunctionData({abi:factory,functionName:'getPool',args:[base.address as Address,q.asset.address as Address,fee]}))}) as string;
       if(!/^0x0+$/.test(id))candidates.push(pool(id,'bsc','PancakeSwap V3',base,q.asset,fee/1e6,FACTORY,coin,mid));}catch{}
     }
    }else{
     const api=await net.json(`https://api-v3.raydium.io/pools/info/mint?mint1=${base.address}&mint2=${q.asset.address}&poolType=concentrated&poolSortField=liquidity&sortType=desc&pageSize=3&page=1`).catch(()=>null);
     for(const item of api?.data?.data??[])if(item.programId===RAYDIUM_CLMM)candidates.push(pool(item.id,'solana','Raydium CLMM',base,q.asset,item.feeRate,RAYDIUM_CLMM,coin,mid));
    }
   }
   // Keep at most two pools per chain, ranked by measured round-trip cost at 100 USD.
   const ranked:{p:Pool;cost:number}[]=[];
   for(const p of candidates){
    try{const qs=chain==='bsc'?(await bsc.quotesAll([p],await bsc.head(),[100])).get(p.id)??[]:await sol.quotes(p,[100]);const q=qs[0];
     if(q?.valid){ranked.push({p,cost:Number(q.buy)/Number(q.sell)-1});p.enabled=true;}else{p.reason=q?.reason??'100 美元档无有效报价';}
    }catch(e){p.reason=String(e).slice(0,120);}
   }
   ranked.sort((a,b)=>a.cost-b.cost);entry.pools.push(...ranked.slice(0,2).map(x=>x.p));
   log(`${coin} ${chain}: 候选 ${candidates.length}，可报价 ${ranked.length}，保留 ${Math.min(2,ranked.length)}`);
  }
  if(!entry.pools.length)skip('两条链都没有可报价的 PancakeSwap V3 / Raydium CLMM 池');
 }
 return {version:1,createdAt:new Date().toISOString(),source:'币安公开网络列表 + 链上精度 + 发现时 100 美元报价',takerFee:0.001,coins};
}
function pool(id:string,chain:'bsc'|'solana',dex:string,base:Asset,quote:Asset,fee:number,program:string,coin:string,mid:number):Pool{
 // initialPrice is USD per base unit: every pool of a coin quotes the same base quantity (L-003).
 return {id,chain,market:`${coin}/${quote.symbol}`,dex,base,quote,fee,program,initialPrice:mid,enabled:false};
}
async function splDecimals(net:Net,cfg:Config,mint:string){
 const r=await net.rpc(cfg.solanaRpc,'getAccountInfo',[mint,{encoding:'jsonParsed',commitment:'confirmed'}]);const d=r?.value?.data?.parsed?.info?.decimals;
 if(typeof d!=='number')throw new Error('不是 SPL mint');return d;
}
