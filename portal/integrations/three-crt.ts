import * as THREE from 'three';
import { createCRT as fallback, type CRTHandle } from '../src/lib/crt-fallback';
// No model, image, or video downloads: mesh geometry and canvas textures only.
export function createCRT(host: HTMLElement, initial: string, reduced: boolean): CRTHandle {
    if (reduced)
        return fallback(host, initial, true);
    const canvas = document.createElement('canvas');
    const context = canvas.getContext('webgl2', { alpha: true, antialias: true, powerPreference: 'low-power' });
    if (!context)
        return fallback(host, initial, reduced);
    const renderer = new THREE.WebGLRenderer({ canvas, context, alpha: true, antialias: true });
    renderer.setPixelRatio(Math.min(devicePixelRatio, 1.5));
    renderer.setClearColor('#071d1a', 1);
    host.append(canvas);
    const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(32, 1, .1, 100);
    camera.position.z = 5.1;
    const textCanvas = document.createElement('canvas');
    textCanvas.width = 1200;
    textCanvas.height = 700;
    const ctx = textCanvas.getContext('2d')!;
    let text = initial, tear = 0, frame = 0, visible = true;
    const texture = new THREE.CanvasTexture(textCanvas);
    texture.minFilter = THREE.LinearFilter;
    texture.magFilter = THREE.LinearFilter;
    texture.generateMipmaps = false;
    function paint() { ctx.fillStyle = '#071d1a'; ctx.fillRect(0, 0, 1200, 700); ctx.fillStyle = '#9ae7bc'; ctx.font = '22px monospace'; text.split('\n').forEach((line, i) => ctx.fillText(line, 50, 65 + i * 39)); texture.needsUpdate = true; }
    const screenGeometry = new THREE.PlaneGeometry(4, 2.333, 48, 32);
    const pos = screenGeometry.attributes.position;
    for (let i = 0; i < pos.count; i++) {
        const x = pos.getX(i), y = pos.getY(i);
        pos.setZ(i, -.055 * (x * x + y * y));
    }
    screenGeometry.computeVertexNormals();
    const material = new THREE.ShaderMaterial({ uniforms: { tube: { value: texture }, time: { value: 0 }, tear: { value: 0 }, resolution: { value: new THREE.Vector2(1200, 700) } }, vertexShader: `varying vec2 vUv;void main(){vUv=uv;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}`, fragmentShader: `precision highp float;varying vec2 vUv;uniform sampler2D tube;uniform vec2 resolution;uniform float time;uniform float tear;float hash(float n){return fract(sin(n)*43758.5453);}void main(){vec2 p=vUv;vec2 centered=p-.5;p=centered*(1.+.065*dot(centered,centered))+.5;float band=step(.94,hash(floor(p.y*35.)+floor(time*18.)));p.x+=(hash(floor(time*27.))-0.5)*tear*band*.045;float shift=.0015+tear*band*.008;vec3 c=vec3(texture2D(tube,p+vec2(shift,0.)).r,texture2D(tube,p).g,texture2D(tube,p-vec2(shift,0.)).b);float scan=.88+.12*sin(p.y*resolution.y*3.14159);float grille=mod(floor(p.x*resolution.x),3.);vec3 mask=grille<1.?vec3(1.,.84,.84):grille<2.?vec3(.84,1.,.84):vec3(.84,.84,1.);float vignette=pow(max(0.,1.-dot(centered*1.35,centered*1.35)),.55);float edge=step(0.,p.x)*step(p.x,1.)*step(0.,p.y)*step(p.y,1.);c*=scan*mask*vignette*edge;c+=vec3(hash(dot(p,vec2(27.,819.))+time)*.009);gl_FragColor=vec4(c,1.);}` });
    const screen = new THREE.Mesh(screenGeometry, material);
    scene.add(screen);
    const bezelGeometry = new THREE.BoxGeometry(4.26, 2.58, .16), bezelMaterial = new THREE.MeshBasicMaterial({ color: '#202821' }), bezel = new THREE.Mesh(bezelGeometry, bezelMaterial);
    bezel.position.z = -.25;
    scene.add(bezel);
    const standGeometry = new THREE.BoxGeometry(1.1, .18, .2), standMaterial = new THREE.MeshBasicMaterial({ color: '#202821' }), stand = new THREE.Mesh(standGeometry, standMaterial);
    stand.position.set(0, -1.35, -.25);
    scene.add(stand);
    const resize = new ResizeObserver(() => { const r = host.getBoundingClientRect(); if (r.width && r.height) {
        renderer.setSize(r.width, r.height, false);
        camera.aspect = r.width / r.height;
        camera.updateProjectionMatrix();
    } });
    resize.observe(host);
    const draw = (t: number) => { material.uniforms.time.value = t / 1000; tear *= .92; material.uniforms.tear.value = tear; renderer.render(scene, camera); if (visible)
        frame = requestAnimationFrame(draw); };
    const observer = new IntersectionObserver(([e]) => { visible = e.isIntersecting; cancelAnimationFrame(frame); if (visible)
        draw(performance.now()); });
    observer.observe(host);
    const lost = (e: Event) => { e.preventDefault(); visible = false; cancelAnimationFrame(frame); canvas.style.display = 'none'; };
    canvas.addEventListener('webglcontextlost', lost);
    paint();
    draw(0);
    return { update: (value: string) => { text = value; tear = 1; paint(); }, dispose: () => { visible = false; cancelAnimationFrame(frame); resize.disconnect(); observer.disconnect(); canvas.removeEventListener('webglcontextlost', lost); screenGeometry.dispose(); bezelGeometry.dispose(); standGeometry.dispose(); material.dispose(); bezelMaterial.dispose(); standMaterial.dispose(); texture.dispose(); renderer.dispose(); canvas.remove(); } };
}
