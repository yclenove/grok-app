export const WORKER_COMPLETION_NOT_STARTED = "not_started";
export const WORKER_COMPLETION_UNKNOWN = "unknown";

const COMPLETIONS = new Set([
  WORKER_COMPLETION_NOT_STARTED,
  WORKER_COMPLETION_UNKNOWN,
]);

const UNEXPECTED_ERROR_MESSAGE = "managed browser worker failed unexpectedly";
const TRUSTED_EXCEPTIONS = new WeakMap();

const STATUS_CODES = new Map([
  [400, "invalid_request"],
  [401, "unauthorized"],
  [403, "forbidden"],
  [404, "not_found"],
  [409, "conflict"],
  [413, "payload_too_large"],
  [422, "unprocessable_entity"],
  [429, "rate_limited"],
  [500, "worker_internal"],
  [501, "worker_unavailable"],
  [502, "worker_transport"],
  [503, "worker_unavailable"],
  [504, "worker_timeout"],
]);

export function defaultWorkerCode(status) {
  return STATUS_CODES.get(Number(status)) || "worker_error";
}

function validStatus(status) {
  return Number.isInteger(status) && status >= 400 && status <= 599;
}

function validCode(code) {
  return (
    typeof code === "string" &&
    code.length > 0 &&
    code.length <= 64 &&
    /^[a-z0-9_]+$/.test(code)
  );
}

function boundedMessage(message) {
  const text = String(message || "managed browser worker error").trim();
  return (text || "managed browser worker error").slice(0, 1024);
}

export function createWorkerError({
  status,
  code,
  completion,
  message,
  currentPageGeneration,
}) {
  const numericStatus = Number(status);
  if (!validStatus(numericStatus)) throw new Error("invalid worker error status");
  if (!validCode(code)) throw new Error("invalid worker error code");
  if (!COMPLETIONS.has(completion)) throw new Error("invalid worker completion");
  const error = {
    status: numericStatus,
    code,
    completion,
    message: boundedMessage(message),
  };
  if (currentPageGeneration != null) {
    const generation = Number(currentPageGeneration);
    if (!Number.isSafeInteger(generation) || generation < 1) {
      throw new Error("invalid current page generation");
    }
    error.currentPageGeneration = generation;
  }
  return error;
}

export function workerErrorBody(httpStatus, body = {}) {
  const status = Number(httpStatus);
  try {
    const nested = body && typeof body.error === "object" ? body.error : null;
    const error = nested
      ? createWorkerError({ ...nested, status: nested.status ?? status })
      : createWorkerError({
          status,
          code: body.errorCode || defaultWorkerCode(status),
          completion: body.completion || WORKER_COMPLETION_NOT_STARTED,
          message: body.error || body.message,
          currentPageGeneration:
            body.currentPageGeneration == null
              ? body.pageGeneration
              : body.currentPageGeneration,
        });
    if (error.status !== status) throw new Error("worker status does not match HTTP status");
    return { ok: false, error };
  } catch {
    return {
      ok: false,
      error: createWorkerError({
        status: validStatus(status) ? status : 500,
        code: "invalid_error_envelope",
        completion: WORKER_COMPLETION_UNKNOWN,
        message: "managed browser worker produced an invalid error envelope",
      }),
    };
  }
}

export function workerException(
  status,
  code,
  message,
  completion = WORKER_COMPLETION_NOT_STARTED,
  currentPageGeneration,
) {
  const spec = createWorkerError({
    status,
    code,
    completion,
    message,
    currentPageGeneration,
  });
  const error = new Error(spec.message);
  error.statusCode = spec.status;
  error.workerCode = spec.code;
  error.completion = spec.completion;
  error.currentPageGeneration = spec.currentPageGeneration;
  error.safeRejected = spec.completion === WORKER_COMPLETION_NOT_STARTED;
  TRUSTED_EXCEPTIONS.set(error, Object.freeze({ ...spec }));
  return error;
}

export function workerExceptionSpec(error) {
  const trusted =
    error && (typeof error === "object" || typeof error === "function")
      ? TRUSTED_EXCEPTIONS.get(error)
      : undefined;
  if (trusted) {
    return createWorkerError(trusted);
  }
  return createWorkerError({
    status: 500,
    code: "worker_internal",
    completion: WORKER_COMPLETION_UNKNOWN,
    message: UNEXPECTED_ERROR_MESSAGE,
  });
}
