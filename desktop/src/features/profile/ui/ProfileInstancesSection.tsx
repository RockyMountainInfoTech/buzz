import * as React from "react";
import { ChevronRight } from "lucide-react";

import { ProfileSectionGroup } from "@/features/profile/ui/UserProfilePanelFields";
import type { ManagedAgent } from "@/shared/api/types";
import { cn } from "@/shared/lib/cn";

function ProfileInstanceRow({
  instance,
  currentPubkey,
  onOpenInstance,
  archived = false,
}: {
  instance: ManagedAgent;
  currentPubkey: string | null;
  onOpenInstance: (pubkey: string) => void;
  archived?: boolean;
}) {
  const isCurrent = instance.pubkey === currentPubkey;
  return (
    <button
      className="flex min-h-16 w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-muted/40 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
      data-testid={`user-profile-instance-${instance.pubkey}`}
      onClick={() => onOpenInstance(instance.pubkey)}
      type="button"
    >
      <span className="min-w-0 flex-1 truncate text-sm font-medium">
        {instance.name}
      </span>
      <span className="text-xs capitalize text-muted-foreground">
        {archived
          ? "Archived"
          : isCurrent
            ? "Current"
            : instance.status.replace("_", " ")}
      </span>
      <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />
    </button>
  );
}

/**
 * The persona's managed-agent instances, split into live rows and a labeled
 * "Archived" subsection. Archived rows keep the explicit-pubkey click so
 * unarchive stays UI-reachable for channel-less agents; live and archived rows
 * are otherwise identical. The count reflects both buckets, and the section is
 * expected to render only when at least one instance (live or archived) exists.
 */
export function ProfileInstancesSection({
  currentPubkey,
  instances,
  archivedInstances,
  onOpenInstance,
}: {
  currentPubkey: string | null;
  instances: ManagedAgent[];
  archivedInstances: ManagedAgent[];
  onOpenInstance: (pubkey: string) => void;
}) {
  const [expanded, setExpanded] = React.useState(false);
  const totalCount = instances.length + archivedInstances.length;
  if (totalCount === 0) return null;
  const instanceCountLabel = `${totalCount} instance${totalCount === 1 ? "" : "s"}`;

  return (
    <ProfileSectionGroup
      testId="user-profile-instances-section"
      title="Instances"
    >
      <button
        aria-expanded={expanded}
        className="flex min-h-16 w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-muted/40 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
        data-testid="user-profile-instances"
        onClick={() => setExpanded((value) => !value)}
        type="button"
      >
        <span className="min-w-0 flex-1 text-sm font-medium">
          {instanceCountLabel}
        </span>
        <ChevronRight
          className={cn(
            "h-4 w-4 shrink-0 text-muted-foreground transition-transform",
            expanded && "rotate-90",
          )}
        />
      </button>
      {expanded ? (
        <>
          {instances.map((instance) => (
            <ProfileInstanceRow
              currentPubkey={currentPubkey}
              instance={instance}
              key={instance.pubkey}
              onOpenInstance={onOpenInstance}
            />
          ))}
          {archivedInstances.length > 0 ? (
            <div data-testid="user-profile-instances-archived">
              <p
                className="px-4 pt-3 pb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground"
                data-testid="user-profile-instances-archived-header"
              >
                Archived
              </p>
              {archivedInstances.map((instance) => (
                <ProfileInstanceRow
                  archived
                  currentPubkey={currentPubkey}
                  instance={instance}
                  key={instance.pubkey}
                  onOpenInstance={onOpenInstance}
                />
              ))}
            </div>
          ) : null}
        </>
      ) : null}
    </ProfileSectionGroup>
  );
}
