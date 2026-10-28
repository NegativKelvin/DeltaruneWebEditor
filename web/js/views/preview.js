// Room preview page: pick a room and walk in it (saved data). The room editor has a Play button for unsaved edits.
import { h, assetDatalist, errMsg } from '../ui.js';
import { go } from '../app.js';
import { createPlayer } from '../editors/roomplay.js';

export async function render(main, ctx) {
  const { capi, ch, arg } = ctx;
  const c = encodeURIComponent(ch);
  const input = h('input', { type: 'text', value: arg || '', placeholder: 'room name', list: assetDatalist(capi, ch, 'rooms'), style: { width: '240px' } });
  const open = () => { if (input.value) go(`#/${c}/preview/${encodeURIComponent(input.value)}`); };
  input.addEventListener('keydown', (e) => { if (e.key === 'Enter') open(); });
  const host = h('div', { class: 'full', style: { height: 'calc(100vh - 120px)' } });
  main.append(h('div', { class: 'page', style: { maxWidth: 'none' } },
    h('div', { class: 'row' }, h('h2', { class: 'grow' }, 'Room preview'), input, h('button', { class: 'primary', onclick: open }, 'Play'),
      arg ? h('a', { class: 'button', href: `#/${c}/rooms/${encodeURIComponent(arg)}` }, 'Edit room') : null),
    h('p', { class: 'dim small' }, 'Runs the room in the browser, without the game. Solid objects block Kris, triggers and doors show what they run, Z shows the dialogue of an object. ',
      'Object code (cutscenes, enemies, scripts) does not run here.'),
    host));
  if (!arg) return null;
  let player;
  try {
    const room = await capi.read('rooms', arg);
    room.data.GameObjects ||= [];
    player = await createPlayer(host, { capi, room: { name: arg, data: room.data }, onRoomChange: (n) => { input.value = n; } });
  } catch (e) {
    host.append(h('div', { class: 'error-box' }, errMsg(e)));
  }
  return () => player?.dispose();
}
