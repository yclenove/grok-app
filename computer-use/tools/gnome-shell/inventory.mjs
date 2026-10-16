// Pure window lifetime/fence logic. No GI, clocks, process inspection or input.
// Ported from v1 policy.js: unknown session properties must block too.
export function screenBlocked(shield, mode) {
    const values = [shield?.actor?.visible, shield?.active, shield?.locked,
        mode?.isLocked, mode?.isGreeter, mode?.hasWindows];
    return values.some(value => typeof value !== 'boolean') ||
        values.slice(0, 5).some(Boolean) || !mode.hasWindows;
}

export class WindowInventory {
    constructor(epoch, windows, describe, activate, blocked) {
        this.epoch = epoch;
        this.windows = windows;
        this.describe = describe;
        this.activateWindow = activate;
        this.blocked = blocked;
        this.generations = new Map();
        this.nextGeneration = 1;
    }

    list() {
        if (this.blocked()) throw new Error('Session blocked');
        const windows = this.windows();
        if (windows.length > 4096) throw new Error('Window limit');
        const live = new Set(windows);
        for (const window of this.generations.keys()) {
            if (!live.has(window)) this.generations.delete(window);
        }
        return windows.map(window => {
            if (!this.generations.has(window)) {
                if (!Number.isSafeInteger(this.nextGeneration)) throw new Error('Generation limit');
                this.generations.set(window, this.nextGeneration++);
            }
            const {id, title, appId, pid, peerPid} = this.describe(window);
            if (typeof id !== 'string' || !id || !Number.isInteger(pid) || pid < 0 || pid > 0xffffffff)
                throw new Error('Malformed window');
            return [id, this.generations.get(window), title, appId, pid, peerPid === true];
        });
    }

    activate(epoch, id, generation, pid, policyAllowed, sender, trustedOwner) {
        if (!policyAllowed || !trustedOwner || sender !== trustedOwner || epoch !== this.epoch)
            return false;
        const records = this.list();
        const matches = records.filter(record => record[0] === id);
        if (matches.length !== 1) return false;
        const record = matches[0];
        if (record[1] !== generation || record[4] !== pid || !record[5]) return false;
        // Exact object from this synchronous inventory, with a second native PID read.
        const targets = this.windows().filter(window => this.describe(window).id === id);
        if (targets.length !== 1 || this.generations.get(targets[0]) !== generation) return false;
        const target = this.describe(targets[0]);
        if (!target.peerPid || target.pid !== pid || this.blocked()) return false;
        this.activateWindow(targets[0]);
        return true;
    }
}
