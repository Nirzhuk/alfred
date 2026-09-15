// Synthetic DTO metadata only. Rust tests exercise real revisions and digests.
export function memoryVersionFixture(semanticRevision = 1) {
  return {
    semanticRevision,
    contentDigest: "d".repeat(64),
    artifactContentDigest: null as string | null,
    factKey: null as string | null,
    observedAt: null as string | null,
    lastConfirmedAt: null as string | null,
    validFrom: null as string | null,
    expiresAt: null as string | null,
  };
}

export function candidateVersionFixture(candidateRevision = 1) {
  return {
    candidateRevision,
    expectedTargetMemoryId: null as string | null,
    expectedTargetSemanticRevision: null as number | null,
    expectedTargetContentDigest: null as string | null,
    targetComplete: false,
    factKey: null as string | null,
    observedAt: null as string | null,
    lastConfirmedAt: null as string | null,
    validFrom: null as string | null,
    expiresAt: null as string | null,
    possibleConflicts: [] as Array<{
      memoryId: string;
      title: string;
      relation: "exact_duplicate" | "same_fact_key" | "possible_relationship";
    }>,
  };
}
