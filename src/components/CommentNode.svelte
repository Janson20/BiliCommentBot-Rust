<script>
  export let comment;
  export let depth = 0;

  const indent = Math.min(depth, 6) * 20;
</script>

<div class="comment-node" style="margin-left: {indent}px;">
  <div class="comment-meta">
    <span class="comment-user">{comment.user}</span>
    <span class="comment-time">{comment.timestamp}</span>
    {#if comment.video_title || comment.bvid}
      <span class="video-tag" title={comment.video_title || comment.bvid}>
        {comment.video_title || comment.bvid}
      </span>
    {/if}
    {#if depth > 0}
      <span class="depth-badge">L{depth}</span>
    {/if}
  </div>
  <div class="comment-bubble original">
    <span class="label">原评论</span>
    <div class="text">{comment.content}</div>
  </div>
  {#if comment.reply_content}
    <div class="comment-bubble reply">
      <span class="label">🤖 AI回复</span>
      <div class="text">{comment.reply_content}</div>
    </div>
  {/if}
  {#each comment.children || [] as child}
    <svelte:self comment={child} depth={depth + 1} />
  {/each}
</div>

<style>
  .comment-node { margin-bottom: 4px; }
  .comment-meta {
    display: flex; align-items: center; gap: 8px; margin-bottom: 4px;
  }
  .comment-user { color: #00b4d8; font-size: 0.82rem; font-weight: 600; }
  .comment-time { color: #5a7a9a; font-size: 0.72rem; }
  .video-tag {
    font-size: 0.68rem; color: #8aa0b8; background: #0d1b2a;
    padding: 1px 8px; border-radius: 4px; max-width: 180px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .depth-badge {
    font-size: 0.65rem; background: #1e3a5f; color: #8aa0b8;
    padding: 1px 6px; border-radius: 8px;
  }
  .comment-bubble {
    margin: 3px 0 6px 0; padding: 8px 12px; border-radius: 8px;
    font-size: 0.8rem; line-height: 1.5;
  }
  .comment-bubble.original { background: #1e3a5f; border-left: 3px solid #5a7a9a; }
  .comment-bubble.reply { background: #0d3320; border-left: 3px solid #00b4d8; }
  .comment-bubble .label { font-size: 0.68rem; color: #8aa0b8; display: block; margin-bottom: 3px; }
  .comment-bubble .text { color: #c0d0e0; word-break: break-all; }
</style>
