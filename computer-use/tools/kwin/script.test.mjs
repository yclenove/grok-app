import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {test} from 'node:test';
import vm from 'node:vm';

const code = readFileSync(new URL('./contents/code/main.js', import.meta.url), 'utf8');
function fixture(destination = ':1.42') {
    const reports = [];
    const signals = [];
    const windows = [{internalId: 'uuid', caption: 'Fixture', desktopFileName: 'org.fixture', pid: 123}];
    const context = {
        readConfig: () => destination,
        workspace: {windowList: () => windows, windowAdded: {connect: f => signals.push(f)}, windowRemoved: {connect: f => signals.push(f)}},
        QTimer: function () { this.timeout = {connect: f => signals.push(f)}; this.start = () => {}; },
        callDBus: (...args) => { reports.push(args); args.at(-1)(); },
    };
    return {context, reports, windows, signals};
}

test('script emits bounded JSON reports only to configured unique engine owner', () => {
    const {context, reports, windows, signals} = fixture();
    vm.runInNewContext(code, context, {timeout: 1000});
    assert.equal(reports[0][0], ':1.42');
    assert.equal(reports[0][3], 'Publish');
    const first = JSON.parse(reports[0][4]);
    assert.equal(first[0], 1);
    assert.deepEqual(first[2][0], ['uuid', 1, 'Fixture', 'org.fixture', 123, false]);
    windows[0] = {...windows[0]};
    signals[0]();
    assert.equal(JSON.parse(reports[1][4])[2][0][1], 2);
    assert.equal(reports.length, 2);
});

test('script refuses ambient destinations and stops after missing callback', () => {
    for (const name of ['', 'org.grok.Engine', ':malformed']) {
        const {context} = fixture(name);
        assert.throws(() => vm.runInNewContext(code, context, {timeout: 1000}));
    }
    const {context, reports, signals} = fixture();
    context.callDBus = (...args) => reports.push(args);
    vm.runInNewContext(code, context, {timeout: 1000});
    signals[0]();
    assert.equal(reports.length, 1);
});
