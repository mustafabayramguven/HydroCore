'use strict';
// HydroCore dashboard. No external requests and no inline scripts: everything is served
// from the server binary. All dynamic text is inserted via textContent (no innerHTML).

const SVGNS = 'http://www.w3.org/2000/svg';
const $ = (sel, el = document) => el.querySelector(sel);

function h(tag, attrs, ...kids) { return build(document.createElement(tag), attrs, kids); }
function s(tag, attrs, ...kids) { return build(document.createElementNS(SVGNS, tag), attrs, kids); }
function build(el, attrs, kids) {
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const kid of kids.flat()) {
    if (kid === null || kid === undefined || kid === false) continue;
    el.append(kid.nodeType ? kid : document.createTextNode(String(kid)));
  }
  return el;
}

// ---- dates (the user's calendar day is the *browser's* local day) -----------------
const pad = (n) => String(n).padStart(2, '0');
const iso = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const parseISO = (t) => { const [y, m, d] = t.split('-').map(Number); return new Date(y, m - 1, d); };
const addDays = (d, n) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
const mondayOf = (d) => addDays(d, -((d.getDay() + 6) % 7));
const fmtDate = (t, opts) => parseISO(t).toLocaleDateString(undefined, opts || { weekday: 'short', month: 'short', day: 'numeric' });
const fmtTime = (utc) => new Date(utc).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
const fmtDateTime = (utc) => new Date(utc).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
const ml = (n) => (n === null || n === undefined ? '–' : `${Math.round(n).toLocaleString()} ml`);
const pct = (i, g) => (g > 0 ? Math.round((i * 100) / g) : 0);
const todayISO = () => iso(new Date());

// ---- state & api ---------------------------------------------------------------
const state = { user: null, tab: 'today', weekStart: iso(mondayOf(new Date())), range: 30, shown: 40 };

async function api(path, params) {
  const q = new URLSearchParams(params || {});
  const res = await fetch(`${path}${q.toString() ? `?${q}` : ''}`, { headers: { Accept: 'application/json' } });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

// ---- components ---------------------------------------------------------------
function stat(value, label) {
  return h('div', { class: 'stat' }, h('div', { class: 'v' }, value), h('div', { class: 'l' }, label));
}

function ring(intake, goal) {
  const R = 78, C = 2 * Math.PI * R, p = Math.min(1, goal > 0 ? intake / goal : 0), met = intake >= goal;
  return s('svg', { class: 'ring', viewBox: '0 0 200 200', width: 200, height: 200, role: 'img', 'aria-label': `${pct(intake, goal)}% of daily goal` },
    s('circle', { class: 'track', cx: 100, cy: 100, r: R, fill: 'none', 'stroke-width': 16 }),
    s('circle', { class: `fill${met ? ' met' : ''}`, cx: 100, cy: 100, r: R, fill: 'none', 'stroke-width': 16, 'stroke-linecap': 'round',
      'stroke-dasharray': C, 'stroke-dashoffset': C * (1 - p), transform: 'rotate(-90 100 100)' }),
    s('text', { x: 100, y: 104, 'text-anchor': 'middle', 'font-size': 38, 'font-weight': 700 }, `${pct(intake, goal)}%`),
    s('text', { x: 100, y: 130, 'text-anchor': 'middle', 'font-size': 13, class: 'axis' }, 'of daily goal'));
}

// items: [{date, label, intake, goal, has}]; draws bars, goal ticks and a today marker.
function barChart(items, { height = 260 } = {}) {
  const W = 720, H = height, L = 46, R = 8, T = 10, B = 26;
  const maxV = Math.max(1, ...items.map((i) => Math.max(i.intake || 0, i.goal || 0))) * 1.08;
  const step = niceStep(maxV / 4);
  const top = Math.ceil(maxV / step) * step;
  const y = (v) => T + (H - T - B) * (1 - v / top);
  const slot = (W - L - R) / items.length, bw = Math.max(1.5, Math.min(48, slot * 0.7));
  const svg = s('svg', { class: 'chart', viewBox: `0 0 ${W} ${H}`, role: 'img', 'aria-label': 'Water intake chart' });
  for (let v = 0; v <= top + 0.1; v += step) {
    svg.append(s('line', { class: 'grid', x1: L, x2: W - R, y1: y(v), y2: y(v) }),
      s('text', { class: 'axis', x: L - 6, y: y(v) + 4, 'text-anchor': 'end' }, v >= 1000 ? `${v / 1000}L` : v));
  }
  const today = todayISO();
  items.forEach((it, idx) => {
    const cx = L + slot * idx + slot / 2, x = cx - bw / 2;
    if (it.has) {
      const bh = Math.max(1, y(0) - y(it.intake));
      svg.append(s('rect', { class: `bar${it.intake >= it.goal ? ' met' : ''}`, x, y: y(0) - bh, width: bw, height: bh, rx: Math.min(4, bw / 3) },
        s('title', null, `${fmtDate(it.date)}: ${ml(it.intake)} / ${ml(it.goal)} (${pct(it.intake, it.goal)}%)`)));
      svg.append(s('line', { class: 'goal', x1: x - 2, x2: x + bw + 2, y1: y(it.goal), y2: y(it.goal) }));
    } else {
      svg.append(s('rect', { class: 'nodata', x, y: y(0) - 3, width: bw, height: 3 }, s('title', null, `${fmtDate(it.date)}: no data`)));
    }
    if (it.date === today) svg.append(s('line', { class: 'today-mark', x1: x, x2: x + bw, y1: y(0) + 2, y2: y(0) + 2 }));
    if (it.label) svg.append(s('text', { class: 'axis', x: cx, y: H - 8, 'text-anchor': 'middle' }, it.label));
  });
  return h('div', null, svg, h('div', { class: 'legend' },
    h('span', null, h('i', { style: 'background:var(--bar)' }), 'below goal'),
    h('span', null, h('i', { style: 'background:var(--met)' }), 'goal reached'),
    h('span', null, h('i', { style: 'background:var(--warn)' }), 'daily goal'),
    h('span', null, h('i', { style: 'background:var(--nodata)' }), 'no data')));
}
function niceStep(x) {
  const p = 10 ** Math.floor(Math.log10(x)), f = x / p;
  return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 2.5 ? 2.5 : f <= 5 ? 5 : 10) * p;
}

