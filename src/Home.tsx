import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import {
  Alert,
  App as AntApp,
  Button,
  Descriptions,
  Divider,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Segmented,
  Select,
  Skeleton,
  Tag,
  Typography,
} from "antd";
import {
  CloudSyncOutlined,
  DesktopOutlined,
  HistoryOutlined,
  LockOutlined,
  LogoutOutlined,
  PoweroffOutlined,
  ReloadOutlined,
  RightOutlined,
  SafetyCertificateOutlined,
  SettingOutlined,
  UserOutlined,
  WalletOutlined,
  GiftOutlined,
  CopyOutlined,
  ApiOutlined,
  DatabaseOutlined,
  KeyOutlined,
  LinkOutlined,
  SyncOutlined,
  UnlockOutlined,
  ArrowLeftOutlined,
  CheckCircleOutlined,
} from "@ant-design/icons";
import BrandOrb from "./BrandOrb";
import WindowControls from "./WindowControls";
import {
  api,
  ClientConfig,
  CodexEnv,
  FrontState,
  InviteInfo,
  ModelSettings,
  OpenIdItem,
  OpenPlatformOverview,
  quotaToUsd,
  SelfInfo,
  SiteStatus,
  TopUpInfo,
  TopUpMethod,
  TopUpRecord,
  UsageSummary,
} from "./api";

const { Text, Title } = Typography;

interface HomeProps {
  onRerunWizard: () => void;
  onLoggedOut: () => void;
}

const fallbackAmounts = [10, 20, 50, 100, 200];

type ClientModule =
  | "home"
  | "platform"
  | "apple-id"
  | "api-keys"
  | "profile"
  | "security"
  | "records"
  | "invite";

