// Shows a running job with its log, until it ends.
import { api, waitJob } from '../api.js';
import { h, attempt } from '../ui.js';

/** Appends a job box to `el` and resolves with the final job. */
export async function followJob(el, id, title) {
  const status = h('span', { class: 'badge' }, 'running');
  const log = h('div', { class: 'log' });
  const cancel = h('button', { class: 'small', onclick: () => { cancel.disabled = true; attempt(() => api.post(`/api/jobs/${id}/cancel`)); } }, 'Cancel');
  el.append(h('div', { class: 'card' }, h('div', { class: 'row' }, h('h3', null, title), status, cancel), log));
  const job = await waitJob(id, (j) => {
    if (j.status !== 'running') cancel.remove();
    status.textContent = j.status;
    status.className = `badge ${j.status === 'succeeded' ? 'new' : j.status === 'failed' ? 'mod' : ''}`;
    log.textContent = j.log.join('\n') + (j.error ? `\nERROR: ${j.error}` : '');
    log.scrollTop = log.scrollHeight;
  });
  return job;
}
