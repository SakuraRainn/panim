<!-- The mark of PANIM: two faces of one person. -->
<script lang="ts">
  let { size = 40 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
  <circle cx="38" cy="32" r="16" fill="none" stroke="var(--peri)" stroke-width="3.2" />
  <circle cx="26" cy="32" r="16" fill="none" stroke="var(--peach)" stroke-width="3.2" />
</svg>
