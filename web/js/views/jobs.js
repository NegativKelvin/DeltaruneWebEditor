// Shows a running job with its log, until it ends.
import { waitJob } from '../api.js';
import { h } from '../ui.js';

/** Appends a job box to `el` and resolves with the final job. */
export async function followJob(el, id, title) {
  const status = h('span', { class: 'badge' }, 'running');
  const log = h('div', { class: 'log' });
  el.append(h('div', { class: 'card' }, h('div', { class: 'row' }, h('h3', null, title), status), log));
  const job = await waitJob(id, (j) => {
    status.textContent = j.status;
    status.className = `badge ${j.status === 'succeeded' ? 'new' : j.status === 'failed' ? 'mod' : ''}`;
    log.textContent = j.log.join('\n') + (j.error ? `\nERROR: ${j.error}` : '');
    log.scrollTop = log.scrollHeight;
  });
  return job;
}
