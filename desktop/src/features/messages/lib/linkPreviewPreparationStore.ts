import * as React from "react";

import { uploadMediaBytes } from "@/shared/api/tauri";
import type { SupportedLinkPreview } from "@/shared/lib/linkPreview";
import {
  buildLinkPreviewSnapshotTag,
  isValidLinkPreviewSnapshotCanonicalUrl,
} from "@/shared/lib/linkPreviewSnapshot";
import {
  loadLinkPreviewMetadata,
  resolveLinkPreview,
} from "@/shared/lib/useResolvedLinkPreviews";

const POST_SUBMIT_PREVIEW_BUDGET_MS = 3_000;
const SETTLED_PREVIEW_JOB_TTL_MS = 5 * 60_000;

type PreviewJob = {
  promise: Promise<string[] | null>;
  settled: boolean;
  settledAt: number | null;
};

type BackgroundPreviewTask = {
  id: number;
  skip: () => void;
};

type BackgroundPreviewSnapshot = {
  canSkip: boolean;
  isPreparing: boolean;
};

export type PreparedBackgroundLinkPreviews = {
  promise: Promise<string[][]>;
  skip: () => void;
};

const jobs = new Map<string, PreviewJob>();
const tasks = new Map<number, BackgroundPreviewTask>();
const listeners = new Set<() => void>();
let nextTaskId = 0;
let snapshot: BackgroundPreviewSnapshot = {
  canSkip: false,
  isPreparing: false,
};

function publishSnapshot(): void {
  snapshot = {
    canSkip: tasks.size > 0,
    isPreparing: tasks.size > 0,
  };
  for (const listener of listeners) listener();
}

function dataUrlBytes(dataUrl: string | null | undefined): Uint8Array | null {
  if (!dataUrl) return null;
  const comma = dataUrl.indexOf(",");
  if (comma < 0) return null;
  try {
    const binary = atob(dataUrl.slice(comma + 1));
    return Uint8Array.from(binary, (character) => character.charCodeAt(0));
  } catch {
    return null;
  }
}

async function uploadDataUrl(
  dataUrl: string | null | undefined,
  filename: string,
): Promise<{ failed: boolean; sha256: string; url: string }> {
  const bytes = dataUrlBytes(dataUrl);
  if (!bytes) return { failed: false, sha256: "", url: "" };
  try {
    const uploaded = await uploadMediaBytes([...bytes], filename);
    return { failed: false, sha256: uploaded.sha256, url: uploaded.url };
  } catch {
    return { failed: true, sha256: "", url: "" };
  }
}

async function buildSnapshot(
  candidate: SupportedLinkPreview,
): Promise<string[] | null> {
  const metadata = await loadLinkPreviewMetadata(candidate.href);
  if (!metadata) return null;
  const preview = resolveLinkPreview(candidate, metadata);
  if (!preview.snapshotReady) return null;
  const [image, favicon] = await Promise.all([
    uploadDataUrl(preview.imageDataUrl, "link-preview-image.png"),
    uploadDataUrl(preview.faviconDataUrl, "link-preview-favicon.png"),
  ]);
  if (image.failed || favicon.failed) return null;
  return buildLinkPreviewSnapshotTag({
    canonicalUrl: preview.href,
    title: preview.title,
    siteName: preview.provider,
    description: preview.description ?? "",
    imageUrl: image.url,
    imageSha256: image.sha256,
    faviconUrl: favicon.url,
    faviconSha256: favicon.sha256,
  });
}

function isReusableJob(job: PreviewJob, now = Date.now()): boolean {
  return (
    !job.settled ||
    (job.settledAt !== null && now - job.settledAt < SETTLED_PREVIEW_JOB_TTL_MS)
  );
}

/** Start or adopt the one preparation job for this exact canonical URL. */
export function prepareLinkPreview(
  candidate: SupportedLinkPreview,
): Promise<string[] | null> {
  if (
    candidate.href.startsWith("buzz://") ||
    !isValidLinkPreviewSnapshotCanonicalUrl(candidate.href)
  ) {
    return Promise.resolve(null);
  }
  const existing = jobs.get(candidate.href);
  if (existing && isReusableJob(existing)) {
    return existing.promise;
  }
  if (existing) jobs.delete(candidate.href);

  const job: PreviewJob = {
    promise: Promise.resolve(null),
    settled: false,
    settledAt: null,
  };
  job.promise = buildSnapshot(candidate)
    .catch(() => null)
    .then((tag) => {
      if (tag === null && jobs.get(candidate.href) === job) {
        jobs.delete(candidate.href);
      }
      return tag;
    })
    .finally(() => {
      job.settled = true;
      job.settledAt = Date.now();
    });
  jobs.set(candidate.href, job);
  return job.promise;
}

/**
 * Promote the frozen composer generation into a navigation-safe send task.
 * Preparation is best effort: Skip, timeout, or failure all authorize the
 * already-requested send without previews.
 */
export function prepareBackgroundLinkPreviews(
  candidates: readonly SupportedLinkPreview[],
  timeoutMs = POST_SUBMIT_PREVIEW_BUDGET_MS,
): PreparedBackgroundLinkPreviews | null {
  const external = candidates.filter(
    (candidate) =>
      !candidate.href.startsWith("buzz://") &&
      isValidLinkPreviewSnapshotCanonicalUrl(candidate.href),
  );
  if (external.length === 0) return null;

  const pending = external.some(
    (candidate) => !jobs.get(candidate.href)?.settled,
  );
  if (!pending) {
    return {
      promise: Promise.all(external.map(prepareLinkPreview)).then((tags) =>
        tags.filter((tag): tag is string[] => tag !== null),
      ),
      skip: () => undefined,
    };
  }

  const taskId = nextTaskId++;
  let finish: ((tags: string[][]) => void) | null = null;
  let terminal = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const complete = (tags: string[][]) => {
    if (terminal) return;
    terminal = true;
    if (timer !== null) clearTimeout(timer);
    tasks.delete(taskId);
    publishSnapshot();
    finish?.(tags);
  };
  const promise = new Promise<string[][]>((resolve) => {
    finish = resolve;
  });
  const skip = () => complete([]);
  tasks.set(taskId, { id: taskId, skip });
  publishSnapshot();

  timer = setTimeout(skip, timeoutMs);
  void Promise.all(external.map(prepareLinkPreview)).then((tags) => {
    complete(tags.filter((tag): tag is string[] => tag !== null));
  });

  return { promise, skip };
}

export function skipBackgroundLinkPreviews(): void {
  for (const task of [...tasks.values()]) task.skip();
}

export function resetLinkPreviewPreparations(): void {
  for (const task of [...tasks.values()]) task.skip();
  jobs.clear();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): BackgroundPreviewSnapshot {
  return snapshot;
}

export function useBackgroundLinkPreviewPreparation(): BackgroundPreviewSnapshot {
  return React.useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

export const __linkPreviewPreparationTest = {
  isReusableJob,
  jobs,
  reset: resetLinkPreviewPreparations,
};
