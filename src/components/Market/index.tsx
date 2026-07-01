import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import SchoolPage, { type SchoolSubTab } from "../School";
import Pond from "../Pond";
import Skills from "../Skills";
import "./Market.css";

export type MarketTab = "experts" | "team" | "skills";

interface MarketProps {
  initialTab?: MarketTab;
  initialSchoolSubTab?: SchoolSubTab;
  onNavigateToChat?: () => void;
}

const MARKET_TABS: Array<{ id: MarketTab; icon: string; labelKey: string; descKey: string }> = [
  { id: "experts", icon: "👥", labelKey: "market.experts", descKey: "market.expertsDesc" },
  { id: "team", icon: "🏢", labelKey: "market.team", descKey: "market.teamDesc" },
  { id: "skills", icon: "⚡", labelKey: "market.skills", descKey: "market.skillsDesc" },
];

export default function Market({ initialTab = "experts", initialSchoolSubTab = "fish", onNavigateToChat }: MarketProps) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<MarketTab>(initialTab);
  const [schoolSubTab, setSchoolSubTab] = useState<SchoolSubTab>(initialSchoolSubTab);

  useEffect(() => {
    setActiveTab(initialTab);
  }, [initialTab]);

  useEffect(() => {
    setSchoolSubTab(initialSchoolSubTab);
  }, [initialSchoolSubTab]);

  return (
    <div className="page market-page">
      <div className="market-shell-header">
        <div>
          <div className="market-kicker">{t("market.kicker")}</div>
          <h1 className="market-title">{t("market.title")}</h1>
        </div>
      </div>

      <div className="market-tabs">
        {MARKET_TABS.map((tab) => (
          <button
            key={tab.id}
            type="button"
            className={`market-tab ${activeTab === tab.id ? "active" : ""}`}
            onClick={() => setActiveTab(tab.id)}
          >
            <span className="market-tab-icon">{tab.icon}</span>
            <span className="market-tab-copy">
              <strong>{t(tab.labelKey)}</strong>
              <small>{t(tab.descKey)}</small>
            </span>
          </button>
        ))}
      </div>

      <div className="market-content">
        <div className="market-panel" hidden={activeTab !== "experts"}>
          <SchoolPage initialSubTab={schoolSubTab} embedded />
        </div>
        <div className="market-panel" hidden={activeTab !== "team"}>
          <Pond
            embedded
            visible={activeTab === "team"}
            onNavigateToSchoolKoi={() => {
              setActiveTab("experts");
              setSchoolSubTab("koi");
            }}
          />
        </div>
        <div className="market-panel" hidden={activeTab !== "skills"}>
          <Skills embedded onNavigateTab={onNavigateToChat ? () => onNavigateToChat() : undefined} />
        </div>
      </div>
    </div>
  );
}
