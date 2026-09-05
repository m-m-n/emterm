/**
 * Structural assertions over the GitHub Actions workflow definitions
 * (task0003, bun-install-reproducibility). These parse the workflow YAML as
 * data rather than matching raw text, so a reformatted-but-equivalent
 * workflow does not fail the assertions and a commented-out install line
 * does not pass them (task0003.md "Test Notes").
 *
 * AC-1..AC-5 are asserted directly; AC-6 (release.yml still runs its bundle
 * builds, unmoved) gets a structural step-sequence guard alongside them, per
 * task0003.md's Test Notes recommendation. AC-7 is this file itself passing.
 */

import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { parse as parseYaml } from "yaml";

const WORKFLOWS_DIR = join(import.meta.dir, ".github", "workflows");

interface WorkflowStep {
  name?: string;
  run?: string;
  uses?: string;
  with?: Record<string, unknown>;
  [key: string]: unknown;
}

interface WorkflowJob {
  "runs-on"?: string;
  steps: WorkflowStep[];
  [key: string]: unknown;
}

interface WorkflowFile {
  name?: string;
  on: Record<string, { branches?: string[] } | null>;
  jobs: Record<string, WorkflowJob>;
}

function loadWorkflows(): Array<{ file: string; doc: WorkflowFile }> {
  const files = readdirSync(WORKFLOWS_DIR).filter(
    (f) => f.endsWith(".yml") || f.endsWith(".yaml"),
  );
  return files.map((file) => ({
    file,
    doc: parseYaml(
      readFileSync(join(WORKFLOWS_DIR, file), "utf-8"),
    ) as WorkflowFile,
  }));
}

function allSteps(
  doc: WorkflowFile,
): Array<{ jobId: string; step: WorkflowStep; index: number }> {
  const result: Array<{ jobId: string; step: WorkflowStep; index: number }> =
    [];
  for (const [jobId, job] of Object.entries(doc.jobs)) {
    job.steps.forEach((step, index) => {
      result.push({ jobId, step, index });
    });
  }
  return result;
}

// Matches an invocation of the project's full bun test suite: `bun test` or
// `bun run test` (package.json's "test" script is "bun test"), as a whole
// shell token rather than a substring of some other command.
const RUNS_TEST_SUITE_RE = /(?:^|[\s;&|(])bun\s+(?:run\s+)?test(?:$|[\s;&|)])/;

