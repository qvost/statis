import {mkdtemp,mkdir,readFile,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
const portal=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..'),repo=path.dirname(portal);
const temp=await mkdtemp(path.join(tmpdir(),'statis-portal-oracle-'));
try{
 await mkdir(path.join(temp,'src'));
 await writeFile(path.join(temp,'Cargo.toml'),`[package]\nname="statis-portal-oracle"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nstatis={path=${JSON.stringify(repo)}}\nrand="0.8"\nrand_chacha="0.3"\n`);
 await writeFile(path.join(temp,'src/main.rs'),await readFile(path.join(portal,'tests/fixtures/oracle.rs')));
 const result=spawnSync('cargo',['run','--quiet','--offline','--manifest-path',path.join(temp,'Cargo.toml'),'--',path.join(portal,'tests/fixtures/format-example.trace'),path.join(portal,'tests/fixtures/rust-oracle.json')],{stdio:'inherit'});
 if(result.error)throw result.error;if(result.status!==0)throw new Error(`Rust oracle exited ${result.status}`);
 const dump=spawnSync(path.join(repo,'target/release/statis'),['trace-dump',path.join(portal,'tests/fixtures/format-example.trace')],{encoding:'utf8'});
 // Some managed executors suppress captured child output. Avoid replacing the golden dump then.
 if(dump.status!==0)throw new Error('Build release statis before generating the oracle.');
 if(dump.stdout?.trim())await writeFile(path.join(portal,'tests/fixtures/format-example.txt'),dump.stdout);
 const data=JSON.parse(await readFile(path.join(portal,'tests/fixtures/rust-oracle.json'),'utf8'));
 if(data.seeds.length!==4)throw new Error('Oracle generation incomplete.');
 console.log('Generated four Rust ChaCha20 / FaultConfig vectors and a TraceWriter format fixture.');
}finally{await rm(temp,{recursive:true,force:true});}
