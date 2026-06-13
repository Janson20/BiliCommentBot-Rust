<script>
  import { toast } from "../lib/stores.js";
</script>

{#if $toast}
  <div class="toast toast-{$toast.type}">
    <span>{$toast.text}</span>
    <button on:click={() => toast.set(null)}>&times;</button>
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    bottom: 24px;
    right: 24px;
    z-index: 9999;
    padding: 12px 18px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 0.85rem;
    box-shadow: 0 4px 16px rgba(0,0,0,0.3);
    animation: slideIn 0.25s ease;
  }
  .toast-info { background: #1e88e5; color: #fff; }
  .toast-error { background: #e53935; color: #fff; }
  .toast-success { background: #43a047; color: #fff; }
  .toast button {
    background: none; border: none; color: #fff; font-size: 1.2rem;
    cursor: pointer; opacity: 0.7;
  }
  .toast button:hover { opacity: 1; }
  @keyframes slideIn {
    from { transform: translateY(20px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }
</style>
