import Skills from "../Skills";
import "./Market.css";

interface MarketProps {
  onNavigateToChat?: () => void;
}

/** Compatibility entry point for callers that still import the former market page. */
export default function Market({ onNavigateToChat }: MarketProps) {
  return (
    <div className="page market-page">
      <Skills embedded onNavigateTab={onNavigateToChat ? () => onNavigateToChat() : undefined} />
    </div>
  );
}
