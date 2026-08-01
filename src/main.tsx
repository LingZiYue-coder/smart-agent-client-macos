import React from "react";
import ReactDOM from "react-dom/client";
import { App as AntApp, ConfigProvider } from "antd";
import zhCN from "antd/locale/zh_CN";
import App from "./App";
import "./App.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ConfigProvider
      locale={zhCN}
      theme={{
        token: {
          colorPrimary: "#2168f5",
          colorInfo: "#2168f5",
          colorSuccess: "#16a34a",
          colorText: "#101828",
          colorTextSecondary: "#667085",
          colorBorder: "#dfe3ea",
          borderRadius: 9,
          fontFamily:
            'Inter, "Segoe UI", "Microsoft YaHei", "PingFang SC", system-ui, sans-serif',
        },
        components: {
          Button: {
            controlHeightLG: 44,
            fontWeight: 550,
          },
          Modal: {
            borderRadiusLG: 16,
          },
          Drawer: {
            colorBgElevated: "#ffffff",
          },
        },
      }}
    >
      <AntApp>
        <App />
      </AntApp>
    </ConfigProvider>
  </React.StrictMode>,
);
