import { describe, it, expect } from "vitest";
import * as fc from "fast-check";
import { sortHistory } from "../../screens/HistoryScreen";
import type { TransferHistoryEntry } from "../../services/p2pBridge";

/**
 * Arbitrary generator for TransferHistoryEntry (P2P format).
 */
const transferHistoryEntryArb: fc.Arbitrary<TransferHistoryEntry> = fc.record({
  session_id: fc.uuid(),
  file_name: fc.string({ minLength: 1, maxLength: 50 }),
  direction: fc.constantFrom("sent", "received"),
  peer_display_name: fc.string({ minLength: 1, maxLength: 30 }),
  timestamp: fc
    .integer({ min: 0, max: 2_000_000_000_000 })
    .map((ts) => new Date(ts).toISOString()),
  file_size: fc.integer({ min: 1, max: 1_000_000_000 }),
  status: fc.constantFrom("success", "failed"),
  failure_reason: fc.oneof(
    fc.constant(null),
    fc.string({ minLength: 1, maxLength: 50 }),
  ),
});

/**
 * Property 21: Transfer History Ordering and Completeness
 *
 * For any list of TransferHistoryEntry objects, the displayed list SHALL be sorted
 * in descending chronological order by timestamp, and each entry SHALL contain
 * non-empty file_name, direction, peer_display_name, and a valid status.
 *
 * **Validates: Requirements 13.1, 13.2**
 */
describe("Property 21: Transfer History Ordering and Completeness", () => {
  it("sortHistory returns entries sorted descending by timestamp", () => {
    fc.assert(
      fc.property(
        fc.array(transferHistoryEntryArb, { minLength: 0, maxLength: 50 }),
        (entries) => {
          const sorted = sortHistory(entries);

          // Length preserved
          expect(sorted).toHaveLength(entries.length);

          // Descending order by timestamp
          for (let i = 1; i < sorted.length; i++) {
            expect(
              new Date(sorted[i - 1].timestamp).getTime(),
            ).toBeGreaterThanOrEqual(new Date(sorted[i].timestamp).getTime());
          }
        },
      ),
      { numRuns: 200 },
    );
  });

  it("each entry contains all required fields after sorting", () => {
    fc.assert(
      fc.property(
        fc.array(transferHistoryEntryArb, { minLength: 1, maxLength: 50 }),
        (entries) => {
          const sorted = sortHistory(entries);

          for (const entry of sorted) {
            expect(entry.file_name).toBeDefined();
            expect(typeof entry.file_name).toBe("string");

            expect(entry.direction).toBeDefined();
            expect(typeof entry.direction).toBe("string");

            expect(entry.peer_display_name).toBeDefined();
            expect(typeof entry.peer_display_name).toBe("string");

            expect(entry.timestamp).toBeDefined();
            expect(typeof entry.timestamp).toBe("string");

            expect(entry.file_size).toBeDefined();
            expect(typeof entry.file_size).toBe("number");

            expect(entry.status).toBeDefined();
            expect(["success", "failed"]).toContain(entry.status);
          }
        },
      ),
      { numRuns: 200 },
    );
  });

  it("sortHistory does not mutate the original array", () => {
    fc.assert(
      fc.property(
        fc.array(transferHistoryEntryArb, { minLength: 0, maxLength: 20 }),
        (entries) => {
          const original = [...entries];
          sortHistory(entries);

          expect(entries).toEqual(original);
        },
      ),
      { numRuns: 200 },
    );
  });
});