function entriesList(entries) {
  if (!entries.length) return h('p', { class: 'muted' }, 'No drinks recorded.');
  return h('ul', { class: 'entries' }, entries.map((e) =>
    h('li', null, h('span', { title: e.timestamp_utc }, fmtTime(e.timestamp_utc)), h('strong', null, `+${ml(e.amount_ml)}`))));
}

// Every calendar day in [from, to] (inclusive), merged with the stored ones.
function fillRange(from, to, stored, labeler) {
  const byDate = new Map(stored.map((d) => [d.date, d]));
  const out = [];
  for (let d = parseISO(from); iso(d) <= to; d = addDays(d, 1)) {
    const k = iso(d), row = byDate.get(k);
    out.push({ date: k, label: labeler(d, out.length), has: !!row, intake: row?.intake_ml || 0, goal: row?.daily_goal_ml || 0, row });
  }
  return out;
}

// ---- views --------------------------------------------------------------------
async function viewToday(root) {
  const today = todayISO();
  const [sum, day] = await Promise.all([
    api('/api/summary', { user: state.user, today }),
    api(`/api/days/${today}`, { user: state.user }),
  ]);
  const S = sum?.summary;
  if (!day) {
    root.append(h('div', { class: 'card empty' }, h('p', { class: 'big' }, 'No data for today yet'),
      h('p', null, 'Log a drink in the HydroCore app and tap Manual Sync.'),
      S && S.recorded_days ? h('p', null, `Showing history below: ${S.recorded_days} day(s) recorded.`) : null));
  } else {
    const left = Math.max(0, day.daily_goal_ml - day.intake_ml);
    root.append(h('div', { class: 'card' },
      h('div', { class: 'hero' }, ring(day.intake_ml, day.daily_goal_ml),
        h('div', null,
          h('div', { class: 'big' }, ml(day.intake_ml)),
          h('div', { class: 'muted' }, `of ${ml(day.daily_goal_ml)} · ${fmtDate(today, { weekday: 'long', month: 'long', day: 'numeric' })}`),
          h('p', null, day.goal_reached ? h('span', { class: 'pill met' }, 'Goal reached 🎉') : h('span', { class: 'pill' }, `${ml(left)} to go`)),
          h('div', { class: 'muted' }, `Last sync ${fmtDateTime(day.last_sync_utc)}`)))));
    root.append(h('div', { class: 'card' }, h('h2', null, `Today’s drinks (${day.entries.length})`), entriesList(day.entries)));
  }
  if (S) root.append(summaryStats(S));
}

