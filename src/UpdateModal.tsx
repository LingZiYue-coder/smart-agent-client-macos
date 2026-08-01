import { useCallback, useEffect, useState } from "react";
import { Modal, Typography } from "antd";
import { CloudDownloadOutlined } from "@ant-design/icons";
import { api, ClientConfig, UpdateCheckResult } from "./api";
import BrandOrb from "./BrandOrb";

const { Text, Paragraph } = Typography;

/**
 * 远程版本检查：低于 min_client_version 强制提示，
 * 低于 latest_version 可选提示；「前往官网下载」打开 download_url。
 */
export default function UpdateModal({ config }: { config: ClientConfig | null }) {
  const [check, setCheck] = useState<UpdateCheckResult | null>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!config) return;
    let cancelled = false;
    void (async () => {
      try {
        const result = await api.checkClientUpdate(config);
        if (cancelled) return;
        setCheck(result);
        if (result.force_update || result.soft_update) {
          setOpen(true);
        }
      } catch {
        // 版本检查失败不阻断主流程
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [config]);

  const goDownload = useCallback(async () => {
    const url = check?.download_url?.trim();
    if (!url) return;
    try {
      await api.openExternalUrl(url);
    } catch {
      // 兜底：部分环境 invoke 失败时尝试 window.open（浏览器预览）
      window.open(url, "_blank", "noopener,noreferrer");
    }
  }, [check?.download_url]);

  if (!check || !(check.force_update || check.soft_update)) {
    return null;
  }

  const force = check.force_update;

  return (
    <Modal
      open={open}
      centered
      closable={!force}
      maskClosable={!force}
      keyboard={!force}
      onCancel={() => {
        if (!force) setOpen(false);
      }}
      title={
        <span style={{ display: "inline-flex", alignItems: "center", gap: 10 }}>
          <BrandOrb size={28} />
          {force ? "需要更新客户端" : "发现新版本"}
        </span>
      }
      okText="前往官网下载"
      okButtonProps={{ icon: <CloudDownloadOutlined /> }}
      onOk={goDownload}
      cancelText={force ? undefined : "稍后提醒"}
      cancelButtonProps={force ? { style: { display: "none" } } : undefined}
      footer={(_, { OkBtn, CancelBtn }) => (
        <>
          {!force && <CancelBtn />}
          <OkBtn />
        </>
      )}
    >
      <Paragraph style={{ marginBottom: 8 }}>
        {force
          ? "这个版本已无法继续使用，请安装最新版后重新打开 Smart Agent。"
          : `新版本 ${check.latest_version} 已可下载，更新后可获得最新功能和体验。`}
      </Paragraph>
      {check.download_url ? (
        <Text type="secondary">安装新版不会影响你的账户余额和已有设置。</Text>
      ) : (
        <Text type="danger">下载入口暂时不可用，请稍后再试。</Text>
      )}
    </Modal>
  );
}
