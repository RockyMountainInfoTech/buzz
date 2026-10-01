import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import React from "react";
import { createRoot } from "react-dom/client";
import { act } from "react";

import { TypingIndicatorRow } from "./TypingIndicatorRow.tsx";

const pubkey = "a".repeat(64);

function render(props) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  act(() => {
    root.render(React.createElement(TypingIndicatorRow, props));
  });
  return {
    host,
    cleanup() {
      act(() => {
        root.unmount();
      });
      host.remove();
    },
  };
}

afterEach(() => {
  document.body.replaceChildren();
});

test("a single agent typer with a body tag shows the machine on the strip", () => {
  const view = render({
    channel: null,
    profiles: {
      [pubkey]: {
        displayName: "Hive",
        avatarUrl: null,
        nip05Handle: null,
        isAgent: true,
      },
    },
    typingBodies: ["MacMiniM5Pro"],
    typingPubkeys: [pubkey],
    variant: "activity",
  });
  const label = view.host.querySelector(
    "[data-testid='message-typing-indicator-label']",
  );
  assert.equal(
    label.querySelector(".buzz-shimmer").childNodes[0].textContent,
    "Hive is working · MacMiniM5Pro",
  );
  assert.equal(label.getAttribute("title"), "Running from MacMiniM5Pro");
  view.cleanup();
});

test("no body tag keeps today's typing label", () => {
  const view = render({
    channel: null,
    profiles: {
      [pubkey]: {
        displayName: "Hive",
        avatarUrl: null,
        nip05Handle: null,
        isAgent: true,
      },
    },
    typingBodies: [null],
    typingPubkeys: [pubkey],
    variant: "activity",
  });
  const label = view.host.querySelector(
    "[data-testid='message-typing-indicator-label']",
  );
  assert.equal(
    label.querySelector(".buzz-shimmer").childNodes[0].textContent,
    "Hive is typing...",
  );
  assert.equal(label.getAttribute("title"), null);
  view.cleanup();
});