function summaryStats(S) {
  return h('div', { class: 'stats' },
    stat(`${S.current_streak_days} day${S.current_streak_days === 1 ? '' : 's'}`, 'current goal streak'),
    stat(`${S.longest_streak_days} day${S.longest_streak_days === 1 ? '' : 's'}`, 'longest streak'),
    stat(ml(S.avg_last7_ml), '7-day average (finished days)'),
    stat(ml(S.avg_last30_ml), '30-day average (finished days)'),
    stat(ml(S.avg_all_time_ml), 'all-time average'),
    stat(`${S.goal_met_days} / ${S.recorded_days}`, 'days goal met'),
    stat(S.best_day ? ml(S.best_day.intake_ml) : '–', S.best_day ? `best day · ${fmtDate(S.best_day.date, { month: 'short', day: 'numeric', year: 'numeric' })}` : 'best day'),
    stat(`${(S.total_volume_ml / 1000).toLocaleString(undefined, { maximumFractionDigits: 1 })} L`, 'total logged'));
}

async function viewWeek(root) {
  const start = parseISO(state.weekStart), end = addDays(start, 6);
  const thisMonday = iso(mondayOf(new Date()));
  const res = await api('/api/days', { user: state.user, from: iso(start), to: iso(end) });
  const items = fillRange(iso(start), iso(end), res?.days || [], (d) => d.toLocaleDateString(undefined, { weekday: 'short' }));
  const have = items.filter((i) => i.has);
  const total = have.reduce((a, i) => a + i.intake, 0);
  const nav = h('div', { class: 'row-nav' },
    h('button', { onclick: () => { state.weekStart = iso(addDays(start, -7)); render(); } }, '← Previous'),
    h('strong', null, `${fmtDate(iso(start), { month: 'short', day: 'numeric' })} – ${fmtDate(iso(end), { month: 'short', day: 'numeric', year: 'numeric' })}`),
    h('button', { disabled: state.weekStart >= thisMonday, onclick: () => { state.weekStart = iso(addDays(start, 7)); render(); } }, 'Next →'));
  root.append(h('div', { class: 'card' }, nav, barChart(items)));
  root.append(h('div', { class: 'stats' },
    stat(ml(total), 'total this week'),
    stat(have.length ? ml(total / have.length) : '–', 'average per day with data'),
    stat(`${have.filter((i) => i.intake >= i.goal).length} / ${have.length}`, 'days goal met')));
  root.append(dayTable(have.map((i) => i.row).reverse()));
}

async function viewHistory(root) {
  const ranges = [[30, '30 days'], [90, '90 days'], [365, '1 year'], [0, 'All']];
  root.append(h('div', { class: 'chips' }, ranges.map(([n, label]) =>
    h('button', { 'aria-pressed': String(state.range === n), onclick: () => { state.range = n; state.shown = 40; render(); } }, label))));
  const today = todayISO();
  const [sum, res] = await Promise.all([
    api('/api/summary', { user: state.user, today }),
    api('/api/days', { user: state.user, ...(state.range ? { from: iso(addDays(new Date(), -(state.range - 1))) } : {}) }),
  ]);
  const days = res?.days || [];
  if (!days.length) { root.append(h('div', { class: 'card empty' }, 'No synced days in this range.')); return; }
  const from = state.range ? iso(addDays(new Date(), -(state.range - 1))) : days[0].date;
  const items = fillRange(from, today > days[days.length - 1].date ? today : days[days.length - 1].date, days, (d, idx) => {
    const n = state.range || 90;
    if (n <= 31) return d.getDate() % 5 === 1 || idx === 0 ? `${d.getMonth() + 1}/${d.getDate()}` : '';
    return d.getDate() === 1 ? d.toLocaleDateString(undefined, { month: 'short' }) : '';
  });
  root.append(h('div', { class: 'card' }, h('h2', null, 'Daily intake'), barChart(items, { height: 280 })));
  if (sum?.summary) root.append(summaryStats(sum.summary));
  root.append(dayTable(days.slice().reverse()));
}

