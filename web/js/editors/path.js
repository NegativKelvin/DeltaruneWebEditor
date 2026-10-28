// Path editor: drag points on a canvas, plus the properties form.
import { h } from '../ui.js';
import { jsonForm } from './jsonform.js';

export async function open(el, ectx) {
  const { capi, name, asset, setDirty } = ectx;
  const data = structuredClone(asset.data);
  data.Points ||= [];
  const cv = h('canvas', { width: 640, height: 480, style: { background: '#050508', border: '1px solid var(--line)', cursor: 'crosshair' } });
  const g = cv.getContext('2d');
  let drag = -1;
  const formBox = h('div');
  const drawForm = () => formBox.replaceChildren(jsonForm(data, () => { setDirty(true); paint(); }, ectx));
  const paint = () => {
    g.clearRect(0, 0, cv.width, cv.height);
    g.strokeStyle = '#2e2e44';
    for (let x = 0; x < cv.width; x += 32) { g.beginPath(); g.moveTo(x, 0); g.lineTo(x, cv.height); g.stroke(); }
    for (let y = 0; y < cv.height; y += 32) { g.beginPath(); g.moveTo(0, y); g.lineTo(cv.width, y); g.stroke(); }
    const pts = data.Points;
    g.strokeStyle = '#ffd400';
    g.beginPath();
    pts.forEach((p, i) => (i ? g.lineTo(p.X, p.Y) : g.moveTo(p.X, p.Y)));
    if (data.IsClosed && pts.length) g.closePath();
    g.stroke();
    pts.forEach((p, i) => {
      g.fillStyle = i === 0 ? '#4ade80' : '#36c5f0';
      g.fillRect(p.X - 3, p.Y - 3, 6, 6);
    });
  };
  const at = (e) => { const r = cv.getBoundingClientRect(); return [Math.round(e.clientX - r.left), Math.round(e.clientY - r.top)]; };
  cv.addEventListener('mousedown', (e) => {
    const [x, y] = at(e);
    drag = data.Points.findIndex((p) => Math.abs(p.X - x) < 6 && Math.abs(p.Y - y) < 6);
    if (drag < 0 && e.button === 0) {
      data.Points.push({ X: x, Y: y, Speed: 100 });
      drag = data.Points.length - 1;
      setDirty(true);
    } else if (drag >= 0 && e.button === 2) {
      data.Points.splice(drag, 1);
      drag = -1;
      setDirty(true);
    }
    paint();
  });
  cv.addEventListener('contextmenu', (e) => e.preventDefault());
  cv.addEventListener('mousemove', (e) => { if (drag < 0) return; const [x, y] = at(e); data.Points[drag].X = x; data.Points[drag].Y = y; setDirty(true); paint(); });
  window.addEventListener('mouseup', () => { if (drag >= 0) { drag = -1; drawForm(); } });
  paint();
  drawForm();
  el.append(h('p', { class: 'dim small' }, 'Click to add a point. Drag to move. Right-click to delete.'), cv, h('div', { class: 'card' }, formBox));
  return { save: () => capi.write('paths', name, { data }).then((r) => { asset.data = r.data; }) };
}
