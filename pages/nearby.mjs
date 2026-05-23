// SPDX-License-Identifier: AGPL-3.0-only
// Nearby riders list — subscribes to nearby/v2.
import { RancheroClient } from '/pages/shared/ws-client.mjs';

const client  = new RancheroClient();
const tbody   = document.querySelector('#nearby tbody');
const athletes = new Map(); // athleteId → row element

function speedKmh(mmh) {
    return mmh != null ? (mmh / 1_000_000).toFixed(1) : '—';
}

function distKm(m) {
    return m != null ? (m / 1000).toFixed(2) : '—';
}

function fmt(n) {
    return n != null ? Math.round(n) : '—';
}

function upsert(data) {
    const id = data?.athleteId;
    if (id == null) return;
    const state = data?.state;

    let row = athletes.get(id);
    if (!row) {
        row = document.createElement('tr');
        row.innerHTML = `<td>${id}</td><td></td><td></td><td></td><td></td>`;
        tbody.appendChild(row);
        athletes.set(id, row);
    }

    const cells = row.querySelectorAll('td');
    cells[1].textContent = state ? fmt(state.power)          : '—';
    cells[2].textContent = state ? fmt(state.heartrate)      : '—';
    cells[3].textContent = state ? speedKmh(state.speed)     : '—';
    cells[4].textContent = state ? distKm(state.distance)    : '—';
}

client.subscribe('nearby/v2', 'stats', upsert, { resources: ['state'] });