// Matches any `bun install` invocation, so it can be checked for the
// frozen-lockfile flag.
const BUN_INSTALL_RE = /(?:^|[\s;&|(])bun\s+install\b/;
const FROZEN_FLAG_RE = /--frozen-lockfile\b/;

// A run step consumes the dependency graph if it invokes bun (other than
// `bun install` itself), bunx, npm, or tsc.
const CONSUMES_DEPENDENCY_GRAPH_RE =
  /(?:^|[\s;&|(])(?:bunx|npm|tsc)\b|(?:^|[\s;&|(])bun\s+(?!install\b)\S/;

function findTestStep(doc: WorkflowFile) {
  return allSteps(doc).find(
    ({ step }) => typeof step.run === "string" && RUNS_TEST_SUITE_RE.test(step.run),
  );
}

const workflows = loadWorkflows();

describe("CI workflow: exactly one workflow runs the bun test suite (AC-1)", () => {
  test("exactly one workflow file has a step running the bun test suite", () => {
    const withTestStep = workflows.filter(({ doc }) => findTestStep(doc));
    expect(withTestStep.map(({ file }) => file)).toHaveLength(1);
  });

  test("that workflow's trigger covers push to main and pull_request targeting main", () => {
    const withTestStep = workflows.filter(({ doc }) => findTestStep(doc));
    const { doc } = withTestStep[0]!;

    expect(doc.on.push?.branches).toContain("main");
    expect(doc.on.pull_request?.branches).toContain("main");
  });
});

describe("CI workflow: frozen install precedes the test step, nothing before it touches deps (AC-2)", () => {
  // Each test resolves the test-running workflow independently (rather than
  // hoisting the lookup to describe-body scope) so a missing/ambiguous
  // workflow fails the individual test with a normal assertion instead of
  // throwing during collection and swallowing both tests as a single
  // uncaught error.
  function resolveTestWorkflow() {
    const withTestStep = workflows.filter(({ doc }) => findTestStep(doc));
    expect(withTestStep).toHaveLength(1);
    const { doc } = withTestStep[0]!;
    const testStepEntry = findTestStep(doc)!;
    return { doc, testStepEntry };
  }

  test("a frozen-lockfile install step exists in the same job, before the test step", () => {
    const { doc, testStepEntry } = resolveTestWorkflow();
    const jobSteps = doc.jobs[testStepEntry.jobId]!.steps;
    const installIndex = jobSteps.findIndex(
      (step) =>
        typeof step.run === "string" &&
        BUN_INSTALL_RE.test(step.run) &&
        FROZEN_FLAG_RE.test(step.run),
    );

    expect(installIndex).toBeGreaterThanOrEqual(0);
    expect(installIndex).toBeLessThan(testStepEntry.index);
  });

  test("no step before the install step consumes the dependency graph", () => {
    const { doc, testStepEntry } = resolveTestWorkflow();
    const jobSteps = doc.jobs[testStepEntry.jobId]!.steps;
    const installIndex = jobSteps.findIndex(
      (step) =>
        typeof step.run === "string" &&
        BUN_INSTALL_RE.test(step.run) &&
        FROZEN_FLAG_RE.test(step.run),
    );

    for (let i = 0; i < installIndex; i++) {
      const step = jobSteps[i]!;
      if (typeof step.run === "string") {
        expect(CONSUMES_DEPENDENCY_GRAPH_RE.test(step.run)).toBe(false);
      }
    }
  });
});

describe("CI workflow: the test step covers the viewer entry tests (AC-3)", () => {
  test("the test step's command runs the whole suite or names the viewer entry test path", () => {
    const withTestStep = workflows.filter(({ doc }) => findTestStep(doc));
    const { doc } = withTestStep[0]!;
    const { step } = findTestStep(doc)!;
    const run = step.run as string;

    const runsWholeSuite = /(?:^|[\s;&|(])bun\s+(?:run\s+)?test\s*$/m.test(
      run.trim(),
    );
    const namesEntryTestPath = run.includes(
      "src-tauri/viewer/web/entry.test.ts",
    );

    expect(runsWholeSuite || namesEntryTestPath).toBe(true);
  });
});

describe("Every CI install invocation is frozen-lockfile (AC-4)", () => {
  test("no plain `bun install` remains in any workflow file", () => {
    const offenders: string[] = [];

    for (const { file, doc } of workflows) {
      for (const { jobId, step, index } of allSteps(doc)) {
        if (typeof step.run !== "string") continue;
        for (const line of step.run.split("\n")) {
          if (BUN_INSTALL_RE.test(line) && !FROZEN_FLAG_RE.test(line)) {
            offenders.push(`${file}:${jobId}[${index}]: ${line.trim()}`);
          }
        }
      }
    }

    expect(offenders).toEqual([]);
  });

  test("at least one frozen-lockfile install invocation exists (the guarantee is exercised)", () => {
    const frozenInstalls: string[] = [];

    for (const { doc } of workflows) {
      for (const { step } of allSteps(doc)) {
        if (
          typeof step.run === "string" &&
          BUN_INSTALL_RE.test(step.run) &&
          FROZEN_FLAG_RE.test(step.run)
        ) {
          frozenInstalls.push(step.run);
        }
      }
    }

    expect(frozenInstalls.length).toBeGreaterThan(0);
  });
});

describe("No workflow step generates, edits, or commits a lockfile (AC-5)", () => {
  const LOCKFILE_MUTATION_RE =
    /\bbun\s+(?:update|upgrade)\b|\bgit\s+(?:add|commit)\b[^\n]*bun\.lock|>\s*bun\.lock\b/;

  test("no step's run script mutates or commits bun.lock", () => {
    const offenders: string[] = [];

    for (const { file, doc } of workflows) {
      for (const { jobId, step, index } of allSteps(doc)) {
        if (typeof step.run !== "string") continue;
        if (LOCKFILE_MUTATION_RE.test(step.run)) {
          offenders.push(`${file}:${jobId}[${index}]`);
        }
      }
    }

    expect(offenders).toEqual([]);
  });
});

describe("release.yml still runs its bundle builds on both platforms, unmoved (AC-6)", () => {
  function loadReleaseWorkflow(): WorkflowFile {
    return parseYaml(
      readFileSync(join(WORKFLOWS_DIR, "release.yml"), "utf-8"),
    ) as WorkflowFile;
  }

  test("build-linux job step sequence is unchanged (names, order, count)", () => {
    const doc = loadReleaseWorkflow();
    const stepNames = doc.jobs["build-linux"]!.steps.map((s) => s.name);

    expect(stepNames).toEqual([
      "Checkout",
      "Setup Bun",
      "Install Rust stable",
      "Rust cache",
      "Install Linux dependencies",
      "Cache bundled fonts",
      "Fetch bundled fonts",
      "Set version from tag",
      "Install frontend dependencies",
      "Build GUI deb",
      "Upload GUI deb to release",
    ]);
  });

  test("build-linux delegates the bundle build to scripts/build-dpkg.sh, unchanged", () => {
    const doc = loadReleaseWorkflow();
    const step = doc.jobs["build-linux"]!.steps.find(
      (s) => s.name === "Build GUI deb",
    )!;

    expect(step.run).toBe("bash scripts/build-dpkg.sh");
  });

  test("build-windows job step sequence is unchanged (names, order, count)", () => {
    const doc = loadReleaseWorkflow();
    const stepNames = doc.jobs["build-windows"]!.steps.map((s) => s.name);

    expect(stepNames).toEqual([
      "Checkout",
      "Setup Bun",
      "Install Rust stable",
      "Rust cache",
      "Cache bundled fonts",
      "Fetch bundled fonts",
      "Set version from tag",
      "Install frontend dependencies",
      "Build web bundles",
      "Build Windows binary",
      "Upload Windows binary to release",
    ]);
  });

  test("build-windows still builds both the viewer and settings bundles", () => {
    const doc = loadReleaseWorkflow();
    const step = doc.jobs["build-windows"]!.steps.find(
      (s) => s.name === "Build web bundles",
    )!;

    expect(step.run).toContain("bun run build:viewer");
    expect(step.run).toContain("bun run build:settings");
  });

  test("both platform install steps are frozen-lockfile installs", () => {
    const doc = loadReleaseWorkflow();
    for (const jobId of ["build-linux", "build-windows"]) {
      const step = doc.jobs[jobId]!.steps.find(
        (s) => s.name === "Install frontend dependencies",
      )!;
      expect(step.run).toBe("bun install --frozen-lockfile");
    }
  });
});
