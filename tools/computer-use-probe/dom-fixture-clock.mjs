// Unit-fixture helper only. Functional DOM assertions must not depend on how
// much CPU another test process receives. Real native gates retain real clocks;
// deadline tests explicitly bypass this helper and advance their own clock.
export function withObservationClock(window, observe) {
  const performance = window.performance;
  const previous = Object.getOwnPropertyDescriptor(performance, "now");
  const instant = performance.now();
  Object.defineProperty(performance, "now", { configurable: true, value: () => instant });
  try { return observe(); }
  finally {
    if (previous) Object.defineProperty(performance, "now", previous);
    else delete performance.now;
  }
}
