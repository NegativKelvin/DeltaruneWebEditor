// Thin client for the dre-server HTTP API (see README.md).

const enc = encodeURIComponent;

export class ApiError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

async function call(method, path, body, raw) {
  const opts = { method, headers: {} };
  if (raw !== undefined) {
    opts.body = raw;
  } else if (body !== undefined) {
    opts.headers['content-type'] = 'application/json';
    opts.body = JSON.stringify(body);
  }
  const r = await fetch(path, opts);
  if (r.status === 204) return null;
  const text = await r.text();
  let data = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = text; }
  if (!r.ok) throw new ApiError(r.status, (data && data.error) || text || r.statusText);
  return data;
}

export const api = {
  get: (p) => call('GET', p),
  put: (p, body) => call('PUT', p, body),
  post: (p, body) => call('POST', p, body),
  del: (p) => call('DELETE', p),
  putRaw: (p, blob) => call('PUT', p, undefined, blob),
};

/** Builds URL query strings and skips empty values. */
export function qs(params) {
  const s = Object.entries(params)
    .filter(([, v]) => v !== undefined && v !== null && v !== '')
    .map(([k, v]) => `${enc(k)}=${enc(v)}`)
    .join('&');
  return s ? `?${s}` : '';
}

/** Paths for one chapter. */
export function chapterApi(ch) {
  const base = `/api/chapters/${enc(ch)}`;
  const asset = (type, name) => `${base}/assets/${enc(type)}/${enc(name)}`;
  const file = (type, name, f) => `${asset(type, name)}/files/${enc(f)}`;
  return {
    base,
    asset,
    file,
    counts: () => api.get(`${base}/assets`),
    list: (type, params = {}) => api.get(`${base}/assets/${enc(type)}${qs(params)}`),
    read: (type, name) => api.get(asset(type, name)),
    write: (type, name, body) => api.put(asset(type, name), body),
    revert: (type, name) => api.del(asset(type, name)),
    readFile: async (type, name, f) => {
      const r = await fetch(file(type, name, f));
      if (!r.ok) throw new ApiError(r.status, `${f}: ${r.statusText}`);
      return r;
    },
    writeFile: (type, name, f, blob) => api.putRaw(file(type, name, f), blob),
    deleteFile: (type, name, f) => api.del(file(type, name, f)),
    patchCode: (name, rev, edits) => api.post(`${base}/code/${enc(name)}/patch`, { rev, edits }),
    index: (type, fields) => api.get(`${base}/index/${enc(type)}${qs({ fields: fields.join(',') })}`),
    changes: () => api.get(`${base}/changes`),
    extract: () => api.post(`${base}/extract`),
    build: () => api.post(`${base}/build`),
    generic: () => api.get(`${base}/generic`),
    genericList: (list, params = {}) => api.get(`${base}/generic/${enc(list)}${qs(params)}`),
    dialogue: (params = {}) => api.get(`${base}/dialogue${qs(params)}`),
    search: (q, max) => api.get(`${base}/search${qs({ q, max })}`),
    cutscenes: (q) => api.get(`${base}/cutscenes${qs({ q })}`),
    cutscene: (code) => api.get(`${base}/cutscenes/${enc(code)}`),
    enemies: () => api.get(`${base}/enemies`),
  };
}

/** Waits for a job to end. `onUpdate` gets each poll result. */
export async function waitJob(id, onUpdate) {
  for (;;) {
    const j = await api.get(`/api/jobs/${id}`);
    onUpdate?.(j);
    if (j.status !== 'running') return j;
    await new Promise((r) => setTimeout(r, 700));
  }
}
