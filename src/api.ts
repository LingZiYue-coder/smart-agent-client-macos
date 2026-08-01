/**
 * Tauri command 封装。
 *
 * 品牌锁定铁律：前端永不接触站点地址 —— 所有请求经 Rust 侧编译期常量发出，
 * 本文件的任何函数都不存在 baseUrl / 站点地址参数。
 */
import { invoke } from "@tauri-apps/api/core";

export interface FrontState {
  wizard_done: boolean;
  has_pat: boolean;
  has_key: boolean;
  /** 用户是否希望保持 Codex 接入（断开后为 false，禁止自动回写） */
  codex_desired: boolean;
  username: string | null;
  user_id: number | null;
  token_name: string | null;
}

export interface LoginOutcome {
  require_2fa: boolean;
  flow_token: string | null;
  user_id: number | null;
  username: string | null;
}

export interface PatResult {
  reused: boolean;
  generated: boolean;
}

export interface SelfInfo {
  user_id: number;
  username: string;
  display_name: string;
  email: string;
  quota: number;
  used_quota: number;
  request_count: number;
}

export interface ClientConfig {
  default_model: string;
  models: string[];
  min_client_version: string;
  latest_version?: string;
  download_url?: string;
  website_url?: string;
  announcement: string;
  user_agreement: string;
  privacy_policy: string;
}

export interface UpdateCheckResult {
  current_version: string;
  min_client_version: string;
  latest_version: string;
  force_update: boolean;
  soft_update: boolean;
  download_url: string;
}

export interface ModelSettings {
  available_models: string[];
  /** 当前唯一生效模型（写入 Codex config.model） */
  active_model: string;
  synced_to_codex: boolean;
}

export interface DeviceStatus {
  enabled: boolean;
  status: number | null;
  auth_expired: boolean;
}

export interface UsageSummary {
  today_quota: number;
  today_requests: number;
  today_tokens: number;
}

export interface TopUpMethod {
  id: string;
  label: string;
  min_amount: number;
}

export interface TopUpInfo {
  online_enabled: boolean;
  redemption_enabled: boolean;
  min_amount: number;
  amount_options: number[];
  methods: TopUpMethod[];
}

export interface TopUpRecord {
  id: number;
  amount: number;
  money: number;
  trade_no: string;
  payment_method: string;
  create_time: number;
  complete_time: number;
  status: string;
}

export interface TopUpPage {
  page: number;
  page_size: number;
  total: number;
  items: TopUpRecord[];
}

export interface InviteInfo {
  aff_code: string;
  inviter_id: number;
  aff_count: number;
  aff_quota: number;
  aff_history_quota: number;
  inviter_reward: number;
  invitee_reward: number;
  enabled: boolean;
}

export interface InviteClaimResult {
  rewarded: boolean;
  already_done: boolean;
  inviter_quota: number;
  invitee_quota: number;
  status: string;
  message: string;
}

export interface OpenPlatformOverview {
  enabled: boolean;
  id_enabled: boolean;
  id_unlocked: boolean;
  unlock_label: string;
  unlock_hint: string;
  key_enabled: boolean;
  docs_url: string;
  one_models: string[];
  last_sync_time?: string;
  id_count: number;
}

export interface OpenIdItem {
  id: string;
  label: string;
  masked_username: string;
  region: string;
  status: string;
  last_check: string;
}

export interface ProvisionResult {
  token_id: number;
  token_name: string;
  masked_key: string;
  created: boolean;
}

export interface CodexEnv {
  installed: boolean;
  desktop_installed: boolean;
  cli_path: string | null;
  version: string | null;
  npm_global: boolean;
  codex_home: string;
  config_exists: boolean;
  auth_exists: boolean;
  credentials_store: string | null;
  keyring_warning: boolean;
  has_chatgpt_login: boolean;
  provider_configured: boolean;
  config_parse_error: string | null;
}

export interface CodexInstallResult {
  ok: boolean;
  message: string;
}

export interface ApplyPreview {
  config_preview: string;
  auth_preview: string;
  has_chatgpt_login: boolean;
  keyring_warning: boolean;
  codex_home: string;
}

export interface ApplyResult {
  backup_dir: string | null;
  chatgpt_login_overwritten: boolean;
  official_login_preserved: boolean;
  desktop_model_picker_ready: boolean;
  cc_switch_synced: boolean;
  codex_restarted: boolean;
  keyring_warning: boolean;
}

export interface OfficialLoginRestoreResult {
  official_login_restored: boolean;
  login_required: boolean;
}

export interface ConnectivityResult {
  ok: boolean;
  latency_ms: number;
  http_status: number;
  model: string;
  message: string;
}

