/**
 * Display-only machine name. Mirrors `buzz_core::body_label::publishable`:
 * trim, reject empty, reject more than 64 characters, reject controls
 * (C0, DEL, and C1). Nothing authorizes or ranks a turn on this string.
 */

export const MAX_BODY_LABEL_LENGTH = 64;

function isControlCodePoint(code: number) {
  return code <= 0x1f || (code >= 0x7f && code <= 0x9f);
}

export function publishableMachineName(
  raw: string | null | undefined,
): string | null {
  if (typeof raw !== "string") {
    return null;
  }
  const name = raw.trim();
  if (name.length === 0 || Array.from(name).length > MAX_BODY_LABEL_LENGTH) {
    return null;
  }
  const hasControl = Array.from(name).some((character) => {
    const code = character.codePointAt(0) ?? 0;
    return isControlCodePoint(code);
  });
  return hasControl ? null : name;
}

/** Latest `body` tag wins. A missing or invalid tag clears the label. */
export function typingBodyFromTags(
  tags: readonly (readonly string[])[] | null | undefined,
): string | null {
  if (!tags) {
    return null;
  }
  let body: string | null = null;
  for (const tag of tags) {
    if (tag[0] === "body") {
      body = publishableMachineName(tag[1] ?? null);
    }
  }
  return body;
}

/**
 * A newer typing event replaces the stored body, including when the new
 * event has no publishable tag.
 */
export function typingBodyAfterEvent(
  _previous: string | null,
  tags: readonly (readonly string[])[] | null | undefined,
): string | null {
  return typingBodyFromTags(tags);
}

export function sentFromLabel(
  tags: readonly (readonly string[])[] | null | undefined,
): string | null {
  const name = typingBodyFromTags(tags);
  return name ? `Sent from ${name}` : null;
}

export function singleWorkingBody(
  entries: readonly { pubkey: string; body?: string | null }[],
  workingPubkeys: readonly string[],
): string | null {
  if (workingPubkeys.length !== 1) {
    return null;
  }
  const target = workingPubkeys[0]?.toLowerCase();
  if (!target) {
    return null;
  }
  let matched = false;
  let body: string | null = null;
  for (const entry of entries) {
    if (entry.pubkey.toLowerCase() !== target) {
      continue;
    }
    matched = true;
    body = publishableMachineName(entry.body);
  }
  return matched ? body : null;
}
