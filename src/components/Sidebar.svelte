<script>
  import { isRunning } from "../lib/stores.js";
  import { currentRoute, navigate } from "../lib/router.js";

  const navItems = [
    { path: "/", label: "仪表盘", icon: "📊" },
    { path: "/login", label: "扫码登录", icon: "🔑" },
    { path: "/config", label: "配置编辑", icon: "⚙️" },
    { path: "/logs", label: "日志查看", icon: "📜" },
    { path: "/history", label: "回复历史", icon: "📋" },
    { path: "/settings", label: "系统设置", icon: "🛠" },
  ];
</script>

<aside class="sidebar">
  <div class="logo">
    <span class="logo-icon">🤖</span>
    <span class="logo-text">BiliBot-RS</span>
  </div>
  <nav>
    {#each navItems as item}
      <button
        class="nav-btn"
        class:active={$currentRoute === item.path}
        on:click={() => { navigate(item.path); }}
      >
        <span class="nav-icon">{item.icon}</span>
        <span class="nav-label">{item.label}</span>
      </button>
    {/each}
  </nav>
  <div class="sidebar-footer">
    <div class="status-dot" class:running={$isRunning}></div>
    <span class="status-text">{$isRunning ? "运行中" : "已停止"}</span>
  </div>
</aside>

<style>
  .sidebar {
    width: 200px;
    height: 100vh;
    background: linear-gradient(180deg, #16213e 0%, #0f3460 100%);
    display: flex;
    flex-direction: column;
    padding: 0;
    flex-shrink: 0;
  }
  .logo {
    padding: 18px 16px;
    display: flex;
    align-items: center;
    gap: 8px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  }
  .logo-icon { font-size: 1.5rem; }
  .logo-text {
    font-size: 1.05rem;
    font-weight: 700;
    color: #00b4d8;
    letter-spacing: 0.5px;
  }
  nav {
    flex: 1;
    padding: 10px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .nav-btn {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: #b0c4de;
    cursor: pointer;
    font-size: 0.9rem;
    transition: all 0.15s;
    text-align: left;
    width: 100%;
  }
  .nav-btn:hover { background: rgba(255, 255, 255, 0.06); color: #e0e8f0; }
  .nav-btn.active { background: rgba(0, 180, 216, 0.15); color: #00b4d8; font-weight: 600; }
  .nav-icon { font-size: 1.1rem; width: 24px; text-align: center; }
  .sidebar-footer {
    padding: 14px 16px;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .status-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #e74c3c;
    box-shadow: 0 0 6px rgba(231, 76, 60, 0.5);
  }
  .status-dot.running {
    background: #2ecc71;
    box-shadow: 0 0 6px rgba(46, 204, 113, 0.5);
  }
  .status-text { font-size: 0.8rem; color: #8aa0b8; }
</style>
