// 首次使用（配置向导）判定逻辑 —— 独立成模块便于复用与单测

/**
 * AI 是否已配置完成。
 * 必须区分提供商，否则选了 DeepSeek 的用户会被误判为「未配置」而反复看到向导：
 * - deepseek：api_key 默认为空，需用户填写
 * - ollama：base_url / model 有非空默认值，只判断是否为空
 */
export function isAiConfigured(cfg) {
  const provider = String(cfg?.ai?.provider || "deepseek").toLowerCase();
  if (provider === "ollama") {
    return !!String(cfg?.ollama?.base_url || "").trim()
      && !!String(cfg?.ollama?.model || "").trim();
  }
  return !!String(cfg?.deepseek?.api_key || "").trim();
}

/** B 站登录信息是否已写入配置（扫码登录会写入，手动 Cookie 登录由后端同步写入） */
export function isLoggedIn(cfg) {
  return !!String(cfg?.bilibili?.cookie || "").trim()
    && !!String(cfg?.bilibili?.uid || "").trim();
}

/** 是否需要展示首次配置向导 */
export function needsSetupWizard(cfg, liveLoginValid = false) {
  if (cfg?.app?.setup_complete === true) return false;
  if (isLoggedIn(cfg) && isAiConfigured(cfg)) return false;
  // 配置里没有登录信息时，用实时校验兜底（Cookie 可能只存在于 cookie 文件）
  if (isAiConfigured(cfg) && liveLoginValid) return false;
  return true;
}
