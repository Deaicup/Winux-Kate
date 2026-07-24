import { invoke } from "@tauri-apps/api/core";
import { useStore } from "../store";

const LINKS: Record<string, string> = {
  wechat: "https://pc.weixin.qq.com/",
  qq: "https://im.qq.com/pcqq/",
  wecom: "https://work.weixin.qq.com/",
};
const NAMES: Record<string, string> = {
  wechat: "微信",
  qq: "QQ",
  wecom: "企业微信",
};

function MissingApp({ kind }: { kind: string }) {
  return (
    <div className="im-slot missing">
      <div className="missing-card">
        <h2>未检测到 {NAMES[kind]}</h2>
        <p>请安装 {NAMES[kind]} 后重试，或通过 im_set_paths 配置路径。</p>
        <a className="btn" href={LINKS[kind]} target="_blank" rel="noreferrer">
          下载 {NAMES[kind]}
        </a>
      </div>
    </div>
  );
}

export function ImPage() {
  const imView = useStore((s) => s.imView);
  const detection = useStore((s) => s.detection);

  const ensure = (kind: string) => {
    invoke("im_launch", { kind }).catch((e) => alert(`启动失败: ${String(e)}`));
  };

  const wechatMissing = detection && !detection.wechat;
  const qqMissing = detection && !detection.qq;
  const wecomMissing = detection && !detection.wecom;

  return (
    <div className="page im">
      <div className="im-bar">
        <span className="dot" />
        {imView === "split" ? "微信 / QQ 分屏" : "企业微信 满屏"}
        <span className="page-credit">由 Deaicup 工作室制作</span>
        <button
          className="btn"
          style={{ marginLeft: "auto" }}
          onClick={() => invoke("im_toggle")}
        >
          Ctrl+Shift+Tab 切换
        </button>
      </div>
      {imView === "split" ? (
        <div className="im-split">
          {wechatMissing ? (
            <MissingApp kind="wechat" />
          ) : (
            <div
              className="im-slot"
              data-slot="wechat"
              data-slot-kind="we_chat"
              onDoubleClick={() => ensure("wechat")}
            >
              <div className="embed-empty">双击启动 微信</div>
            </div>
          )}
          {qqMissing ? (
            <MissingApp kind="qq" />
          ) : (
            <div
              className="im-slot"
              data-slot="qq"
              data-slot-kind="qq"
              onDoubleClick={() => ensure("qq")}
            >
              <div className="embed-empty">双击启动 QQ</div>
            </div>
          )}
        </div>
      ) : wecomMissing ? (
        <MissingApp kind="wecom" />
      ) : (
        <div
          className="im-full"
          data-slot="wecom"
          data-slot-kind="we_com"
          onDoubleClick={() => ensure("wecom")}
        >
          <div className="embed-empty">双击启动 企业微信</div>
        </div>
      )}
    </div>
  );
}
