// Enemies and encounters: values set with global.monster*[i] = ... in switch cases
// (scr_monstersetup, scr_encountersetup, ...). A change replaces only that value in the code.
import { h, attempt, toast } from '../ui.js';
import { setDirtyCheck } from '../app.js';
import { displayExpr } from './textpreview.js';

const STAT_FIELDS = ['monstername', 'monstermaxhp', 'monsterhp', 'monsterat', 'monsterdf', 'monsterexp', 'monstergold'];
const ENC_FIELDS = ['monsterinstancetype', 'monstertype', 'monstermakex', 'monstermakey'];

export async function render(main, ctx) {
  const { capi, ch } = ctx;
  const c = encodeURIComponent(ch);
  const page = h('div', { class: 'page', style: { maxWidth: 'none' } });
  main.append(page);
  const records = await capi.enemies();
  const objIdx = await capi.index('objects', ['Sprite']).catch(() => ({}));
  const edits = new Map(); // "code|start" -> {rec, field, value}
  setDirtyCheck(() => edits.size > 0);
  const saveBtn = h('button', { class: 'primary', disabled: true, onclick: () => save() }, 'Save changes');

  const monsters = records.filter((r) => STAT_FIELDS.some((f) => r.fields[f]));
  const encRecs = records.filter((r) => !monsters.includes(r) && ENC_FIELDS.some((f) => r.fields[f]));
  const other = records.filter((r) => !monsters.includes(r) && !encRecs.includes(r));
  const monsterByCase = new Map(monsters.map((m) => [m.case, m]));

  const input = (rec, field, width = 70) => {
    const f = rec.fields[field];
    if (!f) return h('span', { class: 'dim' }, '-');
    const key = `${rec.code}|${f.start}`;
    return h('input', { type: 'text', class: 'mono', value: edits.get(key)?.value ?? f.expr, title: f.expr, style: { width: `${width}px` }, oninput: (e) => {
      if (e.target.value === f.expr) edits.delete(key); else edits.set(key, { rec, field, value: e.target.value });
      e.target.style.borderColor = edits.has(key) ? 'var(--accent)' : '';
      saveBtn.disabled = !edits.size;
      saveBtn.textContent = edits.size ? `Save changes (${edits.size})` : 'Save changes';
    } });
  };
  const codeLink = (rec, line) => h('a', { class: 'mono small', href: `#/${c}/assets/code/${encodeURIComponent(rec.code)}?line=${line}` }, `${rec.code.replace(/^gml_(Object_|Script_|GlobalScript_)?/, '')}`);
  const objLink = (expr) => {
    const o = expr && expr.trim();
    if (o && /^\d+$/.test(o)) return h('span', { class: 'dim small' }, `object index ${o}`);
    if (!o || !objIdx[o]) return null;
    const spr = objIdx[o].Sprite;
    return h('span', { class: 'row' },
      spr ? h('img', { class: 'thumb', style: { maxHeight: '32px' }, src: capi.file('sprites', spr, `${spr}_0.png`) }) : null,
      h('a', { class: 'small', href: `#/${c}/assets/objects/${encodeURIComponent(o)}` }, 'open object'));
  };

  page.append(h('div', { class: 'row' }, h('h2', { class: 'grow' }, 'Enemies and encounters'), saveBtn),
    h('p', { class: 'dim small' }, 'Values are GML expressions. Enemy behaviour (attacks, ACTs, text) is in the enemy objects: open them from the encounter list. ',
      'Bullet patterns and the battle box: see Test › Bullet patterns.'));

  // Enemy stats
  const extraFields = [...new Set(monsters.flatMap((m) => Object.keys(m.fields)))].filter((f) => !STAT_FIELDS.includes(f)).sort();
  page.append(h('div', { class: 'card', style: { overflowX: 'auto' } }, h('h3', null, `Enemy stats (${monsters.length})`),
    monsters.length ? h('table', null,
      h('tr', null, h('th', null, 'Case'), h('th', null, 'Name'), ...STAT_FIELDS.slice(1).map((f) => h('th', null, f.replace('monster', ''))), h('th', null, 'Other fields'), h('th', null, 'Code')),
      monsters.map((m) => h('tr', null,
        h('td', { class: 'mono' }, m.case ?? '-'),
        h('td', null, input(m, 'monstername', 260), m.fields.monstername ? h('div', { class: 'small' }, displayExpr(m.fields.monstername.expr)) : null),
        ...STAT_FIELDS.slice(1).map((f) => h('td', null, input(m, f, 60))),
        h('td', null, extraFields.filter((f) => m.fields[f]).length ? h('details', null, h('summary', null, `${extraFields.filter((f) => m.fields[f]).length} more`),
          extraFields.filter((f) => m.fields[f]).map((f) => h('div', { class: 'row small' }, h('span', { class: 'mono dim', style: { minWidth: '140px' } }, f), input(m, f, 200)))) : null),
        h('td', null, codeLink(m, m.fields[Object.keys(m.fields)[0]].line))))) : h('p', { class: 'dim' }, 'No global.monstername / monstermaxhp assignments found.')));

  // Encounters: group by code + case.
  const groups = new Map();
  for (const r of encRecs) {
    const k = `${r.code}|${r.case}`;
    if (!groups.has(k)) groups.set(k, []);
    groups.get(k).push(r);
  }
  const encBox = h('div', { class: 'card' }, h('h3', null, `Encounters (${groups.size})`));
  for (const [, recs] of groups) {
    const first = recs[0];
    const msg = recs.find((r) => r.fields.battlemsg);
    encBox.append(h('div', { class: 'block' },
      h('div', { class: 'block-head' }, h('b', null, `case ${first.case ?? '-'}`), codeLink(first, first.fields[Object.keys(first.fields)[0]].line),
        msg ? h('span', null, 'Text: ', input(msg, 'battlemsg', 380), h('span', { class: 'small' }, ` ${displayExpr(msg.fields.battlemsg.expr)}`)) : null),
      h('table', null, h('tr', null, h('th', null, 'Slot'), h('th', null, 'Enemy object'), h('th', null, 'Enemy type'), h('th', null, 'X'), h('th', null, 'Y')),
        recs.filter((r) => ENC_FIELDS.some((f) => r.fields[f])).map((r) => {
          const t = r.fields.monstertype?.expr?.trim();
          const m = t !== undefined ? monsterByCase.get(t) : null;
          return h('tr', null, h('td', { class: 'mono' }, r.index),
            h('td', null, h('div', { class: 'row' }, input(r, 'monsterinstancetype', 200), objLink(r.fields.monsterinstancetype?.expr))),
            h('td', null, h('div', { class: 'row' }, input(r, 'monstertype', 50), m?.fields.monstername ? h('span', { class: 'small' }, displayExpr(m.fields.monstername.expr)) : null)),
            h('td', null, input(r, 'monstermakex', 120)), h('td', null, input(r, 'monstermakey', 120)));
        }))));
  }
  if (!groups.size) encBox.append(h('p', { class: 'dim' }, 'No global.monstertype / monsterinstancetype assignments found.'));
  page.append(encBox);

  if (other.length) {
    page.append(h('details', { class: 'card' }, h('summary', null, `Other monster assignments (${other.length})`),
      h('table', null, other.map((r) => h('tr', null, h('td', null, codeLink(r, r.fields[Object.keys(r.fields)[0]].line)), h('td', { class: 'mono' }, `case ${r.case ?? '-'} [${r.index}]`),
        h('td', null, Object.keys(r.fields).map((f) => h('div', { class: 'row small' }, h('span', { class: 'mono dim', style: { minWidth: '160px' } }, f), input(r, f, 240)))))))));
  }

  // Enemy objects
  const enemyObjs = await capi.list('objects', { q: 'enemy', limit: 500 }).catch(() => ({ items: [] }));
  page.append(h('div', { class: 'card' }, h('h3', null, `Enemy objects (${enemyObjs.items.length})`),
    h('div', { class: 'fields' }, enemyObjs.items.map((o) => h('a', { class: 'button small', href: `#/${c}/assets/objects/${encodeURIComponent(o.name)}` }, o.name)))));

  async function save() {
    const byCode = new Map();
    for (const { rec, field, value } of edits.values()) {
      if (!value.trim()) { toast(`${field} must not be empty.`, 'error'); return; }
      if (!byCode.has(rec.code)) byCode.set(rec.code, { rev: rec.rev, edits: [] });
      const f = rec.fields[field];
      byCode.get(rec.code).edits.push({ start: f.start, end: f.end, text: value });
    }
    let ok = 0;
    for (const [code, p] of byCode) if (await attempt(() => capi.patchCode(code, p.rev, p.edits))) ok++;
    toast(`Saved ${ok} of ${byCode.size} code entries`, ok === byCode.size ? 'ok' : 'error');
    edits.clear();
    location.hash = `#/${c}/enemies?r=${Date.now()}`;
  }
}
