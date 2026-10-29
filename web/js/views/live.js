// Hot reload: install the live object, then send flags, globals, rooms, and sprites to the running game.
import { live } from '../api.js';
import { h, attempt, add, assetDatalist, clear, loadImage } from '../ui.js';
import { readGameFlags } from './flags.js';

/** PNG strip of all frames of a sprite (frames side by side), for sprite_replace. */
async function spriteStrip(capi, name) {
  const asset = await capi.read('sprites', name);
  const frames = asset.files.filter((f) => new RegExp(`^${name}_\\d+\\.png$`).test(f))
    .sort((a, b) => Number(/_(\d+)\.png$/.exec(a)[1]) - Number(/_(\d+)\.png$/.exec(b)[1]));
  if (!frames.length) throw new Error(`${name} has no frames`);
  const imgs = await Promise.all(frames.map((f) => loadImage(capi.file('sprites', name, f), true)));
  if (imgs.some((i) => !i)) throw new Error(`cannot load the frames of ${name}`);
  const w = asset.data.Width || imgs[0].width, hh = asset.data.Height || imgs[0].height;
  const c = document.createElement('canvas');
  c.width = w * imgs.length;
  c.height = hh;
  const g = c.getContext('2d');
  imgs.forEach((img, i) => g.drawImage(img, i * w, 0));
  const blob = await new Promise((r) => c.toBlob(r, 'image/png'));
  return { blob, frames: imgs.length, xo: asset.data.OriginX || 0, yo: asset.data.OriginY || 0 };
}