function dayTable(rows) {
  if (!rows.length) return h('div', { class: 'card empty' }, 'No data.');
  const body = h('tbody');
  const shown = rows.slice(0, state.shown);
  for (const d of shown) {
    const tr = h('tr', { class: 'day', tabindex: 0 },
      h('td', null, fmtDate(d.date, { weekday: 'short', year: 'numeric', month: 'short', day: 'numeric' })),
      h('td', { class: 'num' }, ml(d.intake_ml)),
      h('td', { class: 'num' }, ml(d.daily_goal_ml)),
      h('td', { class: 'num' }, `${pct(d.intake_ml, d.daily_goal_ml)}%`),
      h('td', null, d.goal_reached ? h('span', { class: 'pill met' }, 'met') : h('span', { class: 'pill' }, 'below')),
      h('td', { class: 'num' }, d.entry_count));
    let open = null;
    const toggle = async () => {
      if (open) { open.remove(); open = null; return; }
      open = h('tr', { class: 'detail' }, h('td', { colspan: 6 }, 'Loading…'));
      tr.after(open);
      try {
        const full = await api(`/api/days/${d.date}`, { user: state.user });
        const cell = open.firstChild; cell.textContent = '';
        cell.append(full ? entriesList(full.entries) : 'Not found.');
      } catch (e) { open.firstChild.textContent = `Error: ${e.message}`; }
    };
    tr.addEventListener('click', toggle);
    tr.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggle(); } });
    body.append(tr);
  }
  const card = h('div', { class: 'card' }, h('h2', null, 'Days (tap a row for its drinks)'),
    h('div', { style: 'overflow-x:auto' }, h('table', null,
      h('thead', null, h('tr', null, h('th', null, 'Date'), h('th', { class: 'num' }, 'Intake'), h('th', { class: 'num' }, 'Goal'),
        h('th', { class: 'num' }, '%'), h('th', null, 'Status'), h('th', { class: 'num' }, 'Drinks'))), body)));
  if (rows.length > shown.length) {
    card.append(h('p', null, h('button', { class: 'btn', onclick: () => { state.shown += 60; render(); } }, `Show more (${rows.length - shown.length} left)`)));
  }
  return card;
}

// ---- shell --------------------------------------------------------------------
let renderToken = 0;
async function render() {
  const token = ++renderToken, root = $('#view');
  for (const b of document.querySelectorAll('.tabs button')) b.setAttribute('aria-selected', String(b.dataset.tab === state.tab));
  const csv = $('#export'); csv.href = `/api/export.csv${state.user ? `?user=${encodeURIComponent(state.user)}` : ''}`;
  const out = h('div', { style: 'display:contents' });
  try {
    if (!state.user) {
      out.append(h('div', { class: 'card empty' }, h('p', { class: 'big' }, 'Waiting for the first sync'),
        h('p', null, 'In the HydroCore app set Server URL to '), h('p', null, h('code', null, `${location.origin}/api/sync`)),
        h('p', null, 'and tap Manual Sync.')));
    } else if (state.tab === 'today') await viewToday(out);
    else if (state.tab === 'week') await viewWeek(out);
    else await viewHistory(out);
  } catch (e) {
    out.replaceChildren(h('div', { class: 'card error' }, `Could not load data: ${e.message}`));
  }
  if (token === renderToken) root.replaceChildren(out);
}

async function loadUsers() {
  const res = await api('/api/users');
  const users = res?.users || [];
  const sel = $('#user');
  sel.replaceChildren(...users.map((u) => h('option', { value: u.user_id }, u.user_id)));
  if (!users.some((u) => u.user_id === state.user)) state.user = users[0]?.user_id || null;
  if (state.user) sel.value = state.user;
  sel.parentElement.hidden = users.length < 2;
}

document.querySelectorAll('.tabs button').forEach((b) => b.addEventListener('click', () => { state.tab = b.dataset.tab; location.hash = state.tab; render(); }));
$('#user').addEventListener('change', (e) => { state.user = e.target.value; render(); });
const fromHash = location.hash.slice(1);
if (['today', 'week', 'history'].includes(fromHash)) state.tab = fromHash;
loadUsers().then(render).catch((e) => { $('#view').textContent = `Could not load data: ${e.message}`; });
