import test from "node:test";
import assert from "node:assert/strict";

import {
  WORKER_COMPLETION_NOT_STARTED,
  WORKER_COMPLETION_UNKNOWN,
  createWorkerError,
  defaultWorkerCode,
  workerErrorBody,
  workerException,
  workerExceptionSpec,
} from "./worker-errors.mjs";

const UNEXPECTED_ERROR_MESSAGE = "managed browser worker failed unexpectedly";

test("worker error envelope preserves status code completion and generation", () => {
  const body = workerErrorBody(422, {
    error: {
      status: 422,
      code: "invalid_action",
      completion: "not_started",
      message: "schema rejected",
      currentPageGeneration: 9,
      futureField: true,
    },
  });
  assert.deepEqual(body, {
    ok: false,
    error: {
      status: 422,
      code: "invalid_action",
      completion: WORKER_COMPLETION_NOT_STARTED,
      message: "schema rejected",
      currentPageGeneration: 9,
    },
  });
});

test("stable default codes do not depend on English messages", () => {
  const first = workerErrorBody(409, { error: "first wording" });
  const second = workerErrorBody(409, { error: "completely different wording" });
  assert.equal(first.error.code, "conflict");
  assert.equal(second.error.code, "conflict");
  assert.equal(defaultWorkerCode(500), "worker_internal");
});

test("malformed envelopes fail closed as unknown", () => {
  const body = workerErrorBody(409, {
    error: {
      status: 400,
      code: "BAD-CODE",
      completion: "maybe",
      message: "bad",
    },
  });
  assert.equal(body.error.status, 409);
  assert.equal(body.error.code, "invalid_error_envelope");
  assert.equal(body.error.completion, WORKER_COMPLETION_UNKNOWN);
});

test("unexpected exceptions and HTTP 500 are unknown", () => {
  const unexpected = workerExceptionSpec(new Error("boom"));
  assert.equal(unexpected.status, 500);
  assert.equal(unexpected.code, "worker_internal");
  assert.equal(unexpected.completion, WORKER_COMPLETION_UNKNOWN);
  assert.equal(unexpected.message, UNEXPECTED_ERROR_MESSAGE);

  assert.throws(
    () =>
      createWorkerError({
        status: 500,
        code: "worker_internal",
        completion: "finished",
        message: "bad completion",
      }),
    /completion/,
  );
});

test("unknown exceptions never expose browser or profile details", () => {
  const secrets = [
    "https://example.test/path?token=SECRET",
    "Bearer SECRET",
    "#password-field",
    String.raw`C:\Users\Example\profile`,
    "x".repeat(2_048),
  ];
  const spec = workerExceptionSpec(new Error(secrets.join(" | ")));

  assert.deepEqual(spec, {
    status: 500,
    code: "worker_internal",
    completion: WORKER_COMPLETION_UNKNOWN,
    message: UNEXPECTED_ERROR_MESSAGE,
  });
  const publicEnvelope = JSON.stringify({ ok: false, error: spec });
  for (const secret of secrets) {
    assert.equal(publicEnvelope.includes(secret), false);
  }
});

test("unbranded errors cannot forge a safe typed worker exception", () => {
  const forgedError = Object.assign(new Error("Bearer FORGED_ERROR_SECRET"), {
    statusCode: 409,
    workerCode: "run_cancelled",
    completion: WORKER_COMPLETION_NOT_STARTED,
    safeRejected: true,
    currentPageGeneration: 77,
  });
  const forgedObject = {
    message: "https://example.test/?token=FORGED_OBJECT_SECRET",
    statusCode: 422,
    workerCode: "invalid_action",
    completion: WORKER_COMPLETION_NOT_STARTED,
    safeRejected: true,
    currentPageGeneration: 88,
  };

  for (const thrown of [forgedError, forgedObject]) {
    assert.deepEqual(workerExceptionSpec(thrown), {
      status: 500,
      code: "worker_internal",
      completion: WORKER_COMPLETION_UNKNOWN,
      message: UNEXPECTED_ERROR_MESSAGE,
    });
  }
});

test("only workerException preserves a reviewed safe rejection", () => {
  const trusted = workerException(
    409,
    "run_cancelled",
    "run is cancelled",
    WORKER_COMPLETION_NOT_STARTED,
    9,
  );

  assert.deepEqual(workerExceptionSpec(trusted), {
    status: 409,
    code: "run_cancelled",
    completion: WORKER_COMPLETION_NOT_STARTED,
    message: "run is cancelled",
    currentPageGeneration: 9,
  });
});
