import {
  BorderOutlined,
  CloseOutlined,
  MinusOutlined,
} from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";

function WindowControls() {
  // macOS 使用系统原生标题栏与红黄绿窗口按钮。
  if (/Macintosh|Mac OS X/i.test(navigator.userAgent)) {
    return null;
  }

  const run = (command: string) => {
    void invoke(command).catch(() => undefined);
  };

  return (
    <div className="window-controls" aria-label="窗口控制">
      <button
        type="button"
        aria-label="最小化"
        onClick={() => run("minimize_main_window")}
      >
        <MinusOutlined />
      </button>
      <button
        type="button"
        aria-label="最大化或还原"
        onClick={() => run("toggle_maximize_main_window")}
      >
        <BorderOutlined />
      </button>
      <button
        type="button"
        className="is-close"
        aria-label="关闭到托盘"
        onClick={() => run("hide_main_window")}
      >
        <CloseOutlined />
      </button>
    </div>
  );
}

export default WindowControls;
