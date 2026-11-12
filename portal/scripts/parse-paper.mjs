// Balanced-brace macro handling for the two checked-in TeX reports.
function argument(s,start){let depth=1,i=start+1;for(;i<s.length&&depth;i++){if(s[i]==='\\'){i++;continue;}if(s[i]==='{')depth++;if(s[i]==='}')depth--;}return {value:s.slice(start+1,i-1),end:i};}
function replaceCommand(s,name,transform){const token='\\'+name;let offset=0;while((offset=s.indexOf(token+'{',offset))!==-1){const arg=argument(s,offset+token.length);const replacement=transform(arg.value);s=s.slice(0,offset)+replacement+s.slice(arg.end);offset+=replacement.length;}return s;}
export function plainTex(value){
 let s=value.replace(/(?<!\\)%[^\n]*/g,'').replace(/\\(?:label|bibliographystyle)\{[^}]*\}/g,'');
 s=replaceCommand(s,'pending',v=>`[PENDING: ${v}]`);
 for(let i=0;i<4;i++)for(const macro of ['textbf','texttt','textsc','textit','emph','url','sys','detokenize'])s=replaceCommand(s,macro,v=>v);
 s=s.replace(/\\(?:eqref|ref|cite)\{([^}]*)\}/g,'[$1]').replace(/\\statis(?:\{\})?/g,'Statis').replace(/\\yes\b/g,'✓').replace(/\\no\b/g,'×').replace(/\\paragraph\{([^}]+)\}/g,'$1 ')
 .replace(/\\(?:centering|small|noindent|maketitle|newblock)\b/g,'').replace(/\\textbackslash\s*/g,'\\').replace(/\\([_&%#])/g,'$1').replace(/\\begin\{equation\}/g,()=> '$$').replace(/\\end\{equation\}/g,()=> '$$').replace(/\\S\b/g,'§').replace(/\\dots\{\}/g,'…').replace(/\\ldots/g,'…').replace(/---/g,'—').replace(/--/g,'–').replace(/~/g,' ');
 // Plain text formatting groups may remain after nested macros. Preserve math groups.
 return s.split(/(\$\$[\s\S]*?\$\$|\$[^$]*\$)/g).map(part=>part.startsWith('$')?part:part.replace(/\\,/g,' ').replace(/\{,\}/g,',').replace(/[{}]/g,'')).join('').trim();
}
function caption(value){const p=value.indexOf('\\caption{');return p<0?'':plainTex(argument(value,p+8).value);}
export function parsePaper(tex,{supplied=false}={}){
 const title=tex.match(/\\title\{([^\n]+)\}/)?.[1]||'Statis: High-Performance Seed-Deterministic Userspace Supervision for Unmodified Linux Binaries';
 const abstract=tex.match(/\\begin\{abstract\}([\s\S]*?)\\end\{abstract\}/)?.[1]||tex.match(/\\small\\noindent\\hspace\{1\.5em\}%\n([\s\S]*?)\\end\{minipage\}/)?.[1]||'';
 const end=tex.indexOf('\\bibliographystyle')>=0?tex.indexOf('\\bibliographystyle'):tex.indexOf('\\begin{thebibliography}');
 const body=tex.slice(tex.indexOf('\\section{Introduction}'),end);
 const blocks=[{type:'heading',level:2,text:'Abstract'},{type:'paragraph',text:plainTex(abstract)}];
 const token=/\\(section|subsection|paragraph)\{([^\n]+?)\}|\\begin\{(equation|figure\*?|table\*?|itemize|enumerate|algorithm)\}(?:\[[^\]]*\])?([\s\S]*?)\\end\{\3\}/g;
 let last=0;
 function paragraphs(s){for(const p of s.split(/\n\s*\n/)){const t=plainTex(p);if(t)blocks.push({type:'paragraph',text:t});}}
 for(const m of body.matchAll(token)){
  paragraphs(body.slice(last,m.index));last=m.index+m[0].length;
  if(m[1]){blocks.push({type:'heading',level:m[1]==='section'?2:m[1]==='subsection'?3:4,text:plainTex(m[2])});continue;}
  const env=m[3],value=m[4];
  if(env==='equation')blocks.push({type:'math',text:value.replace(/\\label\{[^}]*\}/g,'').replace(/\\Warp\b/g,'\\operatorname{warp}').trim()});
  else if(env==='itemize'||env==='enumerate')blocks.push({type:'list',ordered:env==='enumerate',items:value.split('\\item').slice(1).map(plainTex)});
  else if(env==='algorithm')blocks.push({type:'algorithm',caption:caption(value),source:value.trim()});
  else if(env.startsWith('table')){
   const begin=value.indexOf('\\begin{tabular}'),col=value.indexOf('{',begin+15);const arg=argument(value,col);
   const tab=value.slice(arg.end,value.indexOf('\\end{tabular}'));
   const rows=tab.replace(/\\(?:toprule|midrule|bottomrule)/g,'').split('\\\\').map(r=>r.split(/(?<!\\)&/).map(plainTex)).filter(r=>r.some(Boolean));
   blocks.push({type:'table',caption:caption(value),rows});
  }else{
   const kind=value.includes('(sched)')?'supplied-architecture':value.includes('(sup)')?'architecture':value.includes('(s1)')||value.includes('Initial stack (before)')?'auxv':value.includes('fig:trace')||value.includes('fig:tracesize')?'storage':'timewarp';
   const plots=[...value.matchAll(/coordinates\s*\{([\s\S]*?)\};/g)].map(p=>[...p[1].matchAll(/\(([^()]*?)(?<!\\),\s*([\d.e+]+)\)/g)].map(x=>[plainTex(x[1]),Number(x[2])]));
   blocks.push({type:'figure',kind,caption:caption(value),plots,source:value.trim(),supplied});
  }
 }
 paragraphs(body.slice(last));
 const refs=tex.match(/\\begin\{thebibliography\}\{\d+\}([\s\S]*?)\\end\{thebibliography\}/)?.[1]||'';
 blocks.push({type:'heading',level:2,text:'References'});
 for(const ref of refs.split(/\\bibitem\{[^}]+\}/).slice(1))blocks.push({type:'paragraph',text:plainTex(ref)});
 return {title:plainTex(title),author:'Siddid Pandey',pages:supplied?7:5,blocks};
}
