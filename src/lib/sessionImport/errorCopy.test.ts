import { beforeAll, describe, expect, it } from "vitest";
import { createT, loadAllLocaleCatalogs, type Locale } from "@/i18n";
import { LOCALES } from "@/i18n/messages";
import { importErrorKey, importIssueKey } from "./errorCopy";

describe("importErrorKey", () => {
  it.each(Object.getOwnPropertyNames(Object.prototype))("does not accept inherited property %s as an error code", (code) => {
    expect(importErrorKey(code)).toBe("sessionImport.err.generic");
    expect(importErrorKey(` ${code} : SOURCE_BUSY: C:\\synthetic-private\\catalog.sqlite`)).toBe("sessionImport.err.generic");
  });

  it.each([
    ["SOURCE_NOT_FOUND", "sourceNotFound"],
    ["SOURCE_PERMISSION_DENIED", "permissionDenied"],
    ["SOURCE_BUSY", "sourceBusy"],
    ["SOURCE_CHANGED", "sourceChanged"],
    ["SNAPSHOT_UNAVAILABLE", "snapshotUnavailable"],
    ["FORMAT_UNSUPPORTED", "formatUnsupported"],
    ["SCHEMA_UNSUPPORTED", "schemaUnsupported"],
    ["PARTIAL_HISTORY", "partialHistory"],
    ["WORKSPACE_UNRESOLVED", "workspaceUnresolved"],
    ["MAPPING_CONFLICT", "mappingConflict"],
    ["RESOURCE_LIMIT", "resourceLimit"],
    ["DISK_SPACE_LOW", "diskSpaceLow"],
    ["PLAN_STALE", "planStale"],
    ["JOB_STATE_CHANGED", "jobStateChanged"],
    ["JOB_ACTION_CONFLICT", "jobActionConflict"],
    ["SELECTION_BOUNDARY_INVALID", "selectionInvalid"],
    ["IMPORTED_HISTORY_READ_ONLY", "readOnly"],
    ["HISTORY_ACCESS_UNAVAILABLE", "historyUnavailable"],
    ["HANDOFF_CHANGED", "handoffChanged"],
    ["DISPATCH_UNKNOWN", "dispatchUnknown"],
    ["IMPORT_SCHEMA_TOO_NEW", "schemaTooNew"],
  ])("preserves the declared %s mapping", (code, suffix) => {
    expect(importErrorKey(code)).toBe(`sessionImport.err.${suffix}`);
    expect(importErrorKey(` ${code} : constructor: synthetic diagnostic`)).toBe(`sessionImport.err.${suffix}`);
    expect(importErrorKey(`${code}_UNKNOWN: ${code}`)).toBe("sessionImport.err.generic");
    expect(importErrorKey(`${code.toLowerCase()}: synthetic diagnostic`)).toBe("sessionImport.err.generic");
  });

  it("maps host prefixes, not substring theater", () => {
    expect(importErrorKey("IMPORTED_HISTORY_READ_ONLY: imported history cannot be used with native session commands")).toBe(
      "sessionImport.err.readOnly",
    );
    expect(importErrorKey("FORMAT_UNSUPPORTED: JSON object has no messages")).toBe(
      "sessionImport.err.formatUnsupported",
    );
    expect(importErrorKey("PLAN_STALE: scan epoch mismatch")).toBe(
      "sessionImport.err.planStale",
    );
    expect(importErrorKey("SELECTION_BOUNDARY_INVALID: candidate maps differ")).toBe(
      "sessionImport.err.selectionInvalid",
    );
    expect(importErrorKey("MAPPING_CONFLICT: path belongs to another registered source")).toBe(
      "sessionImport.err.mappingConflict",
    );
    expect(importErrorKey("not-a-code")).toBe("sessionImport.err.generic");
    expect(importErrorKey("HANDOFF_CHANGED: draft edited")).toBe("sessionImport.err.handoffChanged");
  });

  it.each([
    ["SOURCE_BUSY", "sessionImport.err.sourceBusy"],
    ["DISK_SPACE_LOW", "sessionImport.err.diskSpaceLow"],
  ] as const)("maps the typed %s prefix without interpreting diagnostic text", (code, key) => {
    expect(importErrorKey(code)).toBe(key);
    expect(importErrorKey(` ${code} : C:\\synthetic-private\\catalog.sqlite: host diagnostic`)).toBe(key);
  });

  it.each([
    "operation failed: SOURCE_BUSY: C:\\synthetic-private\\catalog.sqlite",
    "operation failed: DISK_SPACE_LOW: C:\\synthetic-private\\archive",
    "SOURCE_BUSY_EXTRA: diagnostic",
    "DISK_SPACE_LOW_EXTRA: diagnostic",
  ])("keeps unknown prefixes generic: %s", (raw) => {
    expect(importErrorKey(raw)).toBe("sessionImport.err.generic");
  });
});

