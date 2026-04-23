/**
 * Minimal test harness for imperative iroh-http tests.
 *
 * Used by lifecycle, error handling, and stress tests — anything that
 * doesn't fit the data-driven cases.json model.
 *
 * Usage:
 *   import { suite, test, assert, assertThrows, run } from "../harness.mjs";
 *
 *   suite("lifecycle");
 *   test("node creates successfully", async () => { ... });
 *   await run();
 */

let suiteName = "tests";
const tests = [];
let passed = 0;
let failed = 0;
const failures = [];

export function suite(name) {
  suiteName = name;
}

export function test(name, fn) {
  tests.push({ name, fn });
}

export function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "assertion failed");
  }
}

export function assertEqual(actual, expected, label) {
  if (actual !== expected) {
    throw new Error(
      `${label || "assertEqual"}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`
    );
  }
}

export function assertNotEqual(actual, unexpected, label) {
  if (actual === unexpected) {
    throw new Error(
      `${label || "assertNotEqual"}: value must not equal ${JSON.stringify(unexpected)}`
    );
  }
}

export function assertMatch(str, regex, label) {
  if (!regex.test(str)) {
    throw new Error(
      `${label || "assertMatch"}: ${JSON.stringify(str)} does not match ${regex}`
    );
  }
}

export function assertInstanceOf(value, ctor, label) {
  if (!(value instanceof ctor)) {
    throw new Error(
      `${label || "assertInstanceOf"}: expected instance of ${ctor.name}`
    );
  }
}

export async function assertThrows(fn, messagePattern) {
  try {
    await fn();
    throw new Error("expected function to throw, but it did not");
  } catch (err) {
    if (err.message === "expected function to throw, but it did not") throw err;
    if (messagePattern && !err.message.includes(messagePattern)) {
      throw new Error(
        `expected error containing "${messagePattern}", got: "${err.message}"`
      );
    }
    return err;
  }
}

export async function assertResolves(promise) {
  try {
    return await promise;
  } catch (err) {
    throw new Error(`expected promise to resolve, but it rejected: ${err.message}`);
  }
}

/**
 * Run all registered tests.
 * Returns process exit code (0 = all pass, 1 = failures).
 */
export async function run() {
  console.log(`\n═══ ${suiteName} ═══\n`);

  for (const t of tests) {
    try {
      await t.fn();
      passed++;
      console.log(`  ✓ ${t.name}`);
    } catch (err) {
      failed++;
      failures.push({ name: t.name, error: err });
      console.log(`  ✗ ${t.name}`);
      console.log(`      ${err.message}`);
    }
  }

  console.log(`\n${"─".repeat(50)}`);
  console.log(`Results: ${passed} passed, ${failed} failed\n`);

  if (failures.length > 0) {
    console.log("Failed:");
    for (const f of failures) {
      console.log(`  - ${f.name}`);
    }
    console.log("");
  }

  return failed > 0 ? 1 : 0;
}