/** GET /api/status 站点能力探测结果（只挑客户端关心的字段）。 */
export interface SiteStatus {
  system_name?: string;
  version?: string;
  register_enabled?: boolean;
  password_login_enabled?: boolean;
  password_register_enabled?: boolean;
  email_verification?: boolean;
  turnstile_check?: boolean;
  quota_per_unit?: number;
  [key: string]: unknown;
}

/** quota → 美元（500,000 quota = $1，以站点 quota_per_unit 为准）。 */
export function quotaToUsd(quota: number, quotaPerUnit?: number): string {
  const unit = quotaPerUnit && quotaPerUnit > 0 ? quotaPerUnit : 500000;
  return (quota / unit).toFixed(2);
}

export const api = {
  // —— 站点 / 账号 ——
  getStatus: () => invoke<SiteStatus>("get_status"),
  getClientConfig: () => invoke<ClientConfig>("get_client_config"),
  getAppVersion: () => invoke<string>("get_app_version"),
  openExternalUrl: (url: string) => invoke<void>("open_external_url", { url }),
  checkClientUpdate: (config: ClientConfig) =>
    invoke<UpdateCheckResult>("check_client_update", { config }),
  getModelSettings: () => invoke<ModelSettings>("get_model_settings"),
  saveModelSettings: (activeModel: string) =>
    invoke<ModelSettings>("save_model_settings", {
      activeModel,
    }),
  register: (username: string, password: string, inviteCode?: string) =>
    invoke<void>("register_account", { username, password, inviteCode }),
  login: (username: string, password: string) =>
    invoke<LoginOutcome>("login", { username, password }),
  login2fa: (flowToken: string, code: string) =>
    invoke<LoginOutcome>("login_2fa", { flowToken, code }),
  ensurePat: () => invoke<PatResult>("ensure_pat"),
  getSelf: () => invoke<SelfInfo>("get_self"),
  updateProfile: (displayName: string) =>
    invoke<void>("update_profile", { displayName }),
  changePassword: (originalPassword: string, password: string) =>
    invoke<void>("change_password", { originalPassword, password }),
  getUsage: () => invoke<UsageSummary>("get_usage"),
  getTopupInfo: () => invoke<TopUpInfo>("get_topup_info"),
  getTopups: () => invoke<TopUpPage>("get_topups"),
  getInviteInfo: () => invoke<InviteInfo>("get_invite_info"),
  claimInviteReward: () => invoke<InviteClaimResult>("claim_invite_reward"),
  redeemTopup: (key: string) => invoke<number>("redeem_topup", { key }),
  startTopupCheckout: (amount: number, paymentMethod: string) =>
    invoke<void>("start_topup_checkout", { amount, paymentMethod }),
  getOpenPlatformOverview: () =>
    invoke<OpenPlatformOverview>("get_open_platform_overview"),
  getOpenIdItems: () => invoke<OpenIdItem[]>("get_open_id_items"),
  revealOpenIdItem: (itemId: string, field: "username" | "password") =>
    invoke<string>("reveal_open_id_item", { itemId, field }),

  // —— 令牌 ——
  listTokens: () => invoke<unknown[]>("list_tokens"),
  createToken: (name: string) => invoke<void>("create_token", { name }),
  fetchTokenKey: (tokenId: number) =>
    invoke<string>("fetch_token_key", { tokenId }),
  autoProvision: () => invoke<ProvisionResult>("auto_provision"),
  checkDeviceStatus: () => invoke<DeviceStatus>("check_device_status"),

  // —— Codex 接入 ——
  detectCodex: () => invoke<CodexEnv>("detect_codex"),
  installCodexDesktop: () =>
    invoke<CodexInstallResult>("install_codex_desktop"),
  planCodexConfig: () => invoke<ApplyPreview>("plan_codex_config"),
  applyCodexConfig: () => invoke<ApplyResult>("apply_codex_config"),
  ensureCodexConfig: () => invoke<boolean>("ensure_codex_config"),
  disconnectCodex: () => invoke<void>("disconnect_codex"),
  restoreOfficialCodexLogin: () =>
    invoke<OfficialLoginRestoreResult>("restore_official_codex_login"),
  codexDesktopRunning: () => invoke<boolean>("codex_desktop_running"),
  testConnection: () => invoke<ConnectivityResult>("test_connection"),

  // —— 本地状态 ——
  getLocalState: () => invoke<FrontState>("get_local_state"),
  setWizardDone: (done: boolean) => invoke<void>("set_wizard_done", { done }),
  logout: () => invoke<void>("logout"),
};
