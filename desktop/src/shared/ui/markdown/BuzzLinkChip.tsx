import type * as React from "react";
import type { LucideIcon } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import {
  MENTION_CHIP_BASE_CLASSES,
  MENTION_CHIP_HOVER_CLASSES,
} from "@/shared/ui/mentionChip";

export const BUZZ_LINK_CHIP_CLASSES = cn(
  "cursor-pointer",
  MENTION_CHIP_BASE_CLASSES,
  MENTION_CHIP_HOVER_CLASSES,
);

export function BuzzLinkChip({
  children,
  className,
  icon: Icon,
  interactive,
  onClick,
  ...props
}: React.ComponentPropsWithoutRef<"button"> & {
  icon: LucideIcon;
  interactive: boolean;
}) {
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
    <button
      type="button"
      data-buzz-link=""
      className={cn(BUZZ_LINK_CHIP_CLASSES, className)}
      onClick={onClick}
      {...props}
    >
      {content}
    </button>
  );
}

export function BuzzInlineLink({
  children,
  interactive,
  onClick,
  ...props
}: React.ComponentPropsWithoutRef<"button"> & { interactive: boolean }) {
  if (!interactive) {
    return <span className="font-medium text-current">{children}</span>;
  }

  return (
    <button
      type="button"
      className="cursor-pointer font-medium text-primary underline underline-offset-4 transition-colors hover:text-primary/80"
      onClick={onClick}
      {...props}
    >
      {children}
    </button>
  );
}
