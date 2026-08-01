import { useCallback, useEffect, useState } from "react";
import { Spin } from "antd";
import { api, ClientConfig, FrontState } from "./api";
import Wizard from "./Wizard";
import Home from "./Home";
import UpdateModal from "./UpdateModal";
import BrandOrb from "./BrandOrb";
import WindowControls from "./WindowControls";

/**
 * 顶层视图切换：首次使用（或主动重跑）显示初始化向导，否则显示状态首页。
 * 同时拉取客户端配置，驱动远程更新弹窗（跳转官网下载页）。
 */
function App() {
  const [localState, setLocalState] = useState<FrontState | null>(null);
  const [forceWizard, setForceWizard] = useState(false);
  const [clientConfig, setClientConfig] = useState<ClientConfig | null>(null);

  const reload = useCallback(async () => {
    try {
      const s = await api.getLocalState();
      setLocalState(s);
    } catch {
      setLocalState({
        wizard_done: false,
        has_pat: false,
        has_key: false,
        codex_desired: false,
        username: null,
        user_id: null,
        token_name: null,
      });
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    void api
      .getClientConfig()
      .then(setClientConfig)
      .catch(() => setClientConfig(null));
  }, []);

  if (localState === null) {
    return (
      <div className="wizard-shell">
        <header className="app-header wizard-header" data-tauri-drag-region>
          <div className="brand" data-tauri-drag-region>
            <BrandOrb size={30} glowing={false} />
            <div data-tauri-drag-region>
              <strong>Smart Agent</strong>
              <span>正在启动</span>
            </div>
          </div>
          <WindowControls />
        </header>
        <div style={{ display: "flex", justifyContent: "center", paddingTop: 160 }}>
          <Spin size="large" tip="正在加载本地状态…" />
        </div>
      </div>
    );
  }

  return (
    <>
      <UpdateModal config={clientConfig} />
      {forceWizard || !localState.wizard_done ? (
        <Wizard
          onDone={() => {
            setForceWizard(false);
            void reload();
          }}
        />
      ) : (
        <Home
          onRerunWizard={() => setForceWizard(true)}
          onLoggedOut={() => {
            setForceWizard(true);
            void reload();
          }}
        />
      )}
    </>
  );
}

export default App;
