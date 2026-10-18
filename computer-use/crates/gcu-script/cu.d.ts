// Typed surface for the sandboxed `cu` global. The host installs only `cu` and
// `emit`. There is no filesystem, network, process, or environment API.
// User source is a script body: top-level await and return are allowed.

export interface ElementQuery {
  role?: string;
  name?: string;
}

export interface ElementRef {
  index: number;
  role: string;
  name: string;
}

export interface AppState {
  text: string;
  focused: number | null;
  elements: ElementRef[];
  /** Local scan of `elements`. It does not call the driver. */
  find(query: ElementQuery): ElementRef | null;
}

export interface StateOptions {
  /** Default true. `false` requests text only. */
  screenshot?: boolean;
}

export interface WaitElement {
  role: string;
  name?: string;
}

export interface WaitSpec {
  element?: WaitElement;
  text_present?: string;
  text_absent?: string;
  timeout_ms?: number;
}

export interface AppHandle {
  state(opts?: StateOptions): Promise<AppState>;
  setValue(field: ElementRef, value: string): Promise<AppState>;
  press(key: string): Promise<AppState>;
  click(field: ElementRef): Promise<AppState>;
  typeText(text: string, field?: ElementRef): Promise<AppState>;
  waitFor(spec: WaitSpec): Promise<AppState>;
}

export interface Cu {
  /** Authorize and bind an app. This is a driver call. */
  app(name: string): Promise<AppHandle>;
}

declare global {
  const cu: Cu;
  /** At most five emits. This is not a driver call. */
  function emit(value: unknown): void;
}

export {};
