import assert from "node:assert/strict";
import test from "node:test";

import {
  __linkPreviewPreparationTest,
  prepareBackgroundLinkPreviews,
  prepareLinkPreview,
} from "./linkPreviewPreparationStore.ts";

const first = { href: "https://example.com/first" };
const second = { href: "https://example.com/second" };
const firstTag = ["link-preview", "snapshot", first.href];

function deferred() {
  let resolve;
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function seed(candidate, promise, settled = false, settledAt = Date.now()) {
  __linkPreviewPreparationTest.jobs.set(candidate.href, {
    promise,
    settled,
    settledAt: settled ? settledAt : null,
  });
}

test.afterEach(() => {
  __linkPreviewPreparationTest.reset();
});

test("adopts one in-flight job for the same canonical URL", () => {
  const pending = deferred();
  seed(first, pending.promise);

  assert.equal(prepareLinkPreview(first), pending.promise);
  assert.equal(prepareLinkPreview(first), pending.promise);
  pending.resolve(firstTag);
});

test("expires settled jobs while retaining in-flight and recent work", () => {
  const now = 1_000_000;
  assert.equal(
    __linkPreviewPreparationTest.isReusableJob(
      { promise: Promise.resolve(firstTag), settled: false, settledAt: null },
      now,
    ),
    true,
  );
  assert.equal(
    __linkPreviewPreparationTest.isReusableJob(
      { promise: Promise.resolve(firstTag), settled: true, settledAt: now - 1 },
      now,
    ),
    true,
  );
  assert.equal(
    __linkPreviewPreparationTest.isReusableJob(
      {
        promise: Promise.resolve(firstTag),
        settled: true,
        settledAt: now - 5 * 60_000,
      },
      now,
    ),
    false,
  );
});

test("keeps successful sibling tags when another URL fails", async () => {
  const pending = deferred();
  seed(first, Promise.resolve(firstTag), true);
  seed(second, pending.promise);

  const preparation = prepareBackgroundLinkPreviews([first, second], 1_000);
  assert.ok(preparation);
  pending.resolve(null);

  assert.deepEqual(await preparation.promise, [firstTag]);
});

test("timeout sends without synthetic suppression and ignores late completion", async () => {
  const pending = deferred();
  seed(first, pending.promise);

  const preparation = prepareBackgroundLinkPreviews([first], 0);
  assert.ok(preparation);
  assert.deepEqual(await preparation.promise, []);

  pending.resolve(firstTag);
  await pending.promise;
  assert.deepEqual(await preparation.promise, []);
});

test("Skip wins completion and resolves exactly once", async () => {
  const pending = deferred();
  seed(first, pending.promise);

  const preparation = prepareBackgroundLinkPreviews([first], 1_000);
  assert.ok(preparation);
  preparation.skip();
  pending.resolve(firstTag);

  assert.deepEqual(await preparation.promise, []);
});

test("Skip after completion cannot replace finalized tags", async () => {
  const pending = deferred();
  seed(first, pending.promise);

  const preparation = prepareBackgroundLinkPreviews([first], 1_000);
  assert.ok(preparation);
  pending.resolve(firstTag);
  assert.deepEqual(await preparation.promise, [firstTag]);

  preparation.skip();
  assert.deepEqual(await preparation.promise, [firstTag]);
});

test("already-settled partial results contain only successful tags", async () => {
  seed(first, Promise.resolve(firstTag), true);
  seed(second, Promise.resolve(null), true);

  const preparation = prepareBackgroundLinkPreviews([first, second]);
  assert.ok(preparation);
  assert.deepEqual(await preparation.promise, [firstTag]);
});
