import { describe, it, expect } from "vitest";
import * as fc from "fast-check";
import {
  validateSettings,
  type ValidationError,
} from "../../screens/SettingsScreen";
import type { P2PSettings } from "../../services/p2pBridge";

/**
 * Generator for valid positive integers.
 */
const positiveInt = fc.integer({ min: 1, max: 1_000_000_000 });

/**
 * Generator for invalid integers (zero or negative).
 */
const nonPositiveInt = fc.integer({ min: -1_000_000, max: 0 });

/**
 * Generator for valid display names (non-empty after trimming).
 */
const validDisplayName = fc
  .string({ minLength: 1, maxLength: 50 })
  .filter((s) => s.trim().length > 0);

/**
 * Generator for invalid display names (empty or whitespace-only).
 */
const invalidDisplayName = fc.constantFrom("", "   ", "\t", "\n");

/**
 * Generator for valid port numbers.
 */
const validPort = fc.integer({ min: 1, max: 65535 });

/**
 * Generator for invalid port numbers.
 */
const invalidPort = fc.oneof(
  fc.integer({ min: -1000, max: 0 }),
  fc.integer({ min: 65536, max: 100000 }),
);

/**
 * Generator for fully valid P2PSettings.
 */
const validSettings: fc.Arbitrary<P2PSettings> = fc.record({
  display_name: validDisplayName,
  listen_port: validPort,
  chunk_size: positiveInt,
  parallel_streams: positiveInt,
  per_transfer_rate_limit: fc.integer({ min: 0, max: 1_000_000_000 }),
  download_dir: fc.string({ minLength: 0, maxLength: 100 }),
});

/**
 * Property 19: Settings Validation
 *
 * For any P2PSettings object, validateSettings SHALL return an empty error list
 * if and only if: display_name is non-empty after trimming, listen_port is in
 * 1-65535, chunk_size > 0, and parallel_streams > 0. For each invalid field,
 * exactly one ValidationError SHALL be returned for that field.
 *
 * **Validates: Requirements 17.2, 17.3**
 */
describe("Property 19: Settings Validation", () => {
  it("returns empty error list when all fields are valid", () => {
    fc.assert(
      fc.property(validSettings, (settings) => {
        const errors = validateSettings(settings);
        expect(errors).toHaveLength(0);
      }),
      { numRuns: 200 },
    );
  });

  it("returns error only for display_name when only display_name is invalid", () => {
    fc.assert(
      fc.property(
        invalidDisplayName,
        validPort,
        positiveInt,
        positiveInt,
        (display_name, listen_port, chunk_size, parallel_streams) => {
          const errors = validateSettings({
            display_name,
            listen_port,
            chunk_size,
            parallel_streams,
            per_transfer_rate_limit: 0,
            download_dir: "/tmp",
          });
          expect(errors).toHaveLength(1);
          expect(errors[0].field).toBe("display_name");
        },
      ),
      { numRuns: 200 },
    );
  });

  it("returns error only for listen_port when only listen_port is invalid", () => {
    fc.assert(
      fc.property(
        validDisplayName,
        invalidPort,
        positiveInt,
        positiveInt,
        (display_name, listen_port, chunk_size, parallel_streams) => {
          const errors = validateSettings({
            display_name,
            listen_port,
            chunk_size,
            parallel_streams,
            per_transfer_rate_limit: 0,
            download_dir: "/tmp",
          });
          expect(errors).toHaveLength(1);
          expect(errors[0].field).toBe("listen_port");
        },
      ),
      { numRuns: 200 },
    );
  });

  it("returns error only for chunk_size when only chunk_size is invalid", () => {
    fc.assert(
      fc.property(
        validDisplayName,
        validPort,
        nonPositiveInt,
        positiveInt,
        (display_name, listen_port, chunk_size, parallel_streams) => {
          const errors = validateSettings({
            display_name,
            listen_port,
            chunk_size,
            parallel_streams,
            per_transfer_rate_limit: 0,
            download_dir: "/tmp",
          });
          expect(errors).toHaveLength(1);
          expect(errors[0].field).toBe("chunk_size");
        },
      ),
      { numRuns: 200 },
    );
  });

  it("returns error only for parallel_streams when only parallel_streams is invalid", () => {
    fc.assert(
      fc.property(
        validDisplayName,
        validPort,
        positiveInt,
        nonPositiveInt,
        (display_name, listen_port, chunk_size, parallel_streams) => {
          const errors = validateSettings({
            display_name,
            listen_port,
            chunk_size,
            parallel_streams,
            per_transfer_rate_limit: 0,
            download_dir: "/tmp",
          });
          expect(errors).toHaveLength(1);
          expect(errors[0].field).toBe("parallel_streams");
        },
      ),
      { numRuns: 200 },
    );
  });

  it("errors reference only invalid fields; valid fields have no errors", () => {
    fc.assert(
      fc.property(
        fc.record({
          display_name: fc.oneof(validDisplayName, invalidDisplayName),
          listen_port: fc.oneof(validPort, invalidPort),
          chunk_size: fc.oneof(positiveInt, nonPositiveInt),
          parallel_streams: fc.oneof(positiveInt, nonPositiveInt),
          per_transfer_rate_limit: fc.integer({ min: 0, max: 1_000_000 }),
          download_dir: fc.string({ minLength: 0, maxLength: 50 }),
        }),
        (settings: P2PSettings) => {
          const errors = validateSettings(settings);
          const errorFields = new Set(
            errors.map((e: ValidationError) => e.field),
          );

          if (settings.display_name.trim().length > 0) {
            expect(errorFields.has("display_name")).toBe(false);
          }
          if (
            Number.isInteger(settings.listen_port) &&
            settings.listen_port >= 1 &&
            settings.listen_port <= 65535
          ) {
            expect(errorFields.has("listen_port")).toBe(false);
          }
          if (
            settings.chunk_size > 0 &&
            Number.isInteger(settings.chunk_size)
          ) {
            expect(errorFields.has("chunk_size")).toBe(false);
          }
          if (
            settings.parallel_streams > 0 &&
            Number.isInteger(settings.parallel_streams)
          ) {
            expect(errorFields.has("parallel_streams")).toBe(false);
          }
        },
      ),
      { numRuns: 200 },
    );
  });
});
