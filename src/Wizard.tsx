import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  App as AntApp,
  Button,
  Card,
  Checkbox,
  Form,
  Input,
  Modal,
  Result,
  Space,
  Steps,
  Tabs,
  Typography,
} from "antd";
import {
  api,
  ApplyResult,
  ClientConfig,
  CodexEnv,
  ProvisionResult,
  SiteStatus,
} from "./api";
import BrandOrb from "./BrandOrb";
import WindowControls from "./WindowControls";

const { Paragraph } = Typography;

interface WizardProps {
  onDone: () => void;
}

/**
 * 四步向导：环境检测 → 登录/注册（含 2FA）→ 自动准备服务 → 一键接入。
 * 全程不出现任何站点地址的输入或展示（品牌锁定）。
 */
function Wizard({ onDone }: WizardProps) {
  const { message } = AntApp.useApp();
  const [step, setStep] = useState(0);

  // 步骤 1：环境检测
  const [env, setEnv] = useState<CodexEnv | null>(null);
  const [status, setStatus] = useState<SiteStatus | null>(null);
  const [siteError, setSiteError] = useState<string | null>(null);
  const [clientConfig, setClientConfig] = useState<ClientConfig | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [installingCodex, setInstallingCodex] = useState(false);

  // 步骤 2：账号
  const [loggedIn, setLoggedIn] = useState(false);
  const [loginLoading, setLoginLoading] = useState(false);
  const [flowToken, setFlowToken] = useState<string | null>(null);
  const [loginName, setLoginName] = useState<string>("");
  const [agreementAccepted, setAgreementAccepted] = useState(false);
  const [agreementOpen, setAgreementOpen] = useState(false);

  // 步骤 3：准备本机服务
  const [provision, setProvision] = useState<ProvisionResult | null>(null);
  const [provisioning, setProvisioning] = useState(false);
  const [provisionAttempted, setProvisionAttempted] = useState(false);

  // 步骤 4：一键接入
  const [applyResult, setApplyResult] = useState<ApplyResult | null>(null);
  const [applying, setApplying] = useState(false);

  const runDetect = useCallback(async () => {
    setDetecting(true);
    setSiteError(null);
    try {
      const e = await api.detectCodex();
      setEnv(e);
    } catch (err) {
      message.error(String(err));
    }
    try {
      const [s, config] = await Promise.all([
        api.getStatus(),
        api.getClientConfig(),
      ]);
      setStatus(s);
      setClientConfig(config);
    } catch (err) {
      setStatus(null);
      setSiteError(String(err));
    }
    setDetecting(false);
  }, [message]);

  useEffect(() => {
    void runDetect();
  }, [runDetect]);

  const handleLogin = async (values: { username: string; password: string }) => {
    if (!agreementAccepted) {
      message.warning("请先阅读并同意用户协议");
      return;
    }
    setLoginLoading(true);
    try {
      const outcome = await api.login(values.username, values.password);
      if (outcome.require_2fa) {
        setFlowToken(outcome.flow_token);
        message.info("该账号已开启两步验证，请输入动态验证码");
      } else {
        await api.ensurePat();
        setLoggedIn(true);
        setLoginName(outcome.username ?? values.username);
        message.success("登录成功");
      }
    } catch (err) {
      message.error(String(err));
    }
    setLoginLoading(false);
  };

  const handle2fa = async (values: { code: string }) => {
    if (!flowToken) return;
    setLoginLoading(true);
    try {
      const outcome = await api.login2fa(flowToken, values.code);
      await api.ensurePat();
      setLoggedIn(true);
      setLoginName(outcome.username ?? "");
      setFlowToken(null);
      message.success("登录成功");
    } catch (err) {
      message.error(String(err));
    }
    setLoginLoading(false);
  };

  const handleRegister = async (values: {
    username: string;
    password: string;
    confirm: string;
    inviteCode?: string;
  }) => {
    if (!agreementAccepted) {
      message.warning("请先阅读并同意用户协议");
      return;
    }
    if (values.password !== values.confirm) {
      message.error("两次输入的密码不一致");
      return;
    }
    setLoginLoading(true);
    try {
      await api.register(values.username, values.password, values.inviteCode);
      message.success("注册成功，正在自动登录…");
      await handleLogin({ username: values.username, password: values.password });
    } catch (err) {
      message.error(String(err));
    }
    setLoginLoading(false);
  };

  const handleProvision = async () => {
    setProvisionAttempted(true);
    setProvisioning(true);
    try {
      const r = await api.autoProvision();
      setProvision(r);
      message.success(r.created ? "账户已准备好" : "账户设置已恢复");
      const claim = await api.claimInviteReward().catch(() => null);
      if (claim?.rewarded) {
        message.success(claim.message || "邀请奖励已到账");
      }
    } catch (err) {
      message.error(String(err));
    }
    setProvisioning(false);
  };

  useEffect(() => {
    if (step === 2 && loggedIn && provision === null && !provisioning && !provisionAttempted) {
      void handleProvision();
    }
  }, [step, loggedIn, provision, provisioning, provisionAttempted]);

  const openCodexStore = async () => {
    try {
      await api.openExternalUrl("https://chatgpt.com/download/");
    } catch (err) {
      message.error(String(err));
    }
  };

  const installCodex = async () => {
    setInstallingCodex(true);
    try {
      const result = await api.installCodexDesktop();
      message.success(result.message || "Codex 客户端安装完成");
      await runDetect();
    } catch (err) {
      message.error(String(err));
    }
    setInstallingCodex(false);
  };

  const handleApply = async () => {
    const desktopRunning = await api.codexDesktopRunning().catch(() => false);
    if (desktopRunning) {
      const proceed = await new Promise<boolean>((resolve) => {
        Modal.confirm({
          title: "检测到 Codex 正在运行",
          content: "继续后会自动完成接入。请先保存 Codex 中尚未完成的内容。",
          okText: "立即接入",
          cancelText: "返回",
          onOk: () => resolve(true),
          onCancel: () => resolve(false),
        });
      });
      if (!proceed) return;
    }
    setApplying(true);
    try {
      const r = await api.applyCodexConfig();
      setApplyResult(r);
      if (r.desktop_model_picker_ready) {
        message.success("连接设置已完成");
      } else {
        message.warning("连接已完成。建议先在 Codex 登录官方账号，再重新打开 Codex。");
      }
    } catch (err) {
      message.error(String(err));
    }
    setApplying(false);
  };

  const finishWizard = async () => {
    try {
      await api.setWizardDone(true);
    } catch (err) {
      message.error(String(err));
      return;
    }
    onDone();
  };

  const registerEnabled =
    status?.register_enabled !== false && status?.password_register_enabled !== false;

  const stepEnv = (
    <Card title="第 1 步 · 环境检查" loading={detecting && env === null}>
      <Space direction="vertical" style={{ width: "100%" }} size="middle">
        {env && status && (
          <Result
            status={env.installed ? "success" : "warning"}
            title={env.installed ? "环境检查通过" : "尚未安装所需组件"}
            subTitle={
              env.installed
                ? "你的设备已准备好，可以继续。"
                : "请先安装 Codex，然后重新检查。"
            }
          />
        )}

        {env && !env.installed && (
          <Alert
            type="warning"
            showIcon
            message="需要先安装 Codex 客户端"
            description="安装完成后回到这里点「重新检测」，通过后即可继续。"
          />
        )}
        {env?.keyring_warning && (
          <Alert
            type="warning"
            showIcon
            message="当前系统设置可能影响启动"
            description="如果稍后启动失败，请联系客服协助调整。"
          />
        )}
        {env?.config_parse_error && (
          <Alert
            type="warning"
            showIcon
            message="检测到旧设置异常"
            description="启动时会自动保护旧设置并尝试修复。"
          />
        )}
        {siteError && (
          <Alert
            type="error"
            showIcon
            message="站点服务不可达"
            description="暂时无法连接服务，请稍后重试。"
          />
        )}
        {status?.turnstile_check === true && (
          <Alert
            type="error"
            showIcon
            message="站点开启了人机校验（Turnstile）"
            description="客户端内登录暂不支持人机校验，请联系站点管理员关闭后重试。"
          />
        )}

        <Space>
          <Button onClick={() => void runDetect()} loading={detecting}>
            重新检测
          </Button>
          {!env?.installed && (
            <>
              <Button onClick={() => void openCodexStore()}>前往官方安装</Button>
              <Button onClick={() => void installCodex()} loading={installingCodex}>
                自动安装
              </Button>
            </>
          )}
          <Button
            type="primary"
            disabled={status === null || !env?.installed}
            onClick={() => setStep(1)}
          >
            下一步
          </Button>
        </Space>
      </Space>
    </Card>
  );

  const loginForm = (
    <Form layout="vertical" onFinish={handleLogin} disabled={loginLoading}>
      <Form.Item
        label="用户名"
        name="username"
        rules={[{ required: true, message: "请输入用户名" }]}
      >
        <Input autoComplete="username" placeholder="用户名" />
      </Form.Item>
      <Form.Item
        label="密码"
        name="password"
        rules={[{ required: true, message: "请输入密码" }]}
      >
        <Input.Password autoComplete="current-password" placeholder="密码" />
      </Form.Item>
      <Button type="primary" htmlType="submit" loading={loginLoading} block>
        登录
      </Button>
    </Form>
  );

  const registerForm = (
    <Form layout="vertical" onFinish={handleRegister} disabled={loginLoading}>
      <Form.Item
        label="用户名"
        name="username"
        rules={[
          { required: true, message: "请输入用户名" },
          { max: 12, message: "用户名最长 12 个字符" },
        ]}
      >
        <Input autoComplete="username" placeholder="用户名" />
      </Form.Item>
      <Form.Item
        label="邀请码"
        name="inviteCode"
        extra="朋友邀请你时填写；没有可以留空"
      >
        <Input placeholder="可选" maxLength={32} />
      </Form.Item>
      <Form.Item
        label="密码"
        name="password"
        rules={[
          { required: true, message: "请输入密码" },
          { min: 8, max: 20, message: "密码长度需在 8-20 位之间" },
        ]}
      >
        <Input.Password autoComplete="new-password" placeholder="密码（8-20 位）" />
      </Form.Item>
      <Form.Item
        label="确认密码"
        name="confirm"
        rules={[{ required: true, message: "请再次输入密码" }]}
      >
        <Input.Password autoComplete="new-password" placeholder="再次输入密码" />
      </Form.Item>
      <Button type="primary" htmlType="submit" loading={loginLoading} block>
        注册并登录
      </Button>
    </Form>
  );

  const twoFaForm = (
    <Form layout="vertical" onFinish={handle2fa} disabled={loginLoading}>
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="两步验证"
        description="请输入验证器 App 中的 6 位动态验证码（或一次性备用码）。验证会话 5 分钟内有效。"
      />
      <Form.Item
        label="动态验证码"
        name="code"
        rules={[{ required: true, message: "请输入验证码" }]}
      >
        <Input placeholder="6 位验证码" maxLength={16} />
      </Form.Item>
      <Space>
        <Button type="primary" htmlType="submit" loading={loginLoading}>
          验证并登录
        </Button>
        <Button onClick={() => setFlowToken(null)}>返回重新登录</Button>
      </Space>
    </Form>
  );

  const stepAccount = (
    <Card title="第 2 步 · 登录 / 注册">
      {loggedIn ? (
        <Result
          status="success"
          title={`已登录：${loginName}`}
          subTitle="账户已就绪，可以继续下一步。"
          extra={
            <Button type="primary" onClick={() => setStep(2)}>
              下一步
            </Button>
          }
        />
      ) : flowToken ? (
        twoFaForm
      ) : (
        <Tabs
          items={[
            { key: "login", label: "登录", children: loginForm },
            ...(registerEnabled
              ? [{ key: "register", label: "注册", children: registerForm }]
              : []),
          ]}
        />
      )}
      {!loggedIn && !flowToken && (
        <>
          <Checkbox
            checked={agreementAccepted}
            onChange={(event) => setAgreementAccepted(event.target.checked)}
            style={{ marginTop: 16 }}
          >
            我已阅读并同意
            <Button type="link" size="small" onClick={() => setAgreementOpen(true)}>
              《用户协议》
            </Button>
          </Checkbox>
          <Button style={{ marginTop: 8, display: "block" }} onClick={() => setStep(0)}>
            上一步
          </Button>
        </>
      )}
      <Modal
        title="用户协议"
        open={agreementOpen}
        footer={null}
        onCancel={() => setAgreementOpen(false)}
      >
        <Typography.Paragraph style={{ whiteSpace: "pre-wrap" }}>
          {clientConfig?.user_agreement?.trim() ||
            "使用 Smart Agent 即表示你同意遵守服务规则，妥善保管账户，并仅将服务用于合法、授权的用途。"}
        </Typography.Paragraph>
      </Modal>
    </Card>
  );

  const stepProvision = (
    <Card title="第 3 步 · 完成准备">
      <Space direction="vertical" style={{ width: "100%" }} size="middle">
        <Paragraph type="secondary" style={{ marginBottom: 0 }}>
          正在完成必要设置。完成后即可连接 Codex，余额和邀请奖励也会自动同步。
        </Paragraph>
        {provisioning && (
          <Alert type="info" showIcon message="正在准备" description="这一步会自动完成，无需额外设置。" />
        )}
        {provision && (
          <Result status="success" title="准备完成" />
        )}
        <Space>
          <Button onClick={() => setStep(1)}>上一步</Button>
          {provision === null ? (
            <Button type="primary" onClick={() => void handleProvision()} loading={provisioning}>
              {provisionAttempted ? "重新准备" : "准备中"}
            </Button>
          ) : (
            <Button type="primary" onClick={() => setStep(3)}>
              下一步
            </Button>
          )}
        </Space>
      </Space>
    </Card>
  );

  const stepApply = (
    <Card title="第 4 步 · 开始使用">
      <Space direction="vertical" style={{ width: "100%" }} size="middle">
        <Alert
          type="info"
          showIcon
          message="已准备就绪"
          description="点击开始使用后，Smart Agent 会自动完成连接设置。"
        />
        {applyResult && (
          <Alert
            type={applyResult.desktop_model_picker_ready ? "success" : "warning"}
            showIcon
            message={
              applyResult.desktop_model_picker_ready
                ? "启动设置已完成，重新打开 Codex 即可使用"
                : "启动设置已完成。建议在 Codex 登录官方账号后，再重新打开使用"
            }
          />
        )}
        <Space>
          <Button onClick={() => setStep(2)}>上一步</Button>
          {applyResult === null ? (
            <Button
              type="primary"
              onClick={() => void handleApply()}
              loading={applying}
            >
              开始使用
            </Button>
          ) : (
            <Button type="primary" onClick={() => void finishWizard()}>
              完成
            </Button>
          )}
        </Space>
      </Space>
    </Card>
  );

  const contents = [stepEnv, stepAccount, stepProvision, stepApply];

  return (
    <div className="wizard-shell">
      <header className="app-header wizard-header" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <BrandOrb size={30} glowing={false} />
          <div data-tauri-drag-region>
            <strong>Smart Agent</strong>
            <span>初始化</span>
          </div>
        </div>
        <WindowControls />
      </header>
      <div className="page">
        <Typography.Title level={3} style={{ marginTop: 8 }}>
          Smart Agent 初始化
        </Typography.Title>
        <Steps
          current={step}
          size="small"
          style={{ margin: "16px 0 24px" }}
          items={[
            { title: "环境检查" },
            { title: "登录 / 注册" },
            { title: "完成准备" },
            { title: "开始使用" },
          ]}
        />
        {contents[step]}
      </div>
    </div>
  );
}

export default Wizard;