export async function render(main, ctx) {
  const { capi, ch } = ctx;
  const page = h('div', { class: 'page' });
  main.append(page);
  const statusBox = h('div', { class: 'card' });
  const installBox = h('div', { class: 'card' });
  const send = (cmds, msg) => attempt(() => live.send(cmds), msg || 'Sent to game');
  const input = (attrs) => h('input', { type: 'text', ...attrs });

  const room = input({ placeholder: 'room name', list: assetDatalist(capi, ch, 'rooms') });
  const flagN = h('input', { type: 'number', placeholder: 'n', step: 1, min: 0 });
  const flagV = input({ placeholder: 'value', style: { width: '90px' } });
  const gName = input({ placeholder: 'plot' });
  const gV = input({ placeholder: 'value', style: { width: '90px' } });
  const gStr = h('input', { type: 'checkbox' });
  const oName = input({ placeholder: 'object', list: assetDatalist(capi, ch, 'objects') });
  const oVar = input({ placeholder: 'variable' });
  const oV = input({ placeholder: 'number', style: { width: '90px' } });
  const spr = input({ placeholder: 'sprite', list: assetDatalist(capi, ch, 'sprites') });
  const gameFlags = h('div', { class: 'mono small' });

  add(page,
    h('h2', null, 'Hot reload'),
    h('p', { class: 'dim' }, 'Change the running game without a rebuild. A small persistent object (obj_dre_live) reads commands from the save folder 3 times per second.'),
    statusBox, installBox,
    h('div', { class: 'card col' },
      h('h3', null, 'Send to the running game'),
      h('div', { class: 'row' }, room, h('button', { onclick: () => room.value && send([{ op: 'goto', args: [room.value] }]) }, 'Go to room'),
        h('button', { onclick: () => send([{ op: 'restart', args: [] }]) }, 'Restart room')),
      h('div', { class: 'row' }, h('span', { class: 'dim' }, 'global.flag['), flagN, h('span', { class: 'dim' }, '] ='), flagV,
        h('button', { onclick: () => send([{ op: /^-?\d+(\.\d+)?$/.test(flagV.value) ? 'flag' : 'flagstr', args: [flagN.value, flagV.value] }]) }, 'Set flag')),
      h('div', { class: 'row' }, h('span', { class: 'dim' }, 'global.'), gName, h('span', { class: 'dim' }, '='), gV,
        h('label', { class: 'field check' }, gStr, h('span', null, 'text')),
        h('button', { onclick: () => send([{ op: gStr.checked ? 'globalstr' : 'global', args: [gName.value, gV.value] }]) }, 'Set global')),
      h('div', { class: 'row' }, oName, h('span', { class: 'dim' }, '.'), oVar, h('span', { class: 'dim' }, '='), oV,
        h('button', { onclick: () => send([{ op: 'var', args: [oName.value, oVar.value, oV.value] }]) }, 'Set on all instances')),
      h('div', { class: 'row' }, spr, h('button', { onclick: async () => {
        if (!spr.value) return;
        const s = await attempt(() => spriteStrip(capi, spr.value));
        if (s) await attempt(() => live.sprite(spr.value, s.blob, s.frames, s.xo, s.yo), `Sent ${spr.value} (${s.frames} frames)`);
      } }, 'Send sprite (saved frames)')),
      h('div', { class: 'row' }, h('button', { onclick: async () => {
        const v = await readGameFlags();
        if (v) gameFlags.textContent = v.map((x, i) => (x !== '0' && x !== '' ? `[${i}]=${x}` : null)).filter(Boolean).join('  ') || 'All flags are 0.';
      } }, 'Read flags from game'), h('span', { class: 'dim small' }, 'Flags that are not 0:')),
      gameFlags),
    h('div', { class: 'note warn' },
      h('b', null, 'Limits. '),
      'GameMaker cannot load new code while the game runs, so changes to code, objects, rooms (layout, instances, creation code), dialogue text, sounds, and shaders need a build and a game restart. ',
      'Hot reload changes data only: global variables and flags, instance variables, the current room, and sprite images. ',
      'A replaced sprite keeps its size and frame count from the PNG strip; the game forgets it at restart. ',
      'Values are numbers or text (no GML expressions). Commands run at the next poll (about 0.3 s).'));

  async function drawInstall() {
    const st = await attempt(() => capi.liveStatus());
    if (!st) return;
    const host = input({ value: st.host || st.default_hosts[0] || '', placeholder: 'code entry that runs every step', list: assetDatalist(capi, ch, 'code'), style: { width: '340px' } });
    add(clear(installBox),
      h('div', { class: 'row' }, h('h3', { class: 'grow' }, `Live object in ${ch}`),
        st.installed ? h('span', { class: 'badge new' }, 'installed') : h('span', { class: 'badge' }, 'not installed')),
      h('div', { class: 'row' }, h('label', { class: 'field' }, h('span', null, 'Host code (gets one line that makes obj_dre_live)'), host),
        h('button', { class: 'primary', onclick: async () => { if (await attempt(() => capi.liveInstall(host.value.trim()), 'Installed in mod')) drawInstall(); } }, st.installed ? 'Install again' : 'Install in mod'),
        st.installed || st.object || st.hooked ? h('button', { class: 'danger', onclick: async () => { if (await attempt(() => capi.liveUninstall(), 'Removed from mod')) drawInstall(); } }, 'Remove') : null),
      st.default_hosts.length || st.host ? null : h('p', { class: 'note warn' }, 'obj_time is not in this chapter. Name a code entry that runs every step (for example a Step event of a persistent object).'),
      h('p', { class: 'dim small' }, 'After install: build the chapter (Overview and build) and start the game with the built data.win. Then the status above shows "connected".'));
  }

  let timer = null;
  async function poll() {
    try {
      const s = await live.status();
      add(clear(statusBox), h('div', { class: 'row' },
        s.connected ? h('span', { class: 'badge new' }, 'game connected') : h('span', { class: 'badge' }, 'game not connected'),
        s.ack ? h('span', null, `room ${s.ack.room || '?'} · plot ${s.ack.plot || '?'} · ${s.ack.done} commands run · last answer ${Math.round(s.ack.age_ms / 1000)} s ago`) : h('span', { class: 'dim' }, 'No answer from the game yet.'),
        s.pending ? h('span', { class: 'badge mod' }, `${s.pending} waiting`) : null),
      h('p', { class: 'dim mono small' }, s.dir));
    } catch (e) {
      add(clear(statusBox), h('p', null, `${e.message}. Set the save folder in `, h('a', { href: '#/' }, 'Setup'), '.'));
      clearInterval(timer);
    }
  }
  await poll();
  timer = setInterval(poll, 1000);
  drawInstall();
  return () => clearInterval(timer);
}
