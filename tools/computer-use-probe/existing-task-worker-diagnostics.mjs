// Counters around original MV3 APIs only. No request bodies, bindings or exceptions.
export async function instrumentJointWorker(worker) {
  await worker.evaluate(() => {
    globalThis.__jointCalls = [];
    globalThis.__jointHttp = [];
    globalThis.__jointPairingConfirmed = 0;
    globalThis.__jointDisconnectRequests = 0;
    globalThis.__jointObservationResults = 0;
    const originalFetch = globalThis.fetch.bind(globalThis);
    globalThis.fetch = async (input, init) => {
      const route = new URL(typeof input === "string" ? input : input.url).pathname;
      if (route === "/cu/extension-disconnect") globalThis.__jointDisconnectRequests++;
      const record = { route: /^\/cu\/[a-z-]+$/.test(route) ? route : "other", status: null };
      globalThis.__jointHttp.push(record);
      if (globalThis.__jointHttp.length > 32) globalThis.__jointHttp.shift();
      const response = await originalFetch(input, init);
      if (route === "/cu/pairing-confirm" && response.status === 200) globalThis.__jointPairingConfirmed++;
      if (route === "/cu/extension-result" && response.status === 200) globalThis.__jointObservationResults++;
      record.status = response.status; return response;
    };
    const execute = chrome.scripting.executeScript.bind(chrome.scripting);
    const capture = chrome.tabs.captureVisibleTab.bind(chrome.tabs);
    chrome.scripting.executeScript = async request => {
      globalThis.__jointCalls.push({ function: request.func?.name, tab: request.target.tabId });
      return execute(request);
    };
    globalThis.__jointCaptures = 0;
    globalThis.__jointCapturePermissionDenials = 0;
    globalThis.__jointCaptureSettled = 0;
    globalThis.__jointCaptureDecodes = 0;
    const decode = globalThis.createImageBitmap.bind(globalThis);
    globalThis.createImageBitmap = (...args) => {
      globalThis.__jointCaptureDecodes++;
      return decode(...args);
    };
    chrome.tabs.captureVisibleTab = async (...args) => {
      globalThis.__jointCaptures++;
      try { return await capture(...args); }
      catch (error) {
        if (/activeTab|permission/i.test(error?.message || "")) globalThis.__jointCapturePermissionDenials++;
        throw error;
      }
      finally { globalThis.__jointCaptureSettled++; }
    };
  });
}
