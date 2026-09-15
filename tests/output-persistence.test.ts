import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { retryOutputPersistence } from "../src/features/workflow/api";
import { mergeOutputReceipt } from "../src/features/workflow/components/history-page/output-receipt";
import type { OutputReceipt } from "../src/features/workflow/types";

const savedReceipt: OutputReceipt = {
  runId: "run-1",
  nodeId: "output-1",
  stepId: "step-1",
  disposition: {
    label: "Release report",
    saveToMemory: true,
    pinMemory: true,
    memoryTitle: "Captured report",
    asFinalResult: true,
    htmlReport: false,
    includeFilesChanged: true,
  },
  resolvedMemoryTitle: "Captured report",
  state: "saved",
  storageIdentity: "memory-1",
  outputSha256: "a".repeat(64),
  artifactRequired: false,
  memoryId: "memory-1",
  errorCode: null,
  savingStartedAt: null,
  createdAt: "2026-09-09T12:00:00Z",
  updatedAt: "2026-09-09T12:00:01Z",
  deletedAt: null,
};

let originalWindow: PropertyDescriptor | undefined;

beforeEach(() => {
  originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {},
  });
});

afterEach(() => {
  clearMocks();
  if (originalWindow) {
    Object.defineProperty(globalThis, "window", originalWindow);
  } else {
    Reflect.deleteProperty(globalThis, "window");
  }
});

describe("output persistence frontend contract", () => {
  test("retry sends only durable run and node identity", async () => {
    mockIPC((command, payload) => {
      expect(command).toBe("retry_output_persistence");
      expect(payload).toEqual({ runId: "run-1", nodeId: "output-1" });
      expect(Object.keys(payload as object).sort()).toEqual(["nodeId", "runId"]);
      return savedReceipt;
    });

    expect(
      await retryOutputPersistence({ runId: "run-1", nodeId: "output-1" }),
    ).toEqual(savedReceipt);
  });

  test("a delayed receipt cannot replace a deleted tombstone", () => {
    const deleted = {
      ...savedReceipt,
      state: "deleted" as const,
      memoryId: null,
      deletedAt: "2026-09-09T12:00:03Z",
    };

    expect(mergeOutputReceipt([deleted], savedReceipt)).toEqual([deleted]);
    expect(
      mergeOutputReceipt([deleted], {
        ...savedReceipt,
        state: "saving",
        memoryId: null,
        savingStartedAt: "2026-09-09T12:00:02Z",
      }),
    ).toEqual([deleted]);
  });
});
