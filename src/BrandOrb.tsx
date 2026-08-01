/** 品牌 LOGO：根目录官方形象图 */
import brandMark from "./assets/logo.png";

/** Gate AI 形象球：顶栏 / 状态区共用 */
export default function BrandOrb({
  size = 36,
  glowing = false,
  className = "",
}: {
  size?: number;
  glowing?: boolean;
  className?: string;
}) {
  return (
    <span
      className={`brand-orb${glowing ? " is-glowing" : ""}${className ? ` ${className}` : ""}`}
      style={{ width: size, height: size }}
      aria-hidden
    >
      <img
        src={brandMark}
        alt=""
        width={size}
        height={size}
        draggable={false}
        className="brand-orb-img"
      />
    </span>
  );
}
