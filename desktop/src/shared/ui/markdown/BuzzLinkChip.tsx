import * as React from "react";
import type { LucideIcon } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { copyTextToClipboard } from "@/shared/lib/clipboard";
import {
  MENTION_CHIP_BASE_CLASSES,
  MENTION_CHIP_HOVER_CLASSES,
} from "@/shared/ui/mentionChip";

import {
  MediaContextMenu,
  type MediaContextMenuPosition,
  useDismissMediaContextMenu,
} from "./MediaContextMenu";

export const BUZZ_LINK_CHIP_CLASSES = cn(
  "cursor-pointer",
  MENTION_CHIP_BASE_CLASSES,
  MENTION_CHIP_HOVER_CLASSES,
);

function useBuzzLinkContextMenu({
  href,
  interactive,
  onOpenLink,
}: {
  href: string | undefined;
  interactive: boolean;
  onOpenLink: () => void;
}) {
  const [position, setPosition] =
    React.useState<MediaContextMenuPosition | null>(null);
  const closeMenu = React.useCallback(() => setPosition(null), []);
  useDismissMediaContextMenu(Boolean(position), closeMenu);

  const onContextMenuCapture = React.useCallback(
    (event: React.MouseEvent<HTMLElement>) => {
      if (!interactive || !href) return;
      event.preventDefault();
      setPosition({ x: event.clientX, y: event.clientY });
    },
    [href, interactive],
  );

  const contextMenu =
    position && href ? (
      <MediaContextMenu
        dataAttributes={["data-buzz-link-context-menu"]}
        items={[
          {
            label: "Open link",
            onSelect: () => {
              closeMenu();
              onOpenLink();
            },
          },
          {
            label: "Copy link",
            onSelect: () => {
              closeMenu();
              copyTextToClipboard(href, "Link copied to clipboard");
            },
          },
        ]}
        position={position}
      />
    ) : null;

  return { contextMenu, onContextMenuCapture };
}

export function BuzzLinkChip({
  children,
  className,
  href,
  icon: Icon,
  interactive,
  onOpenLink,
  ...props
}: Omit<React.ComponentPropsWithoutRef<"button">, "onClick"> & {
  href?: string;
  icon: LucideIcon;
  interactive: boolean;
  onOpenLink: () => void;
}) {
  const { contextMenu, onContextMenuCapture } = useBuzzLinkContextMenu({
    href,
    interactive,
    onOpenLink,
  });
  const content = (
    <>
      <Icon aria-hidden="true" className="mention-chip-icon" />
      {children}
    </>
  );

  if (!interactive) {
    return (
      <span
        data-buzz-link=""
        className={cn(MENTION_CHIP_BASE_CLASSES, className)}
        {...(props as React.HTMLAttributes<HTMLSpanElement>)}
      >
        {content}
      </span>
    );
  }

  return (
    <>
      <button
        {...props}
        type="button"
        data-buzz-link=""
        className={cn(BUZZ_LINK_CHIP_CLASSES, className)}
        onClick={onOpenLink}
        onContextMenuCapture={onContextMenuCapture}
      >
        {content}
      </button>
      {contextMenu}
    </>
  );
}

export function BuzzInlineLink({
  children,
  href,
  interactive,
  onOpenLink,
  ...props
}: Omit<React.ComponentPropsWithoutRef<"button">, "onClick"> & {
  href?: string;
  interactive: boolean;
  onOpenLink: () => void;
}) {
  const contextMenuHref =
    href ?? (typeof props.title === "string" ? props.title : undefined);
  const { contextMenu, onContextMenuCapture } = useBuzzLinkContextMenu({
    href: contextMenuHref,
    interactive,
    onOpenLink,
  });

  if (!interactive) {
    return <span className="font-medium text-current">{children}</span>;
  }

  return (
    <>
      <button
        {...props}
        type="button"
        className="cursor-pointer font-medium text-primary underline underline-offset-4 transition-colors hover:text-primary/80"
        onClick={onOpenLink}
        onContextMenuCapture={onContextMenuCapture}
      >
        {children}
      </button>
      {contextMenu}
    </>
  );
}
