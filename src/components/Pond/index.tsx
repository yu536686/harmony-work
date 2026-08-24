import { useTranslation } from "react-i18next";
import Collab from "./Collab";
import "./Pond.css";

interface PondProps {
  onNavigateToSchoolKoi?: () => void;
  visible?: boolean;
  embedded?: boolean;
}

export default function Pond({ onNavigateToSchoolKoi, visible = true, embedded = false }: PondProps) {
  const { t } = useTranslation();

  if (embedded) {
    return (
      <div className="pond pond--embedded">
        <Collab onNavigateToSchoolKoi={onNavigateToSchoolKoi} visible={visible} />
      </div>
    );
  }

  return (
    <div className="pond">
      <div className="feature-topbar">
        <h1 className="feature-topbar-title">
          <span aria-hidden>🏊</span>
          {t("pond.title")}
        </h1>
      </div>
      <div className="feature-topbar-body">
        <Collab onNavigateToSchoolKoi={onNavigateToSchoolKoi} visible={visible} />
      </div>
    </div>
  );
}
