import * as React from "react";

import { Markdown } from "@/shared/ui/markdown";
import type { MarkdownProps } from "@/shared/ui/markdown/types";
import { useOpenVideoReviewAt } from "@/shared/ui/VideoReviewNavigation";
import { parseVideoReviewTimecode } from "@/shared/ui/videoReviewTimecode";
import {
  VideoReviewTimecodeButton,
  VideoReviewTimecodeChip,
} from "@/shared/ui/VideoReviewTimecodeButton";

type VideoReviewCommentMarkdownProps = Omit<
  MarkdownProps,
  "leadingInlineContent"
> & {
  videoReviewCommentRootId?: string;
};

/** Renders a video-review timecode inside the comment's first Markdown line. */
export function VideoReviewCommentMarkdown({
  content,
  interactive = true,
  videoReviewCommentRootId,
  ...markdownProps
}: VideoReviewCommentMarkdownProps) {
  const openVideoReviewAt = useOpenVideoReviewAt();
  const reviewTimecode = videoReviewCommentRootId
    ? parseVideoReviewTimecode(content)
    : null;
  const handleTimecodeClick = React.useCallback(
    (event: React.MouseEvent<HTMLButtonElement>) => {
      event.stopPropagation();
      if (reviewTimecode && videoReviewCommentRootId) {
        openVideoReviewAt?.(videoReviewCommentRootId, reviewTimecode.seconds);
      }
    },
    [openVideoReviewAt, reviewTimecode, videoReviewCommentRootId],
  );

  if (!reviewTimecode) {
    return (
      <Markdown
        {...markdownProps}
        content={content}
        interactive={interactive}
      />
    );
  }

  const timecode =
    interactive && openVideoReviewAt ? (
      <VideoReviewTimecodeButton
        surface="message"
        timecode={reviewTimecode.timecode}
        onClick={handleTimecodeClick}
      />
    ) : (
      <VideoReviewTimecodeChip
        surface="message"
        timecode={reviewTimecode.timecode}
      />
    );

  return (
    <Markdown
      {...markdownProps}
      content={reviewTimecode.text || "\u200B"}
      interactive={interactive}
      leadingInlineContent={<>{timecode} </>}
    />
  );
}