describe("damaged input diagnoses", () => {
  beforeAll(loadAllLocaleCatalogs);

  it.each([
    ["empty-jsonl", "empty"],
    ["trailing-incomplete", "trailingIncomplete"],
    ["broken-record", "brokenRecord"],
    ["invalid-utf8", "invalidUtf8"],
    ["binary-record", "binary"],
    ["jsonl-record-too-large", "oversizedLine"],
    ["unknown-record", "unknownRecord"],
  ] as const)("localizes %s consistently in preview, failures and revision health", (reason, suffix) => {
    const key = `sessionImport.diagnostic.${suffix}` as const;
    expect(importIssueKey(reason)).toBe(key);
    const code = reason === "jsonl-record-too-large" ? "RESOURCE_LIMIT" : "FORMAT_UNSUPPORTED";
    expect(importErrorKey(`${code}: ${reason}`)).toBe(key);
    for (const locale of LOCALES) {
      expect(createT(locale)(key)).not.toBe(key);
      expect(createT(locale)(key).trim().length).toBeGreaterThan(0);
      if (locale !== "en") expect(createT(locale)(key)).not.toBe(createT("en")(key));
    }
    expect(importErrorKey(`unknown: ${reason}`)).toBe("sessionImport.err.generic");
    expect(importErrorKey(`${code}: private path ${reason}`)).not.toBe(key);
  });

  it.each(Object.getOwnPropertyNames(Object.prototype))("does not treat inherited %s as a diagnosis", (code) => {
    expect(importIssueKey(code)).toBeNull();
    expect(importErrorKey(`FORMAT_UNSUPPORTED: ${code}`)).toBe("sessionImport.err.formatUnsupported");
  });
});

describe("busy import resource copy", () => {
  beforeAll(loadAllLocaleCatalogs);

  const expected: Record<Locale, string> = {
    de: "Eine Importressource ist ausgelastet oder gesperrt",
    en: "An import resource is busy or locked",
    es: "Un recurso de importación está ocupado o bloqueado",
    fil: "Abala o naka-lock ang isang resource ng pag-import",
    fr: "Une ressource d’importation est occupée ou verrouillée",
    id: "Sumber daya impor sedang sibuk atau terkunci",
    it: "Una risorsa di importazione è occupata o bloccata",
    ja: "インポートに必要なリソースが使用中、またはロックされています",
    ko: "가져오기 리소스가 사용 중이거나 잠겨 있습니다",
    "pt-BR": "Um recurso de importação está ocupado ou bloqueado",
    ru: "Ресурс импорта занят или заблокирован",
    ta: "இறக்குமதிக்கான வளம் பயன்பாட்டில் உள்ளது அல்லது பூட்டப்பட்டுள்ளது",
    uk: "Ресурс імпорту зайнятий або заблокований",
    zh: "导入资源正忙或被锁定",
    "zh-TW": "匯入資源忙碌或被鎖定",
  };

  it.each(Object.entries(expected))("does not blame only the external source in %s", (locale, text) => {
    expect(createT(locale as Locale)(importErrorKey("SOURCE_BUSY: destination catalog or stage locked"))).toBe(text);
  });
});
