// GML code editor for code assets, with links to dialogue and cutscene views.
import { h, add } from '../ui.js';
import { codeEditor } from './codeedit.js';

export async function open(el, ectx) {
  const { capi, ch, name, asset, setDirty, query, save } = ectx;
  const ed = codeEditor({ value: asset.source ?? '', onChange: () => setDirty(true), onSave: save, height: 'calc(100vh - 190px)' });
  const c = encodeURIComponent(ch);
  const hasSteps = /\bc_\w+\s*\(/.test(asset.source || '');
  const hasText = /\b(msg|stringset|c_msg|scr_84_get_lang_string)/.test(asset.source || '');
  add(el,
    h('div', { class: 'row small', style: { marginBottom: '6px' } },
      h('span', { class: 'dim' }, 'GML. Ctrl+S saves. Tab indents.'),
      hasText ? h('a', { href: `#/${c}/dialogue?code=${encodeURIComponent(name)}` }, 'Dialogue in this code') : null,
      hasSteps ? h('a', { href: `#/${c}/cutscenes/${encodeURIComponent(name)}` }, 'Open as cutscene') : null,
      h('a', { href: `#/${c}/search?q=${encodeURIComponent(name.replace(/^gml_(Script|GlobalScript)_/, ''))}` }, 'Find uses')),
    ed.el);
  if (query.line) setTimeout(() => ed.goto(+query.line), 0);
  return {
    save: async () => {
      const r = await capi.write('code', name, { source: ed.value });
      asset.source = r.source;
      asset.rev = r.rev;
    },
  };
}
