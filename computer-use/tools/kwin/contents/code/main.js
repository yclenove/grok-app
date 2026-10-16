// KWin 6 public scripting API, read-only. No activation, input, eval or shell calls.
// Explicit user configuration pins the recipient to a live engine unique owner.
const engine = readConfig('engineBusName', '');
if (!/^:\d+\.\d+$/.test(engine)) throw new Error('Configure the trusted engine unique bus owner');
// Reload-lifetime marker only, never a secret or authorization token.
const epoch = 'kwin-script/' + Math.random().toString(36).slice(2) + Math.random().toString(36).slice(2);
let generation = 1;
const lifetimes = new Map();
let pending = false;

function publish() {
    if (pending) return;
    const windows = workspace.windowList();
    if (windows.length > 4096) throw new Error('Window limit');
    const live = new Set(windows);
    for (const window of lifetimes.keys()) {
        if (!live.has(window)) lifetimes.delete(window);
    }
    const records = windows.map(window => {
        if (!lifetimes.has(window)) lifetimes.set(window, generation++);
        // pid is discovery-only: public scripting API cannot prove peer credentials.
        const pid = Number.isInteger(window.pid) && window.pid > 0 ? window.pid : 0;
        return [String(window.internalId), lifetimes.get(window), window.caption || '',
            window.desktopFileName || '', pid, false];
    });
    pending = true;
    callDBus(engine, '/org/grok/ComputerUse/Windows', 'org.grok.ComputerUse.Windows1',
        'Publish', JSON.stringify([1, epoch, records]), () => { pending = false; });
}

// A missing callback stops publishing; the engine expires old reports in two seconds.
const timer = new QTimer();
timer.interval = 500;
timer.timeout.connect(publish);
workspace.windowAdded.connect(publish);
workspace.windowRemoved.connect(publish);
timer.start();
publish();
