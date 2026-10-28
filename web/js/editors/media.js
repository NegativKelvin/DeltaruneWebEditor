// Backgrounds (tilesets), sounds, fonts: properties form plus a preview of the main file.
import { h, attempt, pickFiles, forgetImages } from '../ui.js';
import { jsonForm } from './jsonform.js';

export async function open(el, ectx) {
  const { capi, type, name, asset, setDirty } = ectx;
  const data = structuredClone(asset.data);
  const preview = h('div', { class: 'card' });
  const main = asset.files.find((f) => /\.(png|ogg|wav|mp3)$/i.test(f) && !f.includes('.mask.'));
  const draw = () => {
    preview.replaceChildren(h('h3', null, 'Preview'));
    if (!main) { preview.append(h('p', { class: 'dim' }, 'No file. Add one in the Files tab.')); return; }
    const url = `${capi.file(type, name, main)}?t=${Date.now()}`;
    if (/\.png$/i.test(main)) {
      const img = h('img', { class: 'thumb', src: url, style: { maxHeight: '50vh' } });
      preview.append(img);
      if (type === 'backgrounds' && data.Tileset) {
        img.addEventListener('load', () => preview.append(h('p', { class: 'dim small' },
          `${img.naturalWidth}×${img.naturalHeight} px, tiles ${data.Tileset.TileWidth}×${data.Tileset.TileHeight}, ${data.Tileset.TileCount} tiles in ${data.Tileset.TileColumns} columns.`)));
      }
    } else {
      preview.append(h('audio', { controls: true, src: url }));
    }
    preview.append(h('div', { class: 'row', style: { marginTop: '6px' } },
      h('button', { onclick: async () => {
        const [f] = await pickFiles(/\.png$/i.test(main) ? 'image/png' : 'audio/*');
        if (f && await attempt(() => capi.writeFile(type, name, main, f), `Replaced ${main}`)) { forgetImages(capi.asset(type, name)); draw(); }
      } }, `Replace ${main}`),
      h('span', { class: 'dim small' }, type === 'fonts' ? 'Keep the glyph layout of the font sheet, or change the glyph list to match.' : type === 'sounds' ? 'Use the same format (ogg or wav) as the original.' : '')));
  };
  draw();
  el.append(preview, h('div', { class: 'card' }, h('h3', null, 'Properties'), jsonForm(data, () => setDirty(true), ectx)));
  return { save: () => capi.write(type, name, { data }).then((r) => { asset.data = r.data; }) };
}
