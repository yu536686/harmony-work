import { PRODUCT_BRAND } from "../branding";
import "./BrandWordmark.css";

export default function BrandWordmark({ className = "" }: { className?: string }) {
  return (
    <span
      className={`brand-wordmark ${className}`.trim()}
      aria-label={PRODUCT_BRAND.displayName}
    >
      <span className="brand-wordmark-harmony" aria-hidden="true">Harmony</span>
      <span className="brand-wordmark-work" aria-hidden="true">Work</span>
    </span>
  );
}
