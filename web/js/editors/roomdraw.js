// Room renderer shared by the room editor and the room preview: background colour, backgrounds,
// tile layers, asset layers, legacy tiles, and instances, in depth order. GMS2 (Layers) and older rooms.
import { loadImage, gmToCss } from '../ui.js';

export const TILE_INDEX = 0x7ffff, TILE_MIRROR = 1 << 28, TILE_FLIP = 1 << 29, TILE_ROTATE = 1 << 30;

export function layerKind(l) {
  return (l.$type || '').replace('Layer', '') || (l.InstanceIndices ? 'Instances' : l.TileData ? 'Tiles' : 'Assets');
}

/**
 * `data`: room JSON (can change between calls). `idx`: {objIdx, sprIdx, bgIdx} from the index endpoint.
 * `onImage`: called when an image finishes loading (paint again).
 */
export function roomRenderer(capi, data, idx, onImage) {
  const { objIdx, sprIdx, bgIdx } = idx;
  const layerCache = new Map();
  const images = new Map();
  const gms2 = () => Array.isArray(data.Layers) && data.Layers.length > 0;

  const objSprite = (obj) => objIdx[obj]?.Sprite || null;
  const objVisible = (obj) => objIdx[obj]?.Visible !== false;
  const sprFrame = (spr, i = 0) => {
    const s = sprIdx[spr];
    if (!s) return null;
    const n = Math.max(1, s.TextureCount || 1);
    return capi.file('sprites', spr, `${spr}_${((Math.floor(i) % n) + n) % n}.png`);
  };
  const img = (url) => {
    if (!url) return null;
    const got = images.get(url);
    if (got !== undefined) return got;
    images.set(url, null);
    loadImage(url).then((im) => { images.set(url, im || false); onImage?.(); });
    return null;
  };

  /** Sprite box of an instance: [x, y, w, h]. `spr` overrides the object sprite. */
  function instBox(inst, spr = objSprite(inst.ObjectName)) {
    const s = sprIdx[spr];
    const w = s ? s.Width : 16, hh = s ? s.Height : 16, ox = s ? s.OriginX : 0, oy = s ? s.OriginY : 0;
    const sx = inst.ScaleX ?? 1, sy = inst.ScaleY ?? 1;
    const x1 = inst.X - ox * sx, y1 = inst.Y - oy * sy;
    const x2 = x1 + w * sx, y2 = y1 + hh * sy;
    return [Math.min(x1, x2), Math.min(y1, y2), Math.abs(x2 - x1), Math.abs(y2 - y1)];
  }

  function drawSprite(g, spr, frame, x, y, sx = 1, sy = 1, rot = 0, color = 0xffffffff, alpha = 1) {
    const s = sprIdx[spr];
    const im = img(sprFrame(spr, frame));
    if (!s || !im) return false;
    g.save();
    g.translate(x, y);
    if (rot) g.rotate((-rot * Math.PI) / 180);
    g.scale(sx, sy);
    g.globalAlpha = alpha * ((((color >>> 24) & 255) / 255) || 1);
    g.drawImage(im, -s.OriginX, -s.OriginY);
    g.restore();
    return true;
  }

  function drawTiled(g, im, x, y, w, hh, tileH, tileV, stretch) {
    if (stretch) { g.drawImage(im, 0, 0, data.Width, data.Height); return; }
    const x0 = tileH ? ((x % w) + w) % w - w : x, y0 = tileV ? ((y % hh) + hh) % hh - hh : y;
    for (let yy = y0; yy < (tileV ? data.Height : y0 + 1); yy += hh) for (let xx = x0; xx < (tileH ? data.Width : x0 + 1); xx += w) g.drawImage(im, xx, yy);
  }

  function tileLayerCanvas(li) {
    const l = data.Layers[li];
    if (layerCache.has(li)) return layerCache.get(li);
    const ts = bgIdx[l.Background]?.Tileset;
    const im = img(capi.file('backgrounds', l.Background, `${l.Background}.png`));
    if (!ts || !im) return null;
    const c = document.createElement('canvas');
    c.width = l.TilesX * ts.TileWidth;
    c.height = l.TilesY * ts.TileHeight;
    const cg = c.getContext('2d');
    const tw = ts.TileWidth, th = ts.TileHeight, bx = ts.OutputBorderX, by = ts.OutputBorderY, cols = ts.TileColumns || 1;
    for (let ty = 0; ty < l.TilesY; ty++) {
      for (let tx = 0; tx < l.TilesX; tx++) {
        const v = l.TileData[ty * l.TilesX + tx] >>> 0;
        const id = v & TILE_INDEX;
        if (!id) continue;
        const sx = (id % cols) * (tw + 2 * bx) + bx, sy = Math.floor(id / cols) * (th + 2 * by) + by;
        if (v & (TILE_MIRROR | TILE_FLIP | TILE_ROTATE)) {
          cg.save();
          cg.translate(tx * tw + tw / 2, ty * th + th / 2);
          if (v & TILE_ROTATE) cg.rotate(Math.PI / 2);
          cg.scale(v & TILE_MIRROR ? -1 : 1, v & TILE_FLIP ? -1 : 1);
          cg.drawImage(im, sx, sy, tw, th, -tw / 2, -th / 2, tw, th);
          cg.restore();
        } else {
          cg.drawImage(im, sx, sy, tw, th, tx * tw, ty * th, tw, th);
        }
      }
    }
    layerCache.set(li, c);
    return c;
  }

  function drawLegacyTile(g, t) {
    const im = img(capi.file('backgrounds', t.Asset, `${t.Asset}.png`)) || img(sprFrame(t.Asset, 0));
    if (!im) return;
    g.save();
    g.globalAlpha = (((t.Color >>> 24) & 255) / 255) || 1;
    g.drawImage(im, t.SourceX, t.SourceY, t.Width, t.Height, t.X, t.Y, t.Width * (t.ScaleX ?? 1), t.Height * (t.ScaleY ?? 1));
    g.restore();
  }

  /**
   * Draws the room in room coordinates (the caller sets the transform).
   * `hidden`: set of layer indexes (or 'bg', 'tiles', 'inst', 'fg' in old rooms) to skip.
   * `drawInstances(indices)`: draws the instances of one instance layer (old rooms: all, sorted by object depth).
   */
  function drawScene(g, hidden, drawInstances) {
    g.save();
    g.beginPath();
    g.rect(0, 0, data.Width, data.Height);
    g.clip();
    g.fillStyle = data.DrawBackgroundColor !== false ? gmToCss(data.BackgroundColor ?? 0, false) : '#000';
    g.fillRect(0, 0, data.Width, data.Height);
    if (gms2()) {
      const order = data.Layers.map((l, i) => i).sort((a, b) => data.Layers[b].Depth - data.Layers[a].Depth);
      for (const li of order) {
        if (hidden.has(li)) continue;
        const l = data.Layers[li];
        const kind = layerKind(l);
        g.save();
        g.translate(l.XOffset || 0, l.YOffset || 0);
        if (kind === 'Background') {
          if (l.Sprite) {
            const im = img(sprFrame(l.Sprite, l.FirstFrame || 0));
            if (im && l.BackgroundVisible !== false) drawTiled(g, im, 0, 0, im.width, im.height, l.TiledHorizontally, l.TiledVertically, l.Stretch);
          } else if (l.BackgroundVisible !== false) {
            g.fillStyle = gmToCss(l.Color ?? 0xff000000);
            g.fillRect(-(l.XOffset || 0), -(l.YOffset || 0), data.Width, data.Height);
          }
        } else if (kind === 'Tiles') {
          const c = tileLayerCanvas(li);
          if (c) g.drawImage(c, 0, 0);
        } else if (kind === 'Assets') {
          for (const t of l.LegacyTiles || []) drawLegacyTile(g, t);
          for (const s of l.Sprites || []) drawSprite(g, s.Sprite, s.FrameIndex || 0, s.X, s.Y, s.ScaleX, s.ScaleY, s.Rotation, s.Color);
        } else if (kind === 'Instances') {
          g.translate(-(l.XOffset || 0), -(l.YOffset || 0));
          drawInstances((l.InstanceIndices || []).filter((i) => data.GameObjects[i]), li);
        }
        g.restore();
      }
    } else {
      const bgs = data.Backgrounds || [];
      const drawBg = (fg) => {
        for (const b of bgs) {
          if (!b.Enabled || !b.BackgroundAsset || !!b.Foreground !== fg) continue;
          const im = img(capi.file('backgrounds', b.BackgroundAsset, `${b.BackgroundAsset}.png`));
          if (im) drawTiled(g, im, b.X, b.Y, im.width, im.height, b.TiledHorizontally, b.TiledVertically, b.Stretch);
        }
      };
      if (!hidden.has('bg')) drawBg(false);
      const tiles = [...(data.Tiles || [])].sort((a, b) => b.Depth - a.Depth);
      const instOrder = data.GameObjects.map((_, i) => i).sort((a, b) => (objIdx[data.GameObjects[b].ObjectName]?.Depth || 0) - (objIdx[data.GameObjects[a].ObjectName]?.Depth || 0));
      if (!hidden.has('tiles')) for (const t of tiles) drawLegacyTile(g, t);
      if (!hidden.has('inst')) drawInstances(instOrder, -1);
      if (!hidden.has('fg')) drawBg(true);
    }
    g.restore();
  }

  return { gms2, objSprite, objVisible, sprFrame, img, instBox, drawSprite, drawScene, layerCache };
}
