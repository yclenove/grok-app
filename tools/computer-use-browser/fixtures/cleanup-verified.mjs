// Test harness only. Never erase an isolated profile after an unconfirmed
// native shutdown, or let a later cleanup failure replace the original one.
export async function finishOwnedCleanup({ shutdown, workerExit, fixtureClose, browserExit, removeProfile }) {
  const failures = [];
  for (const check of [shutdown, workerExit, fixtureClose, browserExit]) {
    try { await check(); }
    catch (error) { failures.push(error); }
  }
  if (failures.length) {
    throw new AggregateError(failures, "owned cleanup unconfirmed; isolated profile retained");
  }
  await removeProfile();
}
