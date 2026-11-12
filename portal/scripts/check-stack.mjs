import {createRequire} from 'node:module';
const require=createRequire(new URL('../package.json',import.meta.url));
const packages=['@vfx-js/core','@vfx-js/react','@vfx-js/transitions','@gsap/react','lenis','three','@types/three'];
const missing=packages.filter(p=>{try{require.resolve(p==='@types/three'?'@types/three/package.json':p);return false;}catch{return true;}});
if(missing.length){console.error(`Full graphics stack unavailable: ${missing.join(', ')}.\nRun npm install with registry access, then rebuild. Offline builds use explicit native/Canvas fallbacks; this is not a verified full-stack release.`);process.exitCode=1;}else console.log('Full graphics stack installed.');