function Home({ onRerunWizard, onLoggedOut }: HomeProps) {
  const { message, modal } = AntApp.useApp();
  const [local, setLocal] = useState<FrontState | null>(null);
  const [self, setSelf] = useState<SelfInfo | null>(null);
  const [usage, setUsage] = useState<UsageSummary | null>(null);
  const [env, setEnv] = useState<CodexEnv | null>(null);
  const [siteStatus, setSiteStatus] = useState<SiteStatus | null>(null);
  const [clientConfig, setClientConfig] = useState<ClientConfig | null>(null);
  const [modelSettings, setModelSettings] = useState<ModelSettings | null>(null);
  const [activeModel, setActiveModel] = useState("");
  const [savingModels, setSavingModels] = useState(false);
  const [topupInfo, setTopupInfo] = useState<TopUpInfo | null>(null);
  const [topups, setTopups] = useState<TopUpRecord[]>([]);
  const [inviteInfo, setInviteInfo] = useState<InviteInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [starting, setStarting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [progress, setProgress] = useState("");
  const [locked, setLocked] = useState(false);
  const [moduleView, setModuleView] = useState<ClientModule>("home");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [restoringOfficialLogin, setRestoringOfficialLogin] = useState(false);
  const [selectedAmount, setSelectedAmount] = useState<number>(50);
  const [customAmount, setCustomAmount] = useState<number | null>(null);
  const [paymentOpen, setPaymentOpen] = useState(false);
  const [paymentLoading, setPaymentLoading] = useState(false);
  const [redeemOpen, setRedeemOpen] = useState(false);
  const [platformLoading, setPlatformLoading] = useState(false);
  const [platformOverview, setPlatformOverview] =
    useState<OpenPlatformOverview | null>(null);
  const [openIdItems, setOpenIdItems] = useState<OpenIdItem[]>([]);
  const [platformTokens, setPlatformTokens] = useState<any[]>([]);
  const [visibleKey, setVisibleKey] = useState("");
  const [syncingIds, setSyncingIds] = useState(false);
  const [platformErrors, setPlatformErrors] = useState({
    overview: "",
    ids: "",
    tokens: "",
  });

  const handleAuthExpired = useCallback(async () => {
    await api.logout().catch(() => undefined);
    message.warning("登录状态已失效，请重新登录");
    onLoggedOut();
  }, [message, onLoggedOut]);

  const checkDevice = useCallback(async () => {
    const current = await api.getLocalState().catch(() => null);
    if (!current?.has_key) return;
    // 仅在用户仍希望保持接入时，才对抗 Desktop 洗配置。
    // 用户点过「断开」后 codex_desired=false，绝不静默重连。
    if (current.codex_desired) {
      const restored = await api.ensureCodexConfig().catch(() => false);
      if (restored) {
        const nextEnv = await api.detectCodex().catch(() => null);
        if (nextEnv) setEnv(nextEnv);
      }
    }
    const result = await api.checkDeviceStatus().catch(() => null);
    if (!result) return;
    if (result.auth_expired) {
      await handleAuthExpired();
      return;
    }
    setLocked(!result.enabled);
  }, [handleAuthExpired]);

  const refresh = useCallback(async () => {
    setLoading(true);
    const [nextLocal, nextEnv, nextStatus, nextConfig] = await Promise.all([
      api.getLocalState().catch(() => null),
      api.detectCodex().catch(() => null),
      api.getStatus().catch(() => null),
      api.getClientConfig().catch(() => null),
    ]);
    setEnv(nextEnv);
    setSiteStatus(nextStatus);
    setClientConfig(nextConfig);

    const [nextSelf, nextUsage, nextTopupInfo, nextTopups, nextModels, nextInvite] =
      await Promise.all([
      api.getSelf().catch(() => null),
      api.getUsage().catch(() => null),
      api.getTopupInfo().catch(() => null),
      api.getTopups().catch(() => null),
      api.getModelSettings().catch(() => null),
      api.getInviteInfo().catch(() => null),
    ]);
    setSelf(nextSelf);
    setUsage(nextUsage);
    setTopupInfo(nextTopupInfo);
    setTopups(nextTopups?.items ?? []);
    setInviteInfo(nextInvite);
    setModelSettings(nextModels);

    // 每次登录首页都确认一次账户级客户端密钥。这样旧版客户端升级后，
    // 即使本机已经保存过设备密钥，也会自动切换到当前账户统一使用的密钥。
    let resolvedLocal = nextLocal;
    if (nextSelf) {
      const provision = await api.autoProvision().catch(() => null);
      if (provision) {
        resolvedLocal = await api.getLocalState().catch(() => nextLocal);
        if (resolvedLocal?.codex_desired) {
          await api.ensureCodexConfig().catch(() => false);
        }
      }
    }
    setLocal(resolvedLocal);

    if (nextModels) {
      setActiveModel(nextModels.active_model);
    }
    if (nextTopupInfo?.amount_options?.length) {
      setSelectedAmount(
        nextTopupInfo.amount_options.includes(50)
          ? 50
          : nextTopupInfo.amount_options[0],
      );
    }

    // 兼容旧版本客户端：被邀请人可能已经完成连接，但当时没有成功提交奖励领取。
    // 服务端按设备与被邀请账号幂等处理，因此这里可以在首页刷新时安全补偿一次。
    if (resolvedLocal?.has_key && nextInvite?.enabled && nextInvite.inviter_id > 0) {
      const claim = await api.claimInviteReward().catch(() => null);
      if (claim?.rewarded) {
        message.success(claim.message || "邀请奖励已到账");
        const refreshedInvite = await api.getInviteInfo().catch(() => null);
        if (refreshedInvite) setInviteInfo(refreshedInvite);
      }
    }
    setLoading(false);
  }, [message]);

  useEffect(() => {
    void refresh();
    void checkDevice();
    // 设备状态 / 可选的配置回写：30 秒一次足够；断开后 ensure 为 no-op
    const interval = window.setInterval(() => void checkDevice(), 30_000);
    const onVisibility = () => {
      if (document.visibilityState === "visible") void checkDevice();
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.clearInterval(interval);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [checkDevice, refresh]);

  const start = async () => {
    setStarting(true);
    try {
      if (!local?.has_key) {
        setProgress("正在准备账户…");
        await api.autoProvision();
      }
      setProgress("正在接入…");
      const applyResult = await api.applyCodexConfig();
      setProgress("正在验证…");
      const result = await api.testConnection();
      if (!result.ok) throw new Error(result.message);
      const claim = await api.claimInviteReward().catch(() => null);
      if (claim?.rewarded) {
        message.success(claim.message || "邀请奖励已到账");
      }
      if (applyResult.desktop_model_picker_ready) {
        message.success("连接成功。请重新打开 Codex 后再使用。");
      } else {
        message.warning(
          "连接已完成。建议先在 Codex 登录官方账号，再重新打开 Codex。",
        );
      }
      await refresh();
    } catch (error) {
      message.error(String(error));
    } finally {
      setStarting(false);
      setProgress("");
    }
  };

  const stop = () => {
    modal.confirm({
      title: "断开 AI 服务？",
      content: "断开后 Codex 将暂时无法使用 AI 服务，之后可以重新一键接入。",
      okText: "确认断开",
      cancelText: "取消",
      onOk: async () => {
        setStopping(true);
        try {
          await api.disconnectCodex();
          message.success("AI 服务已断开");
          await refresh();
        } catch (error) {
          message.error(String(error));
        } finally {
          setStopping(false);
        }
      },
    });
  };

  const logout = () => {
    modal.confirm({
      title: "退出登录？",
      content: "本设备会退出当前账号。",
      okText: "退出",
      cancelText: "取消",
      onOk: async () => {
        await api.logout();
        onLoggedOut();
      },
    });
  };

  const amountOptions = useMemo(
    () => topupInfo?.amount_options?.slice(0, 5) ?? fallbackAmounts,
    [topupInfo],
  );
  const rechargeAmount = customAmount ?? selectedAmount;

  const requestRecharge = () => {
    if (!topupInfo?.online_enabled || !topupInfo.methods.length) {
      message.info("在线充值暂未开放，可使用兑换码充值");
      return;
    }
    if (rechargeAmount < topupInfo.min_amount) {
      message.warning(`最低充值金额为 $${topupInfo.min_amount}`);
      return;
    }
    setPaymentOpen(true);
  };

  const loadOpenPlatform = useCallback(async () => {
    setPlatformLoading(true);
    setVisibleKey("");
    setPlatformErrors({ overview: "", ids: "", tokens: "" });
    try {
      const [overviewResult, tokenResult] = await Promise.allSettled([
        api.getOpenPlatformOverview(),
        api.listTokens(),
      ]);
      const nextErrors = { overview: "", ids: "", tokens: "" };
      const overview =
        overviewResult.status === "fulfilled" ? overviewResult.value : null;
      setPlatformOverview(overview);
      if (overviewResult.status === "rejected") {
        nextErrors.overview =
          overviewResult.reason instanceof Error
            ? overviewResult.reason.message
            : "开放平台配置读取失败";
      }

      if (tokenResult.status === "fulfilled") {
        setPlatformTokens(
          Array.isArray(tokenResult.value) ? tokenResult.value : [],
        );
      } else {
        setPlatformTokens([]);
        nextErrors.tokens =
          tokenResult.reason instanceof Error
            ? tokenResult.reason.message
            : "API KEY 读取失败";
      }

      if (overview?.id_unlocked) {
        try {
          setOpenIdItems(await api.getOpenIdItems());
        } catch (error) {
          setOpenIdItems([]);
          nextErrors.ids =
            error instanceof Error ? error.message : "Apple ID 数据读取失败";
        }
      } else {
        setOpenIdItems([]);
      }
      setPlatformErrors(nextErrors);
    } finally {
      setPlatformLoading(false);
    }
  }, []);

  const openModule = async (view: ClientModule) => {
    setModuleView(view);
    if (view === "platform" || view === "apple-id" || view === "api-keys") {
      await loadOpenPlatform();
    }
  };

  const refreshIdItems = async () => {
    setSyncingIds(true);
    try {
      const overview = await api.getOpenPlatformOverview();
      setPlatformOverview(overview);
      setOpenIdItems(overview.id_unlocked ? await api.getOpenIdItems() : []);
      setPlatformErrors((current) => ({ ...current, overview: "", ids: "" }));
      message.success("Apple ID 列表已刷新");
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      setOpenIdItems([]);
      setPlatformErrors((current) => ({ ...current, ids: detail }));
      message.error(detail);
    } finally {
      setSyncingIds(false);
    }
  };

  const copyIdField = async (
    item: OpenIdItem,
    field: "username" | "password",
  ) => {
    try {
      const value = await api.revealOpenIdItem(item.id, field);
      await navigator.clipboard.writeText(value);
      message.success(field === "username" ? "账号已复制" : "密码已复制");
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    }
  };

  const saveModels = async () => {
    if (!activeModel.trim()) {
      message.warning("请选择一个模型");
      return;
    }
    setSavingModels(true);
    try {
      const next = await api.saveModelSettings(activeModel.trim());
      setModelSettings(next);
      setActiveModel(next.active_model);
      if (!next.synced_to_codex) {
        message.success("模型已保存，接入后生效");
      } else if (await api.codexDesktopRunning().catch(() => false)) {
        modal.info({
          title: "模型已切换",
          content: "请重新打开 Codex，新模型即可生效。",
          okText: "知道了",
        });
      } else {
        message.success("模型已切换，打开 Codex 即可使用");
      }
    } catch (error) {
      message.error(String(error));
    } finally {
      setSavingModels(false);
    }
  };

  const startCheckout = async (method: TopUpMethod) => {
    if (rechargeAmount < method.min_amount) {
      message.warning(`该方式最低充值金额为 $${method.min_amount}`);
      return;
    }
    setPaymentLoading(true);
    try {
      await api.startTopupCheckout(rechargeAmount, method.id);
      setPaymentOpen(false);
      message.success("安全支付窗口已打开，完成后请刷新余额");
    } catch (error) {
      message.error(String(error));
    } finally {
      setPaymentLoading(false);
    }
  };

  if (locked) {
    return (
      <div className="wizard-shell">
        <header className="app-header wizard-header" data-tauri-drag-region>
          <div className="brand" data-tauri-drag-region>
            <BrandOrb size={30} glowing={false} />
            <div data-tauri-drag-region>
              <strong>Smart Agent</strong>
              <span>设备状态</span>
            </div>
          </div>
          <WindowControls />
        </header>
        <div className="locked-page">
          <div className="locked-panel">
            <SafetyCertificateOutlined />
            <Title level={2}>该设备已被停用</Title>
            <Text type="secondary">如有疑问，请联系服务人员处理。</Text>
            <Button onClick={logout}>退出登录</Button>
          </div>
        </div>
      </div>
    );
  }

  // 以「用户意图 + 磁盘实际配置」共同判定；避免 ensure 竞态下的误显示
  const connected =
    local?.codex_desired === true &&
    env?.provider_configured === true &&
    local?.has_key === true;
  const modelSelectionDirty =
    modelSettings !== null && activeModel !== modelSettings.active_model;
  const quotaPerUnit =
    typeof siteStatus?.quota_per_unit === "number"
      ? siteStatus.quota_per_unit
      : undefined;
  const accountName = self?.display_name || self?.username || "账户";
  const inviteUrl = inviteInfo?.aff_code
    ? `https://www.smart-agent.eu.cc/invite.html?code=${encodeURIComponent(inviteInfo.aff_code)}`
    : "";

  return (
    <div className="app-shell">
      <header className="app-header" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <BrandOrb size={34} glowing={false} />
          <div data-tauri-drag-region>
            <strong>Smart Agent</strong>
            <span>Codex 智能助手</span>
          </div>
        </div>
        <div className="header-actions">
          <Button
            type="text"
            icon={<UserOutlined />}
            onClick={() => void openModule("profile")}
          >
            {accountName}
          </Button>
          <Button
            type="text"
            icon={<SettingOutlined />}
            onClick={() => setSettingsOpen(true)}
          >
            设置
          </Button>
          <Button
            type="text"
            icon={<ReloadOutlined spin={loading} />}
            onClick={() => void refresh()}
            aria-label="刷新"
          />
          <WindowControls />
        </div>
      </header>

      {clientConfig?.announcement && (
        <Alert
          banner
          closable
          className="announcement"
          message={clientConfig.announcement}
        />
      )}

      {moduleView === "home" ? (
      <main className="workspace">
        <section className="relay-pane">
          <div className="section-heading">
            <span className="section-accent" />
            <div>
              <Title level={4}>一键接入 Codex</Title>
              <Text type="secondary">自动完成所有步骤，无需任何设置</Text>
            </div>
          </div>

          <div className="open-quickbar" aria-label="开放平台快捷入口">
            <button type="button" onClick={() => void openModule("platform")}>
              <span className="quickbar-icon">
                <ApiOutlined />
              </span>
              <span>
                <strong>开放平台</strong>
                <small>余额、密钥、接入指南</small>
              </span>
            </button>
            <button type="button" onClick={() => void openModule("apple-id")}>
              <span className="quickbar-icon">
                <DatabaseOutlined />
              </span>
              <span>
                <strong>Apple ID</strong>
                <small>
                  {platformOverview
                    ? platformOverview.id_unlocked
                      ? "已解锁"
                      : platformOverview.unlock_label || "暂未解锁"
                    : "打开后查看"}
                </small>
              </span>
            </button>
            <button type="button" onClick={() => void openModule("api-keys")}>
              <span className="quickbar-icon">
                <KeyOutlined />
              </span>
              <span>
                <strong>API 密钥</strong>
                <small>用于其他开发工具接入</small>
              </span>
            </button>
          </div>

          <div className={`relay-hero ${connected ? "is-online" : ""}`}>
            <div className="status-orbit">
              <BrandOrb size={72} glowing={connected} />
            </div>
            <div className="relay-copy">
              <div className="status-line">
                <Title level={2}>
                  {connected ? "AI 服务连接成功" : "准备接入 Codex"}
                </Title>
                <Tag color={connected ? "success" : "default"}>
                  {connected ? "可以使用" : "尚未连接"}
                </Tag>
              </div>
              <Text type="secondary">
                {connected
                  ? "现在打开 Codex，即可直接开始使用。"
                  : "点击下方按钮，Smart Agent 会自动完成连接。"}
              </Text>
            </div>
          </div>

          <div className="status-metrics">
            <div>
              <CloudSyncOutlined />
              <span>AI 服务</span>
              <strong>{connected ? "连接成功" : "等待连接"}</strong>
            </div>
            <div>
              <SafetyCertificateOutlined />
              <span>账户状态</span>
              <strong>{local?.has_key ? "已登录" : "等待登录"}</strong>
            </div>
            <div>
              <DesktopOutlined />
              <span>Codex</span>
              <strong>{env?.installed ? "已准备" : "需要安装"}</strong>
            </div>
          </div>

          <div className="relay-actions">
            {connected ? (
              <>
                <Button
                  size="large"
                  danger
                  loading={stopping}
                  onClick={stop}
                  icon={<PoweroffOutlined />}
                >
                  断开连接
                </Button>
                <Button
                  size="large"
                  onClick={() => void start()}
                  loading={starting}
                  icon={<ReloadOutlined />}
                >
                  重新连接
                </Button>
              </>
            ) : (
              <Button
                type="primary"
                size="large"
                className="primary-launch"
                loading={starting}
                onClick={() => void start()}
                icon={<PoweroffOutlined />}
              >
                {starting ? progress || "正在连接…" : "一键接入 Codex"}
              </Button>
            )}
          </div>

          <Divider />

          <div className="model-control">
            <div className="subheading">
              <Title level={5}>当前模型</Title>
            </div>
            <div className="model-picker-row">
              <Select
                className="model-select"
                size="middle"
                loading={!modelSettings}
                value={activeModel || undefined}
                showSearch
                optionFilterProp="label"
                placeholder="选择模型"
                popupMatchSelectWidth={false}
                options={(modelSettings?.available_models ?? []).map(
                  (model) => ({
                    label: model,
                    value: model,
                  }),
                )}
                onChange={setActiveModel}
              />
              <Button
                type="primary"
                loading={savingModels}
                disabled={!modelSelectionDirty || !activeModel}
                onClick={() => void saveModels()}
              >
                应用
              </Button>
            </div>
            <Text type="secondary" className="model-hint">
              切换后请重新打开 Codex
            </Text>
          </div>
        </section>

        <aside className="account-pane">
          <section className="balance-section">
            <div className="balance-row">
              <div>
                <Text type="secondary">账户余额</Text>
                <div className="balance-value">
                  <span>$</span>
                  {self ? quotaToUsd(self.quota, quotaPerUnit) : "--"}
                </div>
              </div>
              <div className="today-spend">
                <Text type="secondary">今日消耗</Text>
                <strong>
                  ${usage ? quotaToUsd(usage.today_quota, quotaPerUnit) : "--"}
                </strong>
              </div>
            </div>
            <div className="usage-caption">
              今日 {usage?.today_requests ?? 0} 次请求
              <span>·</span>
              数据刚刚同步
            </div>
          </section>

          <Divider />

          <section className="recharge-section">
            <div className="subheading">
              <Title level={5}>快速充值</Title>
              <Text type="secondary">资金到账后余额自动更新</Text>
            </div>
            <Segmented
              block
              className="amount-segment"
              options={amountOptions.map((amount) => ({
                label: `$${amount}`,
                value: amount,
              }))}
              value={customAmount === null ? selectedAmount : undefined}
              onChange={(value) => {
                setCustomAmount(null);
                setSelectedAmount(Number(value));
              }}
            />
            <InputNumber
              className="custom-amount"
              min={topupInfo?.min_amount ?? 1}
              precision={0}
              value={customAmount}
              onChange={(value) => setCustomAmount(value)}
              prefix="$"
              placeholder="自定义充值金额"
              controls={false}
            />
            <Button
              type="primary"
              size="large"
              block
              icon={<WalletOutlined />}
              disabled={!topupInfo?.online_enabled}
              onClick={requestRecharge}
            >
              {topupInfo?.online_enabled ? "立即充值" : "在线充值暂未开放"}
            </Button>
            {topupInfo?.redemption_enabled && (
              <Button type="link" block onClick={() => setRedeemOpen(true)}>
                使用兑换码充值
              </Button>
            )}
          </section>

          <Divider />

          <section className="account-center">
            <div className="subheading">
              <Title level={5}>账户中心</Title>
              <Text type="secondary">{accountName}</Text>
            </div>
            <button type="button" onClick={() => void openModule("profile")}>
              <span className="menu-icon">
                <UserOutlined />
              </span>
              <span>
                <strong>个人资料</strong>
                <small>昵称与账号信息</small>
              </span>
              <RightOutlined />
            </button>
            <button type="button" onClick={() => void openModule("security")}>
              <span className="menu-icon">
                <LockOutlined />
              </span>
              <span>
                <strong>密码与安全</strong>
                <small>登录密码和本机状态</small>
              </span>
              <RightOutlined />
            </button>
            <button type="button" onClick={() => void openModule("records")}>
              <span className="menu-icon">
                <HistoryOutlined />
              </span>
              <span>
                <strong>使用与充值记录</strong>
                <small>查看近期账户明细</small>
              </span>
              <RightOutlined />
            </button>
            <button type="button" onClick={() => void openModule("invite")}>
              <span className="menu-icon">
                <GiftOutlined />
              </span>
              <span>
                <strong>邀请有礼</strong>
                <small>朋友完成首次连接后，双方获得余额</small>
              </span>
              <RightOutlined />
            </button>
          </section>
        </aside>
      </main>
      ) : (
        <ClientModulePage
          view={moduleView}
          self={self}
          usage={usage}
          topups={topups}
          connected={connected}
          quotaPerUnit={quotaPerUnit}
          inviteInfo={inviteInfo}
          inviteUrl={inviteUrl}
          overview={platformOverview}
          overviewLoading={platformLoading}
          openIdItems={openIdItems}
          tokens={platformTokens}
          visibleKey={visibleKey}
          errors={platformErrors}
          syncingIds={syncingIds}
          onBack={() => setModuleView("home")}
          onNavigate={(view) => void openModule(view)}
          onRefreshPlatform={() => void loadOpenPlatform()}
          onRefreshIds={() => void refreshIdItems()}
          onCopyId={(item, field) => void copyIdField(item, field)}
          onVisibleKey={setVisibleKey}
          onRefreshHome={refresh}
          onRecharge={requestRecharge}
          onLoggedOut={onLoggedOut}
        />
      )}

      <Modal
        title="高级设置"
        open={settingsOpen}
        footer={null}
        onCancel={() => setSettingsOpen(false)}
      >
        <div className="settings-panel">
          <div>
            <Title level={5}>重新初始化客户端</Title>
            <Text type="secondary">
              重新检测 Codex、验证账户并自动连接 AI 服务。
            </Text>
          </div>
          <Button onClick={onRerunWizard}>重新运行初始化</Button>
          {connected && (
            <Button
              danger
              loading={stopping}
              onClick={stop}
              icon={<PoweroffOutlined />}
            >
              断开 AI 服务
            </Button>
          )}
          <Divider />
          <div>
            <Title level={5}>恢复 Codex 官方状态</Title>
            <Text type="secondary">
              断开 Smart Agent 接入，并尽量恢复你原先的 Codex 登录状态。
            </Text>
          </div>
          <Button
            loading={restoringOfficialLogin}
            onClick={async () => {
              setRestoringOfficialLogin(true);
              try {
                const result = await api.restoreOfficialCodexLogin();
                if (result.login_required) {
                  message.warning("未找到可恢复的登录状态，请打开 Codex 重新登录");
                } else {
                  message.success("已恢复原先的 Codex 登录状态");
                }
                setSettingsOpen(false);
                await refresh();
              } catch (error) {
                message.error(String(error));
              } finally {
                setRestoringOfficialLogin(false);
              }
            }}
          >
            恢复原先登录状态
          </Button>
        </div>
      </Modal>

      <Modal
        title={`充值 $${rechargeAmount}`}
        open={paymentOpen}
        footer={null}
        onCancel={() => setPaymentOpen(false)}
      >
        <Text type="secondary">
          请选择支付方式。接下来会打开 Smart Agent 安全支付窗口。
        </Text>
        <div className="payment-methods">
          {topupInfo?.methods.map((method) => (
            <Button
              key={method.id}
              size="large"
              block
              loading={paymentLoading}
              onClick={() => void startCheckout(method)}
            >
              {method.label}
              {method.min_amount > 1 && (
                <Text type="secondary">（最低 ${method.min_amount}）</Text>
              )}
            </Button>
          ))}
        </div>
      </Modal>

      <Modal
        title="兑换码充值"
        open={redeemOpen}
        footer={null}
        onCancel={() => setRedeemOpen(false)}
        destroyOnClose
      >
        <Form
          layout="vertical"
          onFinish={async ({ key }: { key: string }) => {
            try {
              await api.redeemTopup(key);
              message.success("兑换成功，余额已更新");
              setRedeemOpen(false);
              await refresh();
            } catch (error) {
              message.error(String(error));
            }
          }}
        >
          <Form.Item
            label="兑换码"
            name="key"
            rules={[{ required: true, message: "请输入兑换码" }]}
          >
            <Input size="large" placeholder="请输入兑换码" autoFocus />
          </Form.Item>
          <Button type="primary" size="large" htmlType="submit" block>
            立即兑换
          </Button>
        </Form>
      </Modal>
    </div>
  );
}

function ClientModulePage(props: {
  view: Exclude<ClientModule, "home">;
  self: SelfInfo | null;
  usage: UsageSummary | null;
  topups: TopUpRecord[];
  connected: boolean;
  quotaPerUnit?: number;
  inviteInfo: InviteInfo | null;
  inviteUrl: string;
  overview: OpenPlatformOverview | null;
  overviewLoading: boolean;
  openIdItems: OpenIdItem[];
  tokens: any[];
  visibleKey: string;
  errors: { overview: string; ids: string; tokens: string };
  syncingIds: boolean;
  onBack: () => void;
  onNavigate: (view: ClientModule) => void;
  onRefreshPlatform: () => void;
  onRefreshIds: () => void;
  onCopyId: (item: OpenIdItem, field: "username" | "password") => void;
  onVisibleKey: (value: string) => void;
  onRefreshHome: () => Promise<void>;
  onRecharge: () => void;
  onLoggedOut: () => void;
}) {
  const { message, modal } = AntApp.useApp();
  const displayName =
    props.self?.display_name || props.self?.username || "Smart Agent 用户";
  const moduleMeta: Record<
    Exclude<ClientModule, "home">,
    { eyebrow: string; title: string; description: string; icon: ReactNode }
  > = {
    platform: {
      eyebrow: "SMART AGENT OPEN PLATFORM",
      title: "开放平台",
      description: "查看开放能力、ONE 模型与接入入口。",
      icon: <ApiOutlined />,
    },
    "apple-id": {
      eyebrow: "APPLE ID",
      title: "Apple ID",
      description: "查看当前可用账号并按需复制登录信息。",
      icon: <DatabaseOutlined />,
    },
    "api-keys": {
      eyebrow: "API ACCESS",
      title: "API 密钥",
      description: "管理当前账户用于 IDE、Agent 与开发工具的密钥。",
      icon: <KeyOutlined />,
    },
    profile: {
      eyebrow: "ACCOUNT PROFILE",
      title: "个人资料",
      description: "查看账户信息并修改客户端显示昵称。",
      icon: <UserOutlined />,
    },
    security: {
      eyebrow: "ACCOUNT SECURITY",
      title: "密码与安全",
      description: "管理登录密码、本机连接状态与账户登录。",
      icon: <LockOutlined />,
    },
    records: {
      eyebrow: "USAGE & BILLING",
      title: "使用与充值记录",
      description: "查看账户用量概览和近期充值结果。",
      icon: <HistoryOutlined />,
    },
    invite: {
      eyebrow: "INVITE REWARDS",
      title: "邀请有礼",
      description: "分享专属邀请链接，朋友完成首次连接后奖励自动到账。",
      icon: <GiftOutlined />,
    },
  };
  const meta = moduleMeta[props.view];
  const balance = props.self
    ? quotaToUsd(props.self.quota, props.quotaPerUnit)
    : "--";

  const copyToken = async (token: any) => {
    try {
      const key = await api.fetchTokenKey(Number(token.id));
      props.onVisibleKey(key);
      await navigator.clipboard.writeText(key);
      message.success("API 密钥已复制");
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    }
  };

  const provisionKey = async () => {
    try {
      await api.autoProvision();
      message.success("账户密钥已准备");
      props.onRefreshPlatform();
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <main className={`module-page module-${props.view}`}>
      <div className="module-topbar">
        <Button
          type="text"
          icon={<ArrowLeftOutlined />}
          onClick={props.onBack}
          className="module-back"
        >
          返回首页
        </Button>
        <Button
          type="text"
          icon={<ReloadOutlined spin={props.overviewLoading} />}
          onClick={
            props.view === "platform" ||
            props.view === "apple-id" ||
            props.view === "api-keys"
              ? props.onRefreshPlatform
              : () => void props.onRefreshHome()
          }
        >
          刷新
        </Button>
      </div>

      <header className="module-heading">
        <span className="module-heading-icon">{meta.icon}</span>
        <div>
          <Text className="module-eyebrow">{meta.eyebrow}</Text>
          <Title level={2}>{meta.title}</Title>
          <Text type="secondary">{meta.description}</Text>
        </div>
      </header>

      {props.view === "platform" && (
        <Skeleton active loading={props.overviewLoading} paragraph={{ rows: 6 }}>
          <div className="platform-dashboard">
            {props.errors.overview && (
              <Alert
                type="error"
                showIcon
                message="开放能力暂时无法读取"
                description={props.errors.overview}
              />
            )}
            <section className="platform-overview-card">
              <div>
                <Text type="secondary">可用余额</Text>
                <strong>${balance}</strong>
                <span>一个账户统一用于客户端与开放能力</span>
              </div>
              <Button
                type="primary"
                icon={<LinkOutlined />}
                onClick={() =>
                  void api.openExternalUrl(
                    props.overview?.docs_url ||
                      "https://platform.smart-agent.eu.cc/",
                  )
                }
              >
                进入网页版控制台
              </Button>
            </section>
            <div className="platform-capability-grid">
              <button
                type="button"
                onClick={() => props.onNavigate("api-keys")}
              >
                <KeyOutlined />
                <span>
                  <strong>{props.tokens.length}</strong>
                  <small>账户密钥</small>
                </span>
                <RightOutlined />
              </button>
              <button
                type="button"
                onClick={() => props.onNavigate("apple-id")}
              >
                <DatabaseOutlined />
                <span>
                  <strong>{props.overview?.id_count ?? 0}</strong>
                  <small>可用 Apple ID</small>
                </span>
                <RightOutlined />
              </button>
              <div>
                <ApiOutlined />
                <span>
                  <strong>{props.overview?.one_models?.length ?? 0}</strong>
                  <small>ONE 模型</small>
                </span>
              </div>
            </div>
            <section className="module-card model-catalog">
              <div className="module-card-heading">
                <div>
                  <Title level={4}>ONE 实验室模型</Title>
                  <Text type="secondary">
                    当前账户可以接入的 ONE 系列模型
                  </Text>
                </div>
                <Button
                  onClick={() =>
                    void api.openExternalUrl(
                      "https://platform.smart-agent.eu.cc/docs",
                    )
                  }
                >
                  查看接入文档
                </Button>
              </div>
              <div className="model-catalog-list">
                {(props.overview?.one_models || []).map((model) => (
                  <div key={model}>
                    <span className="model-mark">
                      <CheckCircleOutlined />
                    </span>
                    <strong>{model}</strong>
                    <Tag color="blue">可用</Tag>
                  </div>
                ))}
                {props.overview && !props.overview.one_models?.length && (
                  <Empty
                    image={Empty.PRESENTED_IMAGE_SIMPLE}
                    description="暂时没有可用的 ONE 模型"
                  />
                )}
              </div>
            </section>
          </div>
        </Skeleton>
      )}

      {props.view === "apple-id" && (
        <Skeleton active loading={props.overviewLoading} paragraph={{ rows: 6 }}>
          <div className="apple-module">
            <section
              className={`apple-access-card ${
                props.overview?.id_unlocked ? "is-unlocked" : ""
              }`}
            >
              <div className="apple-access-icon">
                {props.overview?.id_unlocked ? (
                  <UnlockOutlined />
                ) : (
                  <LockOutlined />
                )}
              </div>
              <div>
                <Text type="secondary">当前权益</Text>
                <Title level={3}>
                  {props.overview?.unlock_label ||
                    (props.overview?.id_unlocked ? "已解锁" : "暂未解锁")}
                </Title>
                <p>
                  {props.overview?.unlock_hint ||
                    "满足当前套餐或充值条件后即可使用。"}
                </p>
              </div>
              {props.overview?.id_unlocked ? (
                <Button
                  icon={<SyncOutlined spin={props.syncingIds} />}
                  loading={props.syncingIds}
                  onClick={props.onRefreshIds}
                >
                  同步账号
                </Button>
              ) : (
                <Button type="primary" onClick={props.onRecharge}>
                  去充值
                </Button>
              )}
            </section>

            <section className="module-card apple-list-card">
              <div className="module-card-heading">
                <div>
                  <Title level={4}>可用账号</Title>
                  <Text type="secondary">
                    {props.overview?.id_unlocked
                      ? `共 ${props.openIdItems.length} 个，敏感信息仅在复制时读取`
                      : "解锁后在这里查看可用账号"}
                  </Text>
                </div>
              </div>
              {!props.overview ? (
                <Empty description="暂时无法读取，请稍后刷新" />
              ) : !props.overview.id_unlocked ? (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description="当前账户尚未解锁 Apple ID"
                />
              ) : props.errors.ids ? (
                <Alert
                  type="error"
                  showIcon
                  message="账号列表暂时无法读取"
                  description={props.errors.ids}
                />
              ) : props.openIdItems.length === 0 ? (
                <Empty description="暂时没有可用账号，请稍后同步" />
              ) : (
                <div className="apple-account-list">
                  {props.openIdItems.map((item) => (
                    <article key={item.id}>
                      <span className="apple-account-icon">
                        <DatabaseOutlined />
                      </span>
                      <div>
                        <strong>{item.label || "Apple ID"}</strong>
                        <small>{item.masked_username || "账号已隐藏"}</small>
                      </div>
                      <Tag color={item.status === "正常" ? "success" : "default"}>
                        {item.status || "可用"}
                      </Tag>
                      <span className="apple-meta">
                        {item.region || "—"} · {item.last_check || "—"}
                      </span>
                      <div className="apple-actions">
                        <Button
                          onClick={() => props.onCopyId(item, "username")}
                        >
                          复制账号
                        </Button>
                        <Button
                          type="primary"
                          onClick={() => props.onCopyId(item, "password")}
                        >
                          复制密码
                        </Button>
                      </div>
                    </article>
                  ))}
                </div>
              )}
            </section>
          </div>
        </Skeleton>
      )}

      {props.view === "api-keys" && (
        <Skeleton active loading={props.overviewLoading} paragraph={{ rows: 6 }}>
          <div className="keys-module">
            <Alert
              type="info"
              showIcon
              message="一个账户只需使用一个客户端密钥"
              description="重新安装或更换设备后登录同一账户，会继续使用该账户密钥。"
            />
            <section className="module-card key-list-card">
              <div className="module-card-heading">
                <div>
                  <Title level={4}>账户密钥</Title>
                  <Text type="secondary">
                    仅在需要接入其他 IDE 或 Agent 时复制完整密钥
                  </Text>
                </div>
                <Button
                  icon={<LinkOutlined />}
                  onClick={() =>
                    void api.openExternalUrl(
                      "https://platform.smart-agent.eu.cc/docs",
                    )
                  }
                >
                  接入文档
                </Button>
              </div>
              {props.errors.tokens ? (
                <Alert
                  type="error"
                  showIcon
                  message="密钥暂时无法读取"
                  description={props.errors.tokens}
                />
              ) : props.tokens.length === 0 ? (
                <div className="key-empty">
                  <span className="key-empty-icon">
                    <KeyOutlined />
                  </span>
                  <Title level={4}>账户密钥尚未准备</Title>
                  <Text type="secondary">
                    点击下方按钮，为当前账户准备唯一的客户端密钥。
                  </Text>
                  <Button type="primary" onClick={() => void provisionKey()}>
                    准备账户密钥
                  </Button>
                </div>
              ) : (
                <div className="key-table">
                  <div className="key-table-head">
                    <span>名称</span>
                    <span>密钥</span>
                    <span>操作</span>
                  </div>
                  {props.tokens.map((token) => (
                    <div className="key-table-row" key={token.id}>
                      <span>
                        <KeyOutlined />
                        <strong>
                          {token.name ||
                            token.token_name ||
                            "Smart Agent 客户端"}
                        </strong>
                      </span>
                      <code>
                        {token.key || token.masked_key || "sk-••••••••••••"}
                      </code>
                      <Button type="primary" onClick={() => void copyToken(token)}>
                        复制密钥
                      </Button>
                    </div>
                  ))}
                </div>
              )}
              {props.visibleKey && (
                <div className="key-reveal">
                  <Text type="secondary">刚刚复制的密钥</Text>
                  <code>{props.visibleKey}</code>
                </div>
              )}
            </section>
          </div>
        </Skeleton>
      )}

      {props.view === "profile" && (
        <div className="profile-module">
          <section className="profile-identity-card">
            <div className="account-avatar">
              <UserOutlined />
            </div>
            <div>
              <Title level={3}>{displayName}</Title>
              <Text type="secondary">
                {props.self?.email || props.self?.username || "当前账户"}
              </Text>
            </div>
            <Tag color="success">已登录</Tag>
          </section>
          <div className="profile-grid">
            <section className="module-card">
              <Title level={4}>账户信息</Title>
              <Descriptions column={1} className="profile-details">
                <Descriptions.Item label="用户名">
                  {props.self?.username || "-"}
                </Descriptions.Item>
                <Descriptions.Item label="邮箱">
                  {props.self?.email || "未绑定"}
                </Descriptions.Item>
                <Descriptions.Item label="账户 ID">
                  {props.self?.user_id || "-"}
                </Descriptions.Item>
              </Descriptions>
            </section>
            <section className="module-card profile-form-card">
              <Title level={4}>修改昵称</Title>
              <Text type="secondary">昵称会显示在客户端右上角。</Text>
              <Form
                layout="vertical"
                initialValues={{ displayName: props.self?.display_name }}
                onFinish={async ({ displayName }: { displayName: string }) => {
                  try {
                    await api.updateProfile(displayName);
                    message.success("个人资料已更新");
                    await props.onRefreshHome();
                  } catch (error) {
                    message.error(
                      error instanceof Error ? error.message : String(error),
                    );
                  }
                }}
              >
                <Form.Item label="昵称" name="displayName">
                  <Input maxLength={20} placeholder="输入你希望显示的昵称" />
                </Form.Item>
                <Button type="primary" htmlType="submit">
                  保存修改
                </Button>
              </Form>
            </section>
          </div>
        </div>
      )}

      {props.view === "security" && (
        <div className="security-grid">
          <section className="module-card password-card">
            <div className="security-card-icon">
              <LockOutlined />
            </div>
            <Title level={4}>修改登录密码</Title>
            <Text type="secondary">
              修改成功后，本设备会退出并要求重新登录。
            </Text>
            <Form
              layout="vertical"
              onFinish={async (values: {
                originalPassword: string;
                password: string;
                confirm: string;
              }) => {
                if (values.password !== values.confirm) {
                  message.error("两次输入的新密码不一致");
                  return;
                }
                try {
                  await api.changePassword(
                    values.originalPassword,
                    values.password,
                  );
                  message.success("密码已修改，请重新登录");
                  await api.logout();
                  props.onLoggedOut();
                } catch (error) {
                  message.error(
                    error instanceof Error ? error.message : String(error),
                  );
                }
              }}
            >
              <Form.Item
                label="原密码"
                name="originalPassword"
                rules={[{ required: true, message: "请输入原密码" }]}
              >
                <Input.Password />
              </Form.Item>
              <Form.Item
                label="新密码"
                name="password"
                rules={[
                  { required: true, message: "请输入新密码" },
                  { min: 8, max: 20, message: "请输入 8–20 位密码" },
                ]}
              >
                <Input.Password />
              </Form.Item>
              <Form.Item
                label="确认新密码"
                name="confirm"
                rules={[{ required: true, message: "请再次输入新密码" }]}
              >
                <Input.Password />
              </Form.Item>
              <Button type="primary" htmlType="submit" block>
                修改密码
              </Button>
            </Form>
          </section>

          <div className="security-side">
            <section className="module-card device-card">
              <div className="security-card-icon">
                <DesktopOutlined />
              </div>
              <Title level={4}>当前设备</Title>
              <Text type="secondary">Smart Agent macOS 客户端</Text>
              <div className="device-status-line">
                <span>
                  <CheckCircleOutlined />
                  账户已登录
                </span>
                <Tag color={props.connected ? "success" : "default"}>
                  {props.connected ? "AI 服务已连接" : "AI 服务未连接"}
                </Tag>
              </div>
            </section>
            <section className="module-card logout-card">
              <Title level={4}>退出当前账户</Title>
              <Text type="secondary">
                退出后，本机需要重新输入账号和密码。
              </Text>
              <Button
                danger
                icon={<LogoutOutlined />}
                onClick={() =>
                  modal.confirm({
                    title: "退出登录？",
                    content: "本设备会退出当前账号。",
                    okText: "退出",
                    cancelText: "取消",
                    onOk: async () => {
                      await api.logout();
                      props.onLoggedOut();
                    },
                  })
                }
              >
                退出登录
              </Button>
            </section>
          </div>
        </div>
      )}

      {props.view === "records" && (
        <div className="records-module">
          <div className="records-summary">
            <div>
              <Text type="secondary">当前余额</Text>
              <strong>${balance}</strong>
            </div>
            <div>
              <Text type="secondary">今日请求</Text>
              <strong>{props.usage?.today_requests ?? 0}</strong>
            </div>
            <div>
              <Text type="secondary">今日消耗</Text>
              <strong>
                $
                {props.usage
                  ? quotaToUsd(
                      props.usage.today_quota,
                      props.quotaPerUnit,
                    )
                  : "--"}
              </strong>
            </div>
            <Button type="primary" onClick={props.onRecharge}>
              充值余额
            </Button>
          </div>
          <section className="module-card record-table-card">
            <div className="module-card-heading">
              <div>
                <Title level={4}>充值记录</Title>
                <Text type="secondary">展示当前账户最近的充值结果</Text>
              </div>
              <Text type="secondary">最近 {props.topups.length} 条</Text>
            </div>
            {!props.self ? (
              <Skeleton active paragraph={{ rows: 4 }} />
            ) : props.topups.length === 0 ? (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description="暂无充值记录"
              />
            ) : (
              <div className="record-table">
                <div className="record-table-head">
                  <span>充值金额</span>
                  <span>支付方式</span>
                  <span>时间</span>
                  <span>状态</span>
                </div>
                {props.topups.map((record) => (
                  <div className="record-table-row" key={record.id}>
                    <strong>${record.amount}</strong>
                    <span>{record.payment_method || "账户充值"}</span>
                    <span>
                      {new Date(record.create_time * 1000).toLocaleString()}
                    </span>
                    <TopupStatus status={record.status} />
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>
      )}

      {props.view === "invite" && (
        <div className="invite-module">
          <section className="invite-hero-card">
            <div>
              <Text className="module-eyebrow">YOUR INVITE CODE</Text>
              <Title level={2}>{props.inviteInfo?.aff_code || "----"}</Title>
              <p>
                朋友通过你的链接注册，并在客户端完成首次连接后，双方奖励会自动到账。
              </p>
              <Button
                type="primary"
                size="large"
                icon={<CopyOutlined />}
                disabled={!props.inviteUrl}
                onClick={async () => {
                  try {
                    await navigator.clipboard.writeText(props.inviteUrl);
                    message.success("邀请链接已复制");
                  } catch {
                    message.info(props.inviteUrl);
                  }
                }}
              >
                复制邀请链接
              </Button>
            </div>
            <div className="invite-number">
              <strong>{props.inviteInfo?.aff_count ?? 0}</strong>
              <span>位朋友已加入</span>
            </div>
          </section>
          <div className="invite-reward-grid">
            <section className="module-card">
              <GiftOutlined />
              <Text type="secondary">你获得的单次奖励</Text>
              <strong>
                $
                {props.inviteInfo
                  ? quotaToUsd(
                      props.inviteInfo.inviter_reward,
                      props.quotaPerUnit,
                    )
                  : "--"}
              </strong>
            </section>
            <section className="module-card">
              <UserOutlined />
              <Text type="secondary">新用户获得的奖励</Text>
              <strong>
                $
                {props.inviteInfo
                  ? quotaToUsd(
                      props.inviteInfo.invitee_reward,
                      props.quotaPerUnit,
                    )
                  : "--"}
              </strong>
            </section>
            <section className="module-card">
              <WalletOutlined />
              <Text type="secondary">累计邀请奖励</Text>
              <strong>
                $
                {props.inviteInfo
                  ? quotaToUsd(
                      props.inviteInfo.aff_history_quota,
                      props.quotaPerUnit,
                    )
                  : "--"}
              </strong>
            </section>
          </div>
        </div>
      )}
    </main>
  );
}

function TopupStatus({ status }: { status: string }) {
  if (status === "success") return <Tag color="success">已到账</Tag>;
  if (status === "pending") return <Tag color="processing">处理中</Tag>;
  if (status === "failed") return <Tag color="error">失败</Tag>;
  return <Tag>{status || "未知"}</Tag>;
}

export default Home;
