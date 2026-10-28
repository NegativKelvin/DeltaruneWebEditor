// Shader editor: edit every shader source file, and preview the GLSL ES pair live in WebGL
// on a sprite, with controls for each uniform.
import { h, attempt, debounce, assetDatalist, pickFiles, toast, add } from '../ui.js';
import { codeEditor } from './codeedit.js';

const TEXT_FILES = ['GLSL_ES_Vertex.txt', 'GLSL_ES_Fragment.txt', 'GLSL_Vertex.txt', 'GLSL_Fragment.txt', 'HLSL9_Vertex.txt', 'HLSL9_Fragment.txt'];

// GameMaker adds this header before shader sources. Only the parts that a source lacks are added.
const VS_HEAD = [
  [/#define\s+MATRIX_VIEW\b/, '#define MATRIX_VIEW 0\n#define MATRIX_PROJECTION 1\n#define MATRIX_WORLD 2\n#define MATRIX_WORLD_VIEW 3\n#define MATRIX_WORLD_VIEW_PROJECTION 4\n#define MATRICES_MAX 5'],
  [/uniform\s+\w+\s+gm_Matrices\b/, 'uniform mat4 gm_Matrices[MATRICES_MAX];'],
  [/uniform\s+\w+\s+gm_LightingEnabled\b/, 'uniform bool gm_LightingEnabled;'],
  [/uniform\s+\w+\s+gm_VS_FogEnabled\b/, 'uniform bool gm_VS_FogEnabled;\nuniform float gm_FogStart;\nuniform float gm_RcpFogRange;'],
];
const FS_HEAD = [
  [/precision\s+\w+\s+float/, 'precision mediump float;'],
  [/uniform\s+\w+\s+gm_BaseTexture\b/, 'uniform sampler2D gm_BaseTexture;'],
  [/uniform\s+\w+\s+gm_PS_FogEnabled\b/, 'uniform bool gm_PS_FogEnabled;\nuniform vec4 gm_FogColour;'],
  [/uniform\s+\w+\s+gm_AlphaTestEnabled\b/, 'uniform bool gm_AlphaTestEnabled;\nuniform float gm_AlphaRefValue;'],
];

function prepare(src, head) {
  const body = src.replace(/^\s*#version.*$/gm, '');
  const add = head.filter(([re]) => !re.test(body)).map(([, text]) => text);
  return { text: add.join('\n') + '\n' + body, extraLines: add.join('\n').split('\n').length };
}

function parseUniforms(src) {
  const out = [];
  const re = /uniform\s+(?:lowp\s+|mediump\s+|highp\s+)?(float|int|bool|vec2|vec3|vec4)\s+(\w+)\s*(?:\[\s*\d+\s*\])?\s*;/g;
  let m;
  while ((m = re.exec(src))) if (!m[2].startsWith('gm_')) out.push({ type: m[1], name: m[2] });
  return out;
}

export async function open(el, ectx) {
  const { capi, ch, name, asset, setDirty } = ectx;
  const sources = {};
  const changed = new Set();
  for (const f of TEXT_FILES) {
    sources[f] = asset.files.includes(f) ? await (await capi.readFile('shaders', name, f)).text() : '';
  }
  const binaries = asset.files.filter((f) => f.endsWith('.bin'));

  // ---- Source tabs ----
  const editors = {};
  const tabHead = h('div', { class: 'tabs' });
  const tabBody = h('div');
  let current = 'GLSL_ES_Fragment.txt';
  const showTab = (f) => {
    current = f;
    for (const b of tabHead.children) b.classList.toggle('active', b.dataset.f === f);
    for (const [k, e] of Object.entries(editors)) e.el.hidden = k !== f;
  };
  for (const f of TEXT_FILES) {
    tabHead.append(h('button', { dataset: { f }, onclick: () => showTab(f) }, f.replace('.txt', '').replace(/_/g, ' '), asset.files.includes(f) ? '' : ' (empty)'));
    editors[f] = codeEditor({ value: sources[f], lang: 'glsl', height: '52vh', onSave: ectx.save, onChange: (v) => {
      sources[f] = v;
      changed.add(f);
      setDirty(true);
      if (f.startsWith('GLSL_ES')) recompile();
    } });
    tabBody.append(editors[f].el);
  }
  showTab(current);

  // ---- Preview ----
  const cv = h('canvas', { width: 384, height: 288, style: { background: '#000', border: '1px solid var(--line)', imageRendering: 'pixelated' } });
  const errors = h('pre', { class: 'log', style: { color: 'var(--danger)', maxHeight: '160px' }, hidden: true });
  const uniBox = h('div', { class: 'col' });
  const spriteIn = h('input', { type: 'text', placeholder: 'sprite for preview', list: assetDatalist(capi, ch, 'sprites'), style: { width: '200px' } });
  const gl = cv.getContext('webgl', { premultipliedAlpha: false, preserveDrawingBuffer: true });
  let prog = null, tex = null, texW = 64, texH = 64, raf = 0, t0 = performance.now();
  const uniVals = {};

  function testPattern() {
    const c = document.createElement('canvas');
    c.width = 64; c.height = 64;
    const g = c.getContext('2d');
    for (let y = 0; y < 8; y++) for (let x = 0; x < 8; x++) { g.fillStyle = (x + y) % 2 ? '#ffd400' : '#36c5f0'; g.fillRect(x * 8, y * 8, 8, 8); }
    g.fillStyle = '#fff'; g.font = 'bold 20px monospace'; g.fillText('DR', 18, 40);
    return c;
  }
  function setTexture(source) {
    if (!gl) return;
    tex = tex || gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
    for (const [k, v] of [[gl.TEXTURE_MIN_FILTER, gl.NEAREST], [gl.TEXTURE_MAG_FILTER, gl.NEAREST], [gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE], [gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE]]) gl.texParameteri(gl.TEXTURE_2D, k, v);
    texW = source.width; texH = source.height;
    draw();
  }
  async function loadSprite() {
    const s = spriteIn.value.trim();
    if (!s) { setTexture(testPattern()); return; }
    try {
      const blob = await (await capi.readFile('sprites', s, `${s}_0.png`)).blob();
      setTexture(await createImageBitmap(blob));
    } catch { toast(`No frame 0 for sprite ${s}.`, 'error'); }
  }
  spriteIn.addEventListener('change', loadSprite);

  function compile(type, src, headLines) {
    const s = gl.createShader(type);
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
      const log = gl.getShaderInfoLog(s) || 'compile error';
      // Report lines of the original source, not of the added header.
      throw new Error(log.replace(/ERROR: 0:(\d+)/g, (_, n) => `ERROR: line ${Math.max(1, n - headLines)}`));
    }
    return s;
  }
  function recompileNow() {
    if (!gl) { errors.hidden = false; errors.textContent = 'WebGL is not available in this browser.'; return; }
    try {
      const vs = prepare(sources['GLSL_ES_Vertex.txt'], VS_HEAD);
      const fs = prepare(sources['GLSL_ES_Fragment.txt'], FS_HEAD);
      const p = gl.createProgram();
      gl.attachShader(p, compile(gl.VERTEX_SHADER, vs.text, vs.extraLines));
      gl.attachShader(p, compile(gl.FRAGMENT_SHADER, fs.text, fs.extraLines));
      gl.linkProgram(p);
      if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
      prog = p;
      errors.hidden = true;
      buildUniforms();
      draw();
    } catch (e) {
      errors.hidden = false;
      errors.textContent = e.message;
    }
  }
  const recompile = debounce(recompileNow, 300);

  function buildUniforms() {
    const list = [...parseUniforms(sources['GLSL_ES_Vertex.txt']), ...parseUniforms(sources['GLSL_ES_Fragment.txt'])];
    const seen = new Set();
    uniBox.replaceChildren(h('b', null, 'Uniforms'));
    for (const u of list) {
      if (seen.has(u.name)) continue;
      seen.add(u.name);
      const n = { float: 1, int: 1, bool: 1, vec2: 2, vec3: 3, vec4: 4 }[u.type];
      if (!uniVals[u.name] || uniVals[u.name].v.length !== n) uniVals[u.name] = { type: u.type, v: Array(n).fill(u.type === 'vec4' || u.type === 'vec3' ? 1 : 0), auto: /time|timer/i.test(u.name) };
      const st = uniVals[u.name];
      st.type = u.type;
      const row = h('div', { class: 'row small' }, h('span', { class: 'mono', style: { minWidth: '110px' } }, `${u.type} ${u.name}`));
      st.v.forEach((val, i) => row.append(h('input', { type: 'number', step: u.type === 'int' ? 1 : 0.01, value: val, style: { width: '64px' }, oninput: (e) => { st.v[i] = +e.target.value; draw(); } })));
      if (n === 1 && u.type !== 'bool') row.append(h('label', { class: 'field check' }, h('input', { type: 'checkbox', checked: st.auto, onchange: (e) => { st.auto = e.target.checked; loop(); } }), h('span', null, 'animate (+1/frame)')));
      uniBox.append(row);
    }
    if (seen.size === 0) uniBox.append(h('span', { class: 'dim small' }, 'No custom uniforms.'));
    loop();
  }

  function draw() {
    if (!gl || !prog || !tex) return;
    gl.viewport(0, 0, cv.width, cv.height);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.useProgram(prog);
    const s = Math.max(1, Math.floor(Math.min(cv.width / texW, cv.height / texH)));
    const w = texW * s, hh = texH * s, x = (cv.width - w) / 2, y = (cv.height - hh) / 2;
    const attrib = (nm, size, arr) => {
      const loc = gl.getAttribLocation(prog, nm);
      if (loc < 0) return;
      const b = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, b);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(arr), gl.STATIC_DRAW);
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, size, gl.FLOAT, false, 0, 0);
    };
    attrib('in_Position', 3, [x, y, 0, x + w, y, 0, x, y + hh, 0, x, y + hh, 0, x + w, y, 0, x + w, y + hh, 0]);
    attrib('in_Colour', 4, Array(6).fill([1, 1, 1, 1]).flat());
    attrib('in_TextureCoord', 2, [0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1]);
    // gm_Matrices: view, projection, world, world-view, world-view-projection.
    const W = cv.width, H = cv.height;
    const proj = [2 / W, 0, 0, 0, 0, -2 / H, 0, 0, 0, 0, 1, 0, -1, 1, 0, 1];
    const id = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
    const mloc = gl.getUniformLocation(prog, 'gm_Matrices');
    if (mloc) gl.uniformMatrix4fv(mloc, false, new Float32Array([...id, ...proj, ...id, ...id, ...proj]));
    const tloc = gl.getUniformLocation(prog, 'gm_BaseTexture');
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    if (tloc) gl.uniform1i(tloc, 0);
    for (const [nm, st] of Object.entries(uniVals)) {
      const loc = gl.getUniformLocation(prog, nm);
      if (!loc) continue;
      if (st.type === 'int' || st.type === 'bool') gl.uniform1i(loc, st.v[0] | 0);
      else gl[`uniform${st.v.length}fv`](loc, new Float32Array(st.v));
    }
    gl.drawArrays(gl.TRIANGLES, 0, 6);
  }
  function loop() {
    cancelAnimationFrame(raf);
    if (!Object.values(uniVals).some((u) => u.auto)) return;
    const step = () => {
      for (const u of Object.values(uniVals)) if (u.auto) u.v[0] += 1;
      draw();
      raf = requestAnimationFrame(step);
    };
    t0 = performance.now();
    raf = requestAnimationFrame(step);
  }

  const binBox = h('div', { class: 'col small' },
    h('b', null, 'Compiled shader data (.bin)'),
    binaries.length ? binaries.map((f) => h('div', { class: 'row' }, h('span', { class: 'mono' }, f),
      h('a', { class: 'button small', href: capi.file('shaders', name, f), download: f }, 'Download'),
      h('button', { class: 'small', onclick: async () => {
        const [file] = await pickFiles('.bin,.cso,.fxo');
        if (file) await attempt(() => capi.writeFile('shaders', name, f, file), `Replaced ${f}`);
      } }, 'Replace'))) : h('span', { class: 'dim' }, 'None.'),
    h('button', { class: 'small', onclick: async () => {
      const files = await pickFiles('.bin', true);
      for (const file of files) await attempt(() => capi.writeFile('shaders', name, file.name, file), `Added ${file.name}`);
    } }, 'Add .bin (for example HLSL11_PixelData.bin)'));

  add(el,
    h('div', { class: 'note warn small' }, 'The Windows game draws with the compiled HLSL11 data (HLSL11_VertexData.bin, HLSL11_PixelData.bin) when the data file has it. ',
      'The preview and other platforms use GLSL ES. After you change GLSL ES, compile the matching HLSL 11 shader (vs_4_0 / ps_4_0, for example with fxc) and replace the .bin files below.'),
    h('div', { class: 'row', style: { alignItems: 'flex-start', flexWrap: 'nowrap' } },
      h('div', { class: 'grow' }, tabHead, tabBody),
      h('div', { class: 'col', style: { width: '400px', flexShrink: 0 } },
        h('div', { class: 'row' }, h('b', null, 'Live preview (GLSL ES)'), spriteIn, h('button', { class: 'small', onclick: () => { recompileNow(); loadSprite(); } }, 'Reload')),
        cv, errors, uniBox,
        h('a', { class: 'small', href: `#/${encodeURIComponent(ch)}/search?q=${encodeURIComponent(name)}` }, `Find code that uses ${name}`),
        binBox)));

  setTexture(testPattern());
  recompileNow();

  return {
    save: async () => {
      for (const f of changed) await capi.writeFile('shaders', name, f, new Blob([sources[f]], { type: 'text/plain' }));
      changed.clear();
    },
    dispose: () => cancelAnimationFrame(raf),
  };
}
