import { SETTINGS_SECTIONS, type SettingsSection } from "./settingsSections";
import "./SettingsNavigation.css";

interface SettingsNavigationProps {
  activeSection?: SettingsSection;
  initialSection?: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
}

export default function SettingsNavigation({
  activeSection,
  initialSection,
  onSectionChange,
}: SettingsNavigationProps) {
  const selectedSection = activeSection ?? initialSection ?? SETTINGS_SECTIONS[0].id;

  return (
    <nav className="settings-navigation" aria-label="设置分类">
      <div className="settings-navigation-list" role="tablist" aria-orientation="vertical">
        {SETTINGS_SECTIONS.map((section) => {
          const Icon = section.icon;
          const selected = selectedSection === section.id;
          return (
            <button
              key={section.id}
              type="button"
              className={`settings-navigation-item${selected ? " is-active" : ""}`}
              role="tab"
              aria-selected={selected}
              aria-controls={`settings-section-${section.id}`}
              onClick={() => onSectionChange(section.id)}
            >
              <Icon size={16} strokeWidth={1.8} aria-hidden="true" />
              <span>{section.label}</span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
