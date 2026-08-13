import * as React from "react";

import { hasPrimaryShortcutModifier } from "@/shared/lib/platform";
import {
  HUDDLE_SHORTCUT_EVENT,
  type HuddleShortcutDetail,
} from "@/shared/lib/keyboard-shortcuts";

type AppShellKeyboardShortcutsOptions = {
  activeChannelId: string | null;
  canSearchCurrentChannel: boolean;
  disabled: boolean;
  onBrowseChannels: () => void;
  onCreateChannel: () => void;
  onGoHome: () => unknown;
  onNewMessage: () => unknown;
  onSearchCurrentChannel: () => void;
  onSearchEverything: () => void;
};

export function useAppShellKeyboardShortcuts({
  activeChannelId,
  canSearchCurrentChannel,
  disabled,
  onBrowseChannels,
  onCreateChannel,
  onGoHome,
  onNewMessage,
  onSearchCurrentChannel,
  onSearchEverything,
}: AppShellKeyboardShortcutsOptions) {
  React.useLayoutEffect(() => {
    if (disabled) return;

    function handleKeyDown(event: KeyboardEvent) {
      const isHuddleShortcut =
        event.ctrlKey &&
        event.shiftKey &&
        !event.metaKey &&
        !event.altKey &&
        event.code === "Space";
      if (isHuddleShortcut) {
        if (event.repeat || event.defaultPrevented || !activeChannelId) return;
        event.preventDefault();
        window.dispatchEvent(
          new CustomEvent<HuddleShortcutDetail>(HUDDLE_SHORTCUT_EVENT, {
            detail: { channelId: activeChannelId },
          }),
        );
        return;
      }

      if (
        !hasPrimaryShortcutModifier(event) ||
        event.altKey ||
        event.repeat ||
        event.defaultPrevented
      ) {
        return;
      }

      const key = event.key.toLowerCase();
      if (key === "f" && !event.shiftKey && canSearchCurrentChannel) {
        event.preventDefault();
        onSearchCurrentChannel();
        return;
      }

      if (key === "k" && !event.shiftKey) {
        event.preventDefault();
        onSearchEverything();
        return;
      }

      if (key === "k" && event.shiftKey) {
        event.preventDefault();
        void onNewMessage();
        return;
      }

      if (key === "n" && event.shiftKey) {
        event.preventDefault();
        onCreateChannel();
        return;
      }

      if (key === "o" && event.shiftKey) {
        event.preventDefault();
        onBrowseChannels();
        return;
      }

      if (key === "a" && event.shiftKey) {
        event.preventDefault();
        void onGoHome();
      }
    }

    window.addEventListener("keydown", handleKeyDown, { capture: true });
    return () =>
      window.removeEventListener("keydown", handleKeyDown, { capture: true });
  }, [
    activeChannelId,
    canSearchCurrentChannel,
    disabled,
    onBrowseChannels,
    onCreateChannel,
    onGoHome,
    onNewMessage,
    onSearchCurrentChannel,
    onSearchEverything,
  ]);
}
