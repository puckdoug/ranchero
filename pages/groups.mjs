// SPDX-License-Identifier: AGPL-3.0-only
// Groups view — subscribes to groups/v2.
import { RancheroClient } from '/pages/shared/ws-client.mjs';

const client   = new RancheroClient();
const container = document.getElementById('groups');

// athletes: athleteId → {data, groupId}
const athletes = new Map();

function speedKmh(mmh) {
    return mmh != null ? (mmh / 1_000_000).toFixed(1) : '—';
}

function fmt(n) {
    return n != null ? Math.round(n) : '—';
}

function render() {
    // Group athletes by groupId.
    const byGroup = new Map();
    for (const [id, entry] of athletes) {
        const gid = entry.groupId ?? 0;
        if (!byGroup.has(gid)) byGroup.set(gid, []);
        byGroup.get(gid).push({ id, data: entry.data });
    }

    container.innerHTML = '';
    for (const [gid, members] of byGroup) {
        const section = document.createElement('section');
        section.innerHTML = `<h2>Group ${gid || '(none)'}</h2>`;
        const table = document.createElement('table');
        table.innerHTML = `
            <thead><tr><th>ID</th><th>Power</th><th>HR</th><th>Speed</th></tr></thead>
            <tbody>${members.map(({ id, data }) => {
                const s = data?.state;
                return `<tr>
                    <td>${id}</td>
                    <td>${s ? fmt(s.power) : '—'}</td>
                    <td>${s ? fmt(s.heartrate) : '—'}</td>
                    <td>${s ? speedKmh(s.speed) : '—'}</td>
                </tr>`;
            }).join('')}</tbody>`;
        section.appendChild(table);
        container.appendChild(section);
    }
}

client.subscribe('groups/v2', 'stats', data => {
    const id = data?.athleteId;
    if (id == null) return;
    const groupId = data?.state?.groupId;
    athletes.set(id, { data, groupId });
    render();
}, { resources: ['state'] });
