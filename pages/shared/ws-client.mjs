// SPDX-License-Identifier: AGPL-3.0-only
// Ranchero WebSocket client — handles connection, reconnection, and
// the subscribe/unsubscribe message framing used by /api/ws/events.

export class RancheroClient {
    #url;
    #ws        = null;
    #uid       = 0;
    #callbacks = new Map(); // subId → callback(data)
    #subs      = new Map(); // subId → {event, source, query}
    #timer     = null;

    constructor(url = `ws://${location.host}/api/ws/events`) {
        this.#url = url;
        this.#connect();
    }

    #connect() {
        this.#ws = new WebSocket(this.#url);

        this.#ws.onopen = () => {
            for (const [subId, sub] of this.#subs) {
                this.#sendSubscribe(subId, sub);
            }
        };

        this.#ws.onmessage = ({ data }) => {
            let msg;
            try { msg = JSON.parse(data); } catch { return; }
            if (msg.type === 'event') {
                this.#callbacks.get(msg.uid)?.(msg.data);
            }
        };

        this.#ws.onclose = () => {
            this.#timer = setTimeout(() => this.#connect(), 2000);
        };
    }

    #sendSubscribe(subId, { event, source, query }) {
        if (this.#ws?.readyState !== WebSocket.OPEN) return;
        this.#ws.send(JSON.stringify({
            method: 'subscribe',
            uid:    ++this.#uid,
            arg:    { event, source, subId, query: query ?? {} },
        }));
    }

    subscribe(event, source, callback, query = {}) {
        const subId = ++this.#uid;
        this.#callbacks.set(subId, callback);
        this.#subs.set(subId, { event, source, query });
        this.#sendSubscribe(subId, { event, source, query });
        return subId;
    }

    unsubscribe(subId) {
        this.#callbacks.delete(subId);
        this.#subs.delete(subId);
        if (this.#ws?.readyState === WebSocket.OPEN) {
            this.#ws.send(JSON.stringify({
                method: 'unsubscribe',
                uid:    ++this.#uid,
                arg:    { subId },
            }));
        }
    }

    destroy() {
        clearTimeout(this.#timer);
        this.#ws?.close();
        this.#callbacks.clear();
        this.#subs.clear();
    }
}
