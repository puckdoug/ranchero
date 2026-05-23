// SPDX-License-Identifier: AGPL-3.0-only
// Live stats widget — subscribes to athlete/watching/v2.
import { RancheroClient } from '/pages/shared/ws-client.mjs';

const client = new RancheroClient();

function set(id, value) {
    const el = document.querySelector(`#${id} .val`);
    if (el) el.textContent = value ?? '—';
}

function fmt(n, decimals = 0) {
    if (n == null) return '—';
    return Number(n).toFixed(decimals);
}

// Speed from the wire is in mm/h; convert to km/h.
function speedKmh(mmh) {
    if (mmh == null) return null;
    return (mmh / 1_000_000).toFixed(1);
}

client.subscribe(
    'athlete/watching/v2',
    'stats',
    data => {
        const state = data?.state;
        const stats = data?.stats;

        set('power', state ? fmt(state.power) : null);
        set('hr',    state ? fmt(state.heartrate) : null);
        set('speed', state ? speedKmh(state.speed) : null);
        set('np',    stats?.power ? fmt(stats.power.np) : null);
        set('tss',   stats?.power ? fmt(stats.power.tss, 1) : null);
        set('wbal',  data?.wBal != null ? fmt(data.wBal) : null);
    },
    { resources: ['stats', 'state'], stats: true },
);
