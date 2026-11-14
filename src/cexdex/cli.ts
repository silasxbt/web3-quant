import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';import {dirname,resolve} from 'node:path';
import {Net} from '../net.js';import {validateConfig} from '../config.js';import {Store} from '../store.js';
import {discover,type Registry} from './registry.js';import {collect} from './collect.js';import {analyze,markdown} from './analyze.js';
const REGISTRY='config/cexdex-registry.json';
export async function run(command:string,args:string[],store:Store){
 if(command==='cexdex-discover'){
  const out=args[0]??REGISTRY,net=new Net(resolve(store.root,'raw','discover-'+Date.now()+'.ndjson.gz'));
  const r=await discover(net,validateConfig({maxImpactBps:300}),m=>console.error(m));mkdirSync(dirname(out),{recursive:true});writeFileSync(out,JSON.stringify(r,null,1)+'\n');
  console.log(JSON.stringify({out,coins:r.coins.filter(c=>!c.excluded).map(c=>({coin:c.coin,pools:c.pools.map(p=>p.chain+' '+p.market+' '+p.fee)}))}));
 }else if(command==='cexdex-collect'){
  const minutes=Number(args[0]??30),registry:Registry=JSON.parse(readFileSync(args[1]??REGISTRY,'utf8'));let stop=false;for(const sig of ['SIGINT','SIGTERM'] as const)process.on(sig,()=>{stop=true;});
  const s=await collect(store,registry,{durationMinutes:minutes},()=>stop,m=>console.log(m));console.log(JSON.stringify({id:s.id,status:s.status,cycles:store.cycles(s.id).length}));
 }else if(command==='cexdex-report'){
  const id=args[0]??store.sessions()[0]?.id;if(!id)throw new Error('暂无会话');const s=store.session(id) as any;if(s.kind!=='cexdex')throw new Error('不是 DDA-012 会话');
  const r=analyze(s,store.cycles(id) as any),out=args[1]??`exports/cexdex-${id}.md`;mkdirSync(dirname(out),{recursive:true});writeFileSync(out,markdown(r));writeFileSync(out.replace(/\.md$/,'.json'),JSON.stringify(r,null,1));
  console.log(JSON.stringify({out,digest:r.digest,cycles:r.quality.cycles,groups:r.summary.groups,withPositive:r.summary.withPositive}));
 }else throw new Error('未知命令 '+command);
}
