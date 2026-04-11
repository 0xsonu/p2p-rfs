import { describe, it, expect } from "vitest";
import * as fc from "fast-check";
import { computeProgress } from "../../hooks/useTransfer";

/**
 * Property 16: Progress Computation
 *
 * For any totalChunks > 0, completedChunks in [0, totalChunks], non-negative
 * elapsedTime, and non-negative bytesTransferred, the computeProgress function
 * SHALL return a percentage equal to (completedChunks / totalChunks) * 100,
 * a non-negative speed, and a non-negative ETA.
 *
 * **Validates: Requirements 12.1, 15.2, 16.4**
 */
describe("Property 16: Progress Computation", () => {
  it("percentage equals (completedChunks / totalChunks) * 100", () => {
    fc.assert(
      fc.property(
        fc
          .integer({ min: 1, max: 100_000 })
          .chain((total) =>
            fc.tuple(
              fc.constant(total),
              fc.integer({ min: 0, max: total }),
              fc.double({ min: 0, max: 1_000_000, noNaN: true }),
              fc.double({ min: 0, max: 1e12, noNaN: true }),
            ),
          ),
        ([totalChunks, completedChunks, elapsedTime, bytesTransferred]) => {
          const result = computeProgress(
            totalChunks,
            completedChunks,
            elapsedTime,
            bytesTransferred,
          );
          const expected = (completedChunks / totalChunks) * 100;
          expect(result.percentage).toBeCloseTo(expected, 10);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("speed is non-negative for any non-negative inputs", () => {
    fc.assert(
      fc.property(
        fc
          .integer({ min: 1, max: 100_000 })
          .chain((total) =>
            fc.tuple(
              fc.constant(total),
              fc.integer({ min: 0, max: total }),
              fc.double({ min: 0, max: 1_000_000, noNaN: true }),
              fc.double({ min: 0, max: 1e12, noNaN: true }),
            ),
          ),
        ([totalChunks, completedChunks, elapsedTime, bytesTransferred]) => {
          const result = computeProgress(
            totalChunks,
            completedChunks,
            elapsedTime,
            bytesTransferred,
          );
          expect(result.speed).toBeGreaterThanOrEqual(0);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("ETA is non-negative for any non-negative inputs", () => {
    fc.assert(
      fc.property(
        fc
          .integer({ min: 1, max: 100_000 })
          .chain((total) =>
            fc.tuple(
              fc.constant(total),
              fc.integer({ min: 0, max: total }),
              fc.double({ min: 0, max: 1_000_000, noNaN: true }),
              fc.double({ min: 0, max: 1e12, noNaN: true }),
            ),
          ),
        ([totalChunks, completedChunks, elapsedTime, bytesTransferred]) => {
          const result = computeProgress(
            totalChunks,
            completedChunks,
            elapsedTime,
            bytesTransferred,
          );
          expect(result.eta).toBeGreaterThanOrEqual(0);
        },
      ),
      { numRuns: 200 },
    );
  });
});
