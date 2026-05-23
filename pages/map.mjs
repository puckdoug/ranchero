// SPDX-License-Identifier: AGPL-3.0-only
// Position trail widget — subscribes to athlete/watching/v2 with the state
// resource and plots the rider's game-world x/z coordinates (exposed as
// state.lat / state.lng by the server; see docs/planning/STEP-19 gap #1 for
// the planned latlng array migration).
import { RancheroClient } from '/pages/shared/ws-client.mjs';

const client  = new RancheroClient();
const canvas  = document.getElementById('map');
const ctx     = canvas.getContext('2d');
const coords  = document.getElementById('coords');
const TRAIL   = 500; // maximum number of trail points kept

const trail = [];
let bounds  = null; // { minX, maxX, minY, maxY }

function worldToCanvas(x, y) {
    const { minX, maxX, minY, maxY } = bounds;
    const rangeX = maxX - minX || 1;
    const rangeY = maxY - minY || 1;
    const margin = 20;
    const w = canvas.width  - margin * 2;
    const h = canvas.height - margin * 2;
    return [
        margin + (x - minX) / rangeX * w,
        margin + (1 - (y - minY) / rangeY) * h,
    ];
}

function updateBounds(x, y) {
    if (!bounds) {
        bounds = { minX: x, maxX: x, minY: y, maxY: y };
    } else {
        if (x < bounds.minX) bounds.minX = x;
        if (x > bounds.maxX) bounds.maxX = x;
        if (y < bounds.minY) bounds.minY = y;
        if (y > bounds.maxY) bounds.maxY = y;
    }
}

function draw() {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (trail.length < 2) return;

    // Trail (older points fade out).
    for (let i = 1; i < trail.length; i++) {
        const alpha = i / trail.length;
        ctx.beginPath();
        ctx.strokeStyle = `rgba(0, 200, 255, ${alpha})`;
        ctx.lineWidth   = 2;
        const [x0, y0] = worldToCanvas(trail[i - 1][0], trail[i - 1][1]);
        const [x1, y1] = worldToCanvas(trail[i][0],     trail[i][1]);
        ctx.moveTo(x0, y0);
        ctx.lineTo(x1, y1);
        ctx.stroke();
    }

    // Current position dot.
    const [cx, cy] = worldToCanvas(trail.at(-1)[0], trail.at(-1)[1]);
    ctx.beginPath();
    ctx.arc(cx, cy, 5, 0, Math.PI * 2);
    ctx.fillStyle = '#00c8ff';
    ctx.fill();
}

client.subscribe(
    'athlete/watching/v2',
    'stats',
    data => {
        const state = data?.state;
        // state.lat and state.lng are the game-world x/z coordinates.
        if (state?.lat == null || state?.lng == null) return;
        const x = state.lat;
        const y = state.lng;

        coords.textContent = `x: ${x.toFixed(2)}  z: ${y.toFixed(2)}`;
        updateBounds(x, y);
        trail.push([x, y]);
        if (trail.length > TRAIL) trail.shift();
        draw();
    },
    { resources: ['state'] },
);
